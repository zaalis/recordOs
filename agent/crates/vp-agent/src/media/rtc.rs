//! WebRTC peer (str0m, ICE-lite, DTLS-SRTP) for one session.
//!
//! One tokio task owns the `Rtc` state machine. Network packets, encoded frames, control messages
//! and timers are serialized through a single `select!` loop, respecting str0m's
//! "mutate → drain" contract. Input from the iPad is decoded, validated and forwarded to the
//! injector thread immediately — it never waits for video.

use crate::config::{CodecId, Settings};
use crate::input::{self, InputCategory, InputEvent, SequenceFilter};
use crate::media::backlog::{FrameTracker, SendBacklog};
use crate::platform::capture::{encode_png, CursorShape, CursorState};
use crate::platform::inject::{InjectCommand, InputPolicy};
use crate::protocol::{AgentStats, AppliedVideo, ControlToAgent, DisplayInfo};
use crate::relay::{RelayMediaLink, RelaySender};
use crate::session::SessionInfo;
use crate::util::{b64u, now_ms};
use parking_lot::{Mutex, RwLock};
use serde_json::json;
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};
use str0m::bwe::{Bitrate, BweKind};
use str0m::change::SdpOffer;
use str0m::channel::ChannelId;
use str0m::format::Codec;
use str0m::media::{MediaKind, MediaTime, Mid, Pt};
use str0m::net::{Protocol, Receive};
use str0m::rtp::Extension;
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc};

pub const CONTROL_LABEL: &str = "vp-control";
pub const INPUT_LABEL: &str = "vp-input";
pub const INPUT_RT_LABEL: &str = "vp-input-rt";

const MAX_INPUT_PER_SEC: u32 = 3000;

/// Session-level id offered for the playout-delay RTP header extension (the standard str0m map
/// leaves 12 free). str0m renumbers it to whatever id the iPad's offer uses.
const PLAYOUT_DELAY_EXT_ID: u8 = 12;

/// Room in the UDP send buffer for a whole frame burst. The Windows default (64 KB) can
/// overflow when a large frame leaves at once, and a datagram refused there is silently lost.
const UDP_SEND_BUFFER: usize = 1 << 20;

pub struct EncodedVideo {
    pub data: Vec<u8>,
    pub keyframe: bool,
    pub captured_at: Instant,
    pub rtp_time: u64,
}

pub enum PipelineEvent {
    Cursor {
        state: CursorState,
        width: u32,
        height: u32,
    },
    CursorShape(CursorShape),
    Applied(AppliedVideo),
    Displays {
        displays: Vec<DisplayInfo>,
        active: String,
    },
    Failure(String),
}

pub enum PipelineCommand {
    Keyframe,
    Bitrate(u32),
    Reconfigure(crate::config::VideoSettings),
    SelectDisplay(String),
    Stop,
}

#[derive(Default)]
pub struct PipelineStats {
    pub stats: AgentStats,
    pub min_kbps: u32,
    pub max_kbps: u32,
}

/// Start conservatively, then let TWCC probe toward the requested ceiling. A
/// 200 Mb/s quality ceiling is appropriate for a strong LAN, but using it as
/// the first estimate can fill an iPad Wi-Fi queue for several seconds before
/// feedback arrives.
pub fn initial_bwe_kbps(desired_kbps: u32) -> u32 {
    desired_kbps.clamp(4_000, 12_000)
}

#[cfg(test)]
mod tests {
    use super::{answer_has_playout_delay, initial_bwe_kbps, rtp_clock};
    use std::time::{Duration, Instant};

    #[test]
    fn maximum_quality_starts_with_a_safe_receiver_estimate() {
        assert_eq!(initial_bwe_kbps(200_000), 12_000);
        assert_eq!(initial_bwe_kbps(1_500), 4_000);
        assert_eq!(initial_bwe_kbps(8_000), 8_000);
    }

    #[test]
    fn rtp_clock_matches_the_capture_pipeline_timestamps() {
        let epoch = Instant::now();
        assert_eq!(rtp_clock(epoch, epoch), 0);
        assert_eq!(rtp_clock(epoch, epoch + Duration::from_secs(1)), 90_000);
        // Same formula as the pipeline: microseconds × 9 / 100, truncated to 32 bits.
        let later = epoch + Duration::from_micros(123_456_789);
        assert_eq!(rtp_clock(epoch, later), (123_456_789u64 * 9 / 100) as u32);
    }

    #[test]
    fn playout_delay_is_detected_in_the_answer_only_when_negotiated() {
        let with = "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=extmap:12 http://www.webrtc.org/experiments/rtp-hdrext/playout-delay\r\n";
        let without = "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=extmap:3 http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01\r\n";
        assert!(answer_has_playout_delay(with));
        assert!(!answer_has_playout_delay(without));
    }
}

pub struct Negotiated {
    pub rtc: Rtc,
    pub answer: String,
    pub sockets: Vec<UdpSocket>,
    pub relay: Option<RelayMediaLink>,
    /// The iPad accepted the playout-delay extension: its jitter buffer can be switched off.
    pub zero_playout_delay: bool,
}

/// Enlarges a UDP socket's send buffer (see [`UDP_SEND_BUFFER`]).
#[cfg(windows)]
fn enlarge_send_buffer(socket: &UdpSocket) {
    use std::os::windows::io::AsRawSocket;
    use windows::Win32::Networking::WinSock::{setsockopt, SOCKET, SOL_SOCKET, SO_SNDBUF};
    let size = (UDP_SEND_BUFFER as i32).to_ne_bytes();
    let result = unsafe {
        setsockopt(
            SOCKET(socket.as_raw_socket() as usize),
            SOL_SOCKET,
            SO_SNDBUF,
            Some(&size),
        )
    };
    if result != 0 {
        tracing::debug!("could not enlarge the UDP send buffer");
    }
}

#[cfg(not(windows))]
fn enlarge_send_buffer(_socket: &UdpSocket) {}

static CRYPTO: Once = Once::new();

pub fn negotiate(
    offer_sdp: &str,
    codec: CodecId,
    ips: &[Ipv4Addr],
    initial_kbps: u32,
    relay: Option<RelayMediaLink>,
) -> anyhow::Result<Negotiated> {
    CRYPTO.call_once(|| str0m::crypto::from_feature_flags().install_process_default());
    let initial_bwe = initial_bwe_kbps(initial_kbps);
    let mut rtc = Rtc::builder()
        .set_ice_lite(true)
        .clear_codecs()
        .enable_h264(codec == CodecId::H264)
        .enable_h265(codec == CodecId::Hevc)
        .enable_av1(codec == CodecId::Av1)
        .enable_bwe(Some(Bitrate::kbps(initial_bwe as u64)))
        // Lets every frame tell the iPad to show it as soon as it is decoded (see write_video).
        .set_extension(PLAYOUT_DELAY_EXT_ID, Extension::PlayoutDelay)
        .set_stats_interval(Some(Duration::from_secs(1)))
        .build(Instant::now());
    // Keep the requested setting as the long-term objective. str0m increases
    // toward it only when receiver feedback confirms that the iPad link can
    // sustain it.
    rtc.bwe()
        .set_desired_bitrate(Bitrate::kbps(initial_kbps as u64));
    let mut sockets = vec![];
    for ip in ips {
        let socket = UdpSocket::bind(SocketAddr::from((*ip, 0)))?;
        socket.set_nonblocking(true)?;
        enlarge_send_buffer(&socket);
        let addr = socket.local_addr()?;
        match Candidate::host(addr, "udp") {
            Ok(candidate) => {
                let _ = rtc.add_local_candidate(candidate);
                sockets.push(socket);
            }
            Err(error) => tracing::debug!(%error, %addr, "skipping candidate"),
        }
    }
    // ICE-lite only accepts host candidates: the relay's virtual address is announced as one.
    if let Some(link) = &relay {
        let candidate = Candidate::host(link.sender.local(), "udp")
            .map_err(|e| anyhow::anyhow!("relay candidate: {e}"))?;
        anyhow::ensure!(
            rtc.add_local_candidate(candidate).is_some(),
            "relay candidate was rejected"
        );
    }
    anyhow::ensure!(
        !sockets.is_empty() || relay.is_some(),
        "no usable local network address"
    );
    let offer =
        SdpOffer::from_sdp_string(offer_sdp).map_err(|e| anyhow::anyhow!("invalid offer: {e}"))?;
    let answer = rtc
        .sdp_api()
        .accept_offer(offer)
        .map_err(|e| anyhow::anyhow!("offer rejected: {e}"))?;
    let answer = answer.to_sdp_string();
    let zero_playout_delay = answer_has_playout_delay(&answer);
    tracing::info!(zero_playout_delay, "video answer ready");
    Ok(Negotiated {
        rtc,
        answer,
        sockets,
        relay,
        zero_playout_delay,
    })
}

/// True if the answer kept the playout-delay extension (only possible when the offer had it).
fn answer_has_playout_delay(answer_sdp: &str) -> bool {
    answer_sdp.lines().any(|line| {
        line.starts_with("a=extmap:")
            && line.contains("http://www.webrtc.org/experiments/rtp-hdrext/playout-delay")
    })
}

pub struct RtcContext {
    pub codec: CodecId,
    /// Encoded frames (at most two queued: stale video is dropped, never delayed).
    pub video: tokio::sync::mpsc::Receiver<EncodedVideo>,
    /// Cursor, display and encoder notifications.
    pub events: tokio::sync::mpsc::Receiver<PipelineEvent>,
    pub pipeline: crossbeam_channel::Sender<PipelineCommand>,
    pub injector: crossbeam_channel::Sender<InjectCommand>,
    pub control_allowed: Arc<AtomicBool>,
    pub info: Arc<RwLock<SessionInfo>>,
    pub on_change: Arc<dyn Fn() + Send + Sync>,
    pub stop: tokio::sync::watch::Receiver<bool>,
    pub stats: Arc<Mutex<PipelineStats>>,
    pub settings: Settings,
    /// Origin of the RTP timestamps written by the capture pipeline.
    pub epoch: Instant,
    /// Frames written but not yet sent, read by the capture pipeline.
    pub backlog: Arc<SendBacklog>,
}

/// Current value of the RTP video clock (90 kHz since `epoch`, wrapped like the RTP header).
/// The iPad uses it to relate a displayed frame's RTP timestamp to its own clock.
fn rtp_clock(epoch: Instant, now: Instant) -> u32 {
    (now.saturating_duration_since(epoch).as_micros() as u64 * 9 / 100) as u32
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChannelKind {
    Control,
    Input,
    InputRealtime,
}

struct State {
    ctx: RtcContext,
    video_mid: Option<Mid>,
    pt: Option<Pt>,
    channels: HashMap<ChannelId, ChannelKind>,
    control: Option<ChannelId>,
    realtime: Option<ChannelId>,
    connected: bool,
    ended: bool,
    rt_filter: SequenceFilter,
    input_window: (Instant, u32),
    input_total: u32,
    invalid_input: u32,
    last_keyframe_request: Instant,
    cursor_seq: u16,
    last_cursor: Option<(CursorState, Instant)>,
    pending_shapes: Vec<CursorShape>,
    app_watch: bool,
    last_app: Option<String>,
    rtt_ms: Option<f32>,
    sent_bytes: (u64, Instant, u32),
    pending_control: Vec<String>,
    /// Measures how long each frame waits before its last packet leaves.
    tracker: FrameTracker,
    zero_playout_delay: bool,
}

pub async fn run(negotiated: Negotiated, ctx: RtcContext) {
    let Negotiated {
        mut rtc,
        sockets,
        relay,
        zero_playout_delay,
        ..
    } = negotiated;
    let mut udp: Vec<Arc<tokio::net::UdpSocket>> = vec![];
    for socket in sockets {
        match tokio::net::UdpSocket::from_std(socket) {
            Ok(s) => udp.push(Arc::new(s)),
            Err(error) => tracing::warn!(%error, "udp socket"),
        }
    }
    let local_only = ctx.settings.network.mode == crate::config::NetworkMode::LocalOnly;
    let (net_tx, mut net_rx) =
        tokio::sync::mpsc::channel::<(Vec<u8>, SocketAddr, SocketAddr)>(1024);
    let mut readers = vec![];
    for socket in &udp {
        let socket = socket.clone();
        let net_tx = net_tx.clone();
        readers.push(tokio::spawn(async move {
            let local = match socket.local_addr() {
                Ok(addr) => addr,
                Err(_) => return,
            };
            let mut buf = vec![0u8; 2048];
            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((n, source)) => {
                        if local_only && !crate::net::is_local_address(source.ip()) {
                            continue;
                        }
                        if net_tx
                            .send((buf[..n].to_vec(), source, local))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => continue,
                    Err(_) => break,
                }
            }
        }));
    }
    let relay_sender: Option<RelaySender> = relay.as_ref().map(|link| link.sender.clone());
    if let Some(RelayMediaLink {
        mut incoming,
        sender,
    }) = relay
    {
        let net_tx = net_tx.clone();
        let local = sender.local();
        readers.push(tokio::spawn(async move {
            while let Some((source, data)) = incoming.recv().await {
                if net_tx.send((data, source, local)).await.is_err() {
                    break;
                }
            }
        }));
    }
    drop(net_tx);

    let app_watch = ctx.settings.input.detect_active_app;
    let tracker = FrameTracker::new(ctx.backlog.clone());
    let mut state = State {
        ctx,
        tracker,
        zero_playout_delay,
        video_mid: None,
        pt: None,
        channels: HashMap::new(),
        control: None,
        realtime: None,
        connected: false,
        ended: false,
        rt_filter: SequenceFilter::default(),
        input_window: (Instant::now(), 0),
        input_total: 0,
        invalid_input: 0,
        last_keyframe_request: Instant::now() - Duration::from_secs(5),
        cursor_seq: 0,
        last_cursor: None,
        pending_shapes: vec![],
        app_watch,
        last_app: None,
        rtt_ms: None,
        sent_bytes: (0, Instant::now(), 0),
        pending_control: vec![],
    };

    let mut ticker = tokio::time::interval(Duration::from_millis(500));
    let mut connect_deadline = Some(Instant::now() + Duration::from_secs(20));
    let mut stop = state.ctx.stop.clone();

    while let Some(timeout) = drain(&mut rtc, &udp, relay_sender.as_ref(), &mut state) {
        if state.ended || !rtc.is_alive() {
            break;
        }
        // Flush one queued control message per iteration (each write is one mutation).
        if !state.pending_control.is_empty() {
            let message = state.pending_control.remove(0);
            if let Some(id) = state.control {
                if let Some(mut channel) = rtc.channel(id) {
                    let _ = channel.write(false, message.as_bytes());
                }
            }
            continue;
        }
        let wait = timeout.saturating_duration_since(Instant::now());
        tokio::select! {
            _ = stop.changed() => break,
            packet = net_rx.recv() => {
                let Some((data, source, destination)) = packet else { break };
                if let Ok(contents) = data.as_slice().try_into() {
                    let input = Input::Receive(Instant::now(), Receive { proto: Protocol::Udp, source, destination, contents });
                    if rtc.handle_input(input).is_err() {
                        break;
                    }
                }
            }
            video = state.ctx.video.recv() => {
                let Some(video) = video else { break };
                write_video(&mut rtc, &mut state, video);
            }
            event = state.ctx.events.recv() => {
                let Some(event) = event else { break };
                handle_pipeline_event(&mut rtc, &mut state, event);
            }
            _ = tokio::time::sleep(wait) => {
                if rtc.handle_input(Input::Timeout(Instant::now())).is_err() {
                    break;
                }
            }
            _ = ticker.tick() => {
                if let Some(deadline) = connect_deadline {
                    if !state.connected && Instant::now() > deadline {
                        tracing::warn!("peer connection timed out");
                        break;
                    }
                    if state.connected {
                        connect_deadline = None;
                    }
                }
                periodic(&mut state);
            }
        }
    }

    for reader in readers {
        reader.abort();
    }
    let _ = state
        .ctx
        .injector
        .send(InjectCommand::Event(InputEvent::ReleaseAll));
    let _ = state.ctx.pipeline.send(PipelineCommand::Stop);
    rtc.disconnect();
    {
        let mut info = state.ctx.info.write();
        info.streaming = false;
        info.stats = None;
    }
    (state.ctx.on_change)();
}

/// Drains `poll_output` until a timeout. Returns `None` when the peer is gone.
fn drain(
    rtc: &mut Rtc,
    udp: &[Arc<tokio::net::UdpSocket>],
    relay: Option<&RelaySender>,
    state: &mut State,
) -> Option<Instant> {
    loop {
        match rtc.poll_output() {
            Ok(Output::Timeout(t)) => return Some(t),
            Ok(Output::Transmit(transmit)) => {
                if let Some(pt) = state.pt {
                    state
                        .tracker
                        .on_transmit(&transmit.contents, *pt, Instant::now());
                }
                if let Some(relay) = relay.filter(|r| r.local() == transmit.source) {
                    relay.send(transmit.destination, &transmit.contents);
                    state.sent_bytes.0 += transmit.contents.len() as u64;
                } else if let Some(socket) = udp
                    .iter()
                    .find(|s| s.local_addr().ok() == Some(transmit.source))
                {
                    let _ = socket.try_send_to(&transmit.contents, transmit.destination);
                    state.sent_bytes.0 += transmit.contents.len() as u64;
                }
            }
            Ok(Output::Event(event)) => handle_event(rtc, state, event),
            Err(error) => {
                tracing::warn!(%error, "rtc error");
                return None;
            }
        }
        if state.ended {
            return None;
        }
    }
}

fn handle_event(rtc: &mut Rtc, state: &mut State, event: Event) {
    match event {
        Event::Connected => {
            state.connected = true;
            let _ = state.ctx.pipeline.send(PipelineCommand::Keyframe);
            tracing::info!("peer connected");
        }
        Event::IceConnectionStateChange(IceConnectionState::Disconnected) => state.ended = true,
        Event::Closed => state.ended = true,
        Event::MediaAdded(added) if added.kind == MediaKind::Video => {
            state.video_mid = Some(added.mid);
            state.pt = select_pt(rtc, added.mid, state.ctx.codec);
            if state.pt.is_none() {
                tracing::error!("negotiated video has no payload type for the selected codec");
            }
        }
        Event::ChannelOpen(id, label) => {
            let kind = match label.as_str() {
                CONTROL_LABEL => ChannelKind::Control,
                INPUT_LABEL => ChannelKind::Input,
                INPUT_RT_LABEL => ChannelKind::InputRealtime,
                _ => return,
            };
            state.channels.insert(id, kind);
            match kind {
                ChannelKind::Control => {
                    state.control = Some(id);
                    let displays = {
                        let stats = state.ctx.stats.lock();
                        (stats.min_kbps, stats.max_kbps)
                    };
                    let _ = displays;
                    let control = state.ctx.control_allowed.load(Ordering::SeqCst);
                    state
                        .pending_control
                        .push(json!({ "t": "permission", "control": control }).to_string());
                    if let Some(video) = state.ctx.info.read().video.clone() {
                        state
                            .pending_control
                            .push(json!({ "t": "video.applied", "video": video }).to_string());
                    }
                    for shape in std::mem::take(&mut state.pending_shapes) {
                        queue_shape(state, &shape);
                    }
                    state.last_app = None;
                }
                ChannelKind::InputRealtime => state.realtime = Some(id),
                ChannelKind::Input => {}
            }
        }
        Event::ChannelData(data) => match state.channels.get(&data.id).copied() {
            Some(ChannelKind::Input) if data.binary => handle_input(state, &data.data, false),
            Some(ChannelKind::InputRealtime) if data.binary => {
                handle_input(state, &data.data, true)
            }
            Some(ChannelKind::Control) if !data.binary => handle_control(state, &data.data),
            _ => state.invalid_input += 1,
        },
        Event::KeyframeRequest(_) => {
            if state.last_keyframe_request.elapsed() > Duration::from_millis(250) {
                state.last_keyframe_request = Instant::now();
                let _ = state.ctx.pipeline.send(PipelineCommand::Keyframe);
            }
        }
        Event::EgressBitrateEstimate(BweKind::Twcc(bitrate)) => {
            let estimate_kbps = (bitrate.as_u64() / 1000) as u32;
            let (min, max) = {
                let mut stats = state.ctx.stats.lock();
                stats.stats.bwe_kbps = estimate_kbps;
                (stats.min_kbps, stats.max_kbps)
            };
            let target =
                ((estimate_kbps as f64 * 0.85) as u32).clamp(min.max(500), max.max(min.max(500)));
            let _ = state.ctx.pipeline.send(PipelineCommand::Bitrate(target));
        }
        Event::PeerStats(stats) => {
            state.rtt_ms = stats.rtt.map(|d| d.as_secs_f32() * 1000.0);
        }
        _ => {}
    }
}

fn select_pt(rtc: &mut Rtc, mid: Mid, codec: CodecId) -> Option<Pt> {
    let writer = rtc.writer(mid)?;
    let wanted = match codec {
        CodecId::H264 => Codec::H264,
        CodecId::Hevc => Codec::H265,
        CodecId::Av1 => Codec::Av1,
    };
    let mut best: Option<(i32, Pt)> = None;
    for params in writer.payload_params() {
        let spec = params.spec();
        if spec.codec != wanted {
            continue;
        }
        let mut score = 1;
        if wanted == Codec::H264 {
            if spec.format.packetization_mode == Some(1) {
                score += 10;
            }
            match spec.format.profile_level_id.map(|p| p >> 8) {
                Some(0x640c) | Some(0x6400) => score += 5,
                Some(0x42e0) => score += 3,
                _ => {}
            }
        }
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, params.pt()));
        }
    }
    best.map(|(_, pt)| pt)
}

fn handle_input(state: &mut State, data: &[u8], realtime: bool) {
    if !state.ctx.control_allowed.load(Ordering::Relaxed) {
        return;
    }
    let now = Instant::now();
    if now.duration_since(state.input_window.0) >= Duration::from_secs(1) {
        state.input_window = (now, 0);
    }
    state.input_window.1 += 1;
    if state.input_window.1 > MAX_INPUT_PER_SEC {
        return;
    }
    let Some((seq, event)) = input::decode(data) else {
        state.invalid_input += 1;
        if state.invalid_input > 500 {
            tracing::warn!("too many invalid input messages; ending session");
            state.ended = true;
        }
        return;
    };
    if realtime && !state.rt_filter.accept(seq) {
        return;
    }
    let policy = &state.ctx.settings.input;
    let allowed = match event.category() {
        InputCategory::Mouse => policy.allow_mouse,
        InputCategory::Keyboard => policy.allow_keyboard,
        InputCategory::Pen => policy.allow_pen,
        InputCategory::Touch => policy.allow_touch,
        InputCategory::System => true,
    };
    if !allowed {
        return;
    }
    state.input_total += 1;
    let is_move = matches!(
        event,
        InputEvent::MoveAbs { .. } | InputEvent::MoveRel { .. }
    );
    match state.ctx.injector.try_send(InjectCommand::Event(event)) {
        Ok(()) => {}
        Err(crossbeam_channel::TrySendError::Full(InjectCommand::Event(ev))) if !is_move => {
            // Never drop presses/releases: block briefly instead.
            let _ = state
                .ctx
                .injector
                .send_timeout(InjectCommand::Event(ev), Duration::from_millis(20));
        }
        Err(_) => {}
    }
}

fn handle_control(state: &mut State, data: &[u8]) {
    if data.len() > 16 * 1024 {
        state.invalid_input += 1;
        return;
    }
    let Ok(message) = serde_json::from_slice::<ControlToAgent>(data) else {
        state.invalid_input += 1;
        return;
    };
    match message {
        ControlToAgent::Ping { id, t0 } => {
            let rtp = rtp_clock(state.ctx.epoch, Instant::now());
            state.pending_control.push(
                json!({ "t": "pong", "id": id, "t0": t0, "agentMs": now_ms(), "rtp": rtp })
                    .to_string(),
            );
        }
        ControlToAgent::VideoSettings { video } => {
            if !state.ctx.settings.streaming.allow_client_overrides
                || crate::protocol::validate_video(&video).is_err()
            {
                return;
            }
            let codec_requested = match video.codec {
                crate::config::CodecSetting::Auto => None,
                crate::config::CodecSetting::H264 => Some(CodecId::H264),
                crate::config::CodecSetting::Hevc => Some(CodecId::Hevc),
                crate::config::CodecSetting::Av1 => Some(CodecId::Av1),
            };
            let current_profile = state.ctx.info.read().video.as_ref().map(|v| v.codec);
            let profile_changed = state.ctx.settings.streaming.video.profile != video.profile;
            if codec_requested.is_some_and(|c| Some(c) != current_profile)
                || (codec_requested.is_none() && profile_changed)
            {
                state.ctx.settings.streaming.video = video;
                state
                    .pending_control
                    .push(json!({ "t": "video.renegotiate", "reason": "codec" }).to_string());
            } else {
                state.ctx.settings.streaming.video = video;
                let _ = state.ctx.pipeline.send(PipelineCommand::Reconfigure(video));
            }
        }
        ControlToAgent::DisplaySelect { display_id } => {
            if display_id.len() <= 64 {
                let _ = state
                    .ctx
                    .pipeline
                    .send(PipelineCommand::SelectDisplay(display_id));
            }
        }
        ControlToAgent::Keyframe {} => {
            let _ = state.ctx.pipeline.send(PipelineCommand::Keyframe);
        }
        ControlToAgent::AppWatch { enabled } => {
            state.app_watch = enabled && state.ctx.settings.input.detect_active_app;
            state.last_app = None;
        }
    }
}

fn queue_shape(state: &mut State, shape: &CursorShape) {
    if state.control.is_none() {
        state.pending_shapes.retain(|s| s.id != shape.id);
        state.pending_shapes.push(shape.clone());
        state.pending_shapes.truncate(8);
        return;
    }
    if let Some(png) = encode_png(shape) {
        state.pending_control.push(
            json!({ "t": "cursor.shape", "id": shape.id, "width": shape.width, "height": shape.height, "hotX": shape.hot_x, "hotY": shape.hot_y, "png": b64u(&png) })
                .to_string(),
        );
    }
}

fn write_video(rtc: &mut Rtc, state: &mut State, video: EncodedVideo) {
    if !state.connected {
        return;
    }
    let (Some(mid), Some(pt)) = (state.video_mid, state.pt) else {
        return;
    };
    if let Some(writer) = rtc.writer(mid) {
        // Minimum and maximum playout delay of zero: the iPad shows each frame as soon as it is
        // decoded instead of holding it in an adaptive jitter buffer (50–200 ms in Safari, which
        // ignores `jitterBufferTarget`). Ignored when the extension was not negotiated.
        match writer
            .playout_delay(MediaTime::ZERO, MediaTime::ZERO)
            .write(
                pt,
                video.captured_at,
                MediaTime::from_90khz(video.rtp_time),
                video.data,
            ) {
            Ok(()) => {
                state.sent_bytes.2 += 1;
                // The RTP header carries the timestamp truncated to 32 bits.
                state
                    .tracker
                    .on_frame_written(video.rtp_time as u32, Instant::now());
            }
            Err(error) => tracing::debug!(%error, "video write failed"),
        }
    }
}

fn handle_pipeline_event(rtc: &mut Rtc, state: &mut State, event: PipelineEvent) {
    match event {
        PipelineEvent::Cursor {
            state: cursor,
            width,
            height,
        } => {
            let Some(id) = state.realtime else { return };
            let now = Instant::now();
            if let Some((last, at)) = state.last_cursor {
                if last == cursor || now.duration_since(at) < Duration::from_millis(7) {
                    return;
                }
            }
            state.last_cursor = Some((cursor, now));
            state.cursor_seq = state.cursor_seq.wrapping_add(1);
            let x = cursor.x as f32 / width.max(1) as f32;
            let y = cursor.y as f32 / height.max(1) as f32;
            let payload =
                input::encode_cursor(x, y, cursor.shape_id, cursor.visible, state.cursor_seq);
            if let Some(mut channel) = rtc.channel(id) {
                let _ = channel.write(true, &payload);
            }
        }
        PipelineEvent::CursorShape(shape) => queue_shape(state, &shape),
        PipelineEvent::Applied(video) => {
            state.ctx.info.write().video = Some(video.clone());
            state
                .pending_control
                .push(json!({ "t": "video.applied", "video": video }).to_string());
            (state.ctx.on_change)();
        }
        PipelineEvent::Displays { displays, active } => {
            state.pending_control.push(
                json!({ "t": "displays", "displays": displays, "activeId": active }).to_string(),
            );
        }
        PipelineEvent::Failure(message) => {
            tracing::error!(%message, "pipeline failure");
            state
                .pending_control
                .push(json!({ "t": "video.renegotiate", "reason": message }).to_string());
        }
    }
}

fn periodic(state: &mut State) {
    let elapsed = state.sent_bytes.1.elapsed();
    if elapsed >= Duration::from_secs(1) {
        let kbps = (state.sent_bytes.0 as f64 * 8.0 / 1000.0 / elapsed.as_secs_f64()) as u32;
        let sent_fps = state.sent_bytes.2 as f32 / elapsed.as_secs_f32();
        let input_rate = state.input_total as f32 / elapsed.as_secs_f32();
        state.sent_bytes = (0, Instant::now(), 0);
        state.input_total = 0;
        let queue = state.tracker.take_delay();
        let stats = {
            let mut shared = state.ctx.stats.lock();
            shared.stats.sent_kbps = kbps;
            shared.stats.sent_fps = sent_fps;
            shared.stats.input_events_per_sec = input_rate;
            shared.stats.send_queue_ms = queue.average_ms;
            shared.stats.send_queue_max_ms = queue.max_ms;
            shared.stats.zero_playout_delay = state.zero_playout_delay;
            shared.stats.clone()
        };
        {
            let mut info = state.ctx.info.write();
            info.stats = Some(stats.clone());
            info.rtt_ms = state.rtt_ms;
        }
        if state.control.is_some() {
            state
                .pending_control
                .push(json!({ "t": "stats", "stats": stats, "rttMs": state.rtt_ms }).to_string());
        }
        (state.ctx.on_change)();
    }
    if state.app_watch && state.control.is_some() {
        let exe = crate::platform::system::foreground_executable();
        if exe != state.last_app {
            if let Some(name) = &exe {
                state.pending_control.push(json!({ "t": "app.active", "exe": name, "profile": crate::platform::system::profile_for(name) }).to_string());
                state.ctx.info.write().active_app = Some(name.clone());
            }
            state.last_app = exe;
        }
    }
}

/// Input policy derived from settings.
pub fn input_policy(settings: &Settings) -> InputPolicy {
    InputPolicy {
        keyboard: settings.input.allow_keyboard,
        mouse: settings.input.allow_mouse,
        pen: settings.input.allow_pen,
        touch: settings.input.allow_touch,
        pen_as_mouse: settings.input.pen_mode == crate::config::PenMode::Mouse,
    }
}
