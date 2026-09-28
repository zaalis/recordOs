//! Capture → convert → encode loop on a dedicated thread.
//!
//! Frames are encoded as soon as they are captured, at most once per frame interval of the
//! target frame rate. When the desktop is idle a refresh frame is sent twice per second.
//! While earlier frames are still waiting to leave the PC, encoding is held back and the newest
//! desktop image is encoded once the path is free (latest wins), so video never queues up.
//! Device loss (resolution change, secure desktop, GPU reset) rebuilds the pipeline automatically.

use super::capture::{Acquire, CursorState, DesktopDuplicator};
use super::convert::Nv12Converter;
use super::gpu::{self, Gpu, Output};
use super::inject::SharedRect;
use super::mf_encoder::{self, EncodedFrame, MfEncoder};
use super::sw_encoder::SoftwareEncoder;
use crate::config::{CodecId, VideoSettings};
use crate::media::backlog::SendBacklog;
use crate::media::codec;
use crate::media::rtc::{EncodedVideo, PipelineCommand, PipelineEvent, PipelineStats};
use crate::protocol::{AppliedVideo, ClientDisplay, DisplayInfo};
use crossbeam_channel::Receiver;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

const IDLE_REFRESH: Duration = Duration::from_millis(500);
/// How often a hardware encoder is polled while one of its frames is still in flight. Relies on
/// the 1 ms system timer held by [`super::system::RealtimeScheduling`].
const OUTPUT_POLL: Duration = Duration::from_millis(1);
/// How often the send path is checked again while a frame is held back.
const HOLD_POLL: Duration = Duration::from_millis(1);
/// Congestion that lasts longer than this still lets one frame through, so the picture and the
/// bandwidth estimate keep moving (at least four frames per second).
const MAX_HOLD: Duration = Duration::from_millis(250);
/// Lowest bitrate the bandwidth estimate may bring the encoder down to. A higher floor made the
/// encoder outrun a weak link permanently, and the excess piled up in the send queue.
const MIN_ADAPTIVE_KBPS: u32 = 1_000;

pub struct PipelineConfig {
    pub codec: CodecId,
    pub hardware: bool,
    pub fallback_reason: Option<String>,
    pub video: VideoSettings,
    pub client: ClientDisplay,
    pub display_id: Option<String>,
    pub prevent_sleep: bool,
    /// Conservative bitrate used until the receiver's TWCC estimate is known.
    pub initial_bitrate_kbps: u32,
    /// Origin of RTP timestamps, shared with the network task (clock sync with the iPad).
    pub epoch: Instant,
    /// Frames written to the peer connection that have not left the PC yet.
    pub backlog: Arc<SendBacklog>,
}

enum Encoder {
    Hardware(MfEncoder),
    Software(Box<SoftwareEncoder>, Vec<u8>, Vec<u8>),
}

impl Encoder {
    fn name(&self) -> String {
        match self {
            Encoder::Hardware(e) => e.name.clone(),
            Encoder::Software(..) => "OpenH264 (software)".into(),
        }
    }
    fn set_bitrate(&mut self, kbps: u32) {
        match self {
            Encoder::Hardware(e) => e.set_bitrate(kbps),
            Encoder::Software(e, ..) => e.set_bitrate(kbps),
        }
    }
    fn bitrate(&self) -> u32 {
        match self {
            Encoder::Hardware(e) => e.bitrate_kbps(),
            Encoder::Software(e, ..) => e.bitrate_kbps(),
        }
    }
}

pub fn display_infos(outputs: &[Output]) -> Vec<DisplayInfo> {
    outputs
        .iter()
        .map(|o| DisplayInfo {
            id: o.id.clone(),
            name: o.name.clone(),
            width: o.width(),
            height: o.height(),
            x: o.rect.left,
            y: o.rect.top,
            refresh_hz: o.refresh_hz,
            primary: o.primary,
        })
        .collect()
}

/// True while encoding another frame would only queue it behind earlier ones: the hand-off to
/// the network task is full, two frames are already waiting, or the one waiting has not left
/// within a frame interval.
fn send_path_busy(
    queued_frames: u32,
    oldest_age: Option<Duration>,
    channel_full: bool,
    interval: Duration,
) -> bool {
    channel_full
        || queued_frames >= 2
        || (queued_frames >= 1 && oldest_age.is_some_and(|age| age >= interval))
}

/// Capture times handed to the encoder: when the desktop image was acquired, kept strictly
/// increasing. Two encoded frames sharing an RTP timestamp would be merged into one corrupt
/// frame by the receiver.
#[derive(Default)]
struct CaptureClock {
    last: Option<Instant>,
}

impl CaptureClock {
    fn stamp(&mut self, candidate: Instant) -> Instant {
        let at = match self.last {
            Some(last) if candidate <= last => last + Duration::from_micros(100),
            _ => candidate,
        };
        self.last = Some(at);
        at
    }
}

/// Hands encoded frames to the network task with their capture time.
struct FrameSink {
    video: tokio::sync::mpsc::Sender<EncodedVideo>,
    epoch: Instant,
}

impl FrameSink {
    /// Frames handed over that the network task has not picked up yet, and whether no slot is
    /// left.
    fn pending(&self) -> (u32, bool) {
        let free = self.video.capacity();
        ((self.video.max_capacity() - free) as u32, free == 0)
    }

    /// Queues one access unit. Returns false when the network side is congested (the frame is
    /// dropped and the next one must be a keyframe).
    fn send(&self, data: Vec<u8>, keyframe: bool, captured_at: Instant) -> bool {
        let rtp_time = (captured_at.duration_since(self.epoch).as_micros() as u64) * 9 / 100;
        self.video
            .try_send(EncodedVideo {
                data,
                keyframe,
                captured_at,
                rtp_time,
            })
            .is_ok()
    }

    /// Capture time of a hardware frame, recovered from its sample time.
    fn captured_at(&self, frame: &EncodedFrame) -> Instant {
        self.epoch + Duration::from_nanos(frame.timestamp_100ns.max(0) as u64 * 100)
    }
}

pub fn spawn(
    config: PipelineConfig,
    video: tokio::sync::mpsc::Sender<EncodedVideo>,
    events: tokio::sync::mpsc::Sender<PipelineEvent>,
    commands: Receiver<PipelineCommand>,
    display_rect: SharedRect,
    stats: Arc<Mutex<PipelineStats>>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("vp-capture".into())
        .spawn(move || {
            super::system::prioritize_current_thread();
            let _realtime = super::system::RealtimeScheduling::acquire();
            mf_encoder::com_init_thread();
            mf_encoder::ensure_media_foundation();
            let _wake = config.prevent_sleep.then(super::system::WakeLock::acquire);
            run(config, video, events, commands, display_rect, stats);
        })
        .expect("capture thread")
}

fn run(
    mut config: PipelineConfig,
    video: tokio::sync::mpsc::Sender<EncodedVideo>,
    events: tokio::sync::mpsc::Sender<PipelineEvent>,
    commands: Receiver<PipelineCommand>,
    display_rect: SharedRect,
    stats: Arc<Mutex<PipelineStats>>,
) {
    let epoch = config.epoch;
    let backlog = config.backlog.clone();
    let sink = FrameSink { video, epoch };
    let mut failures = 0u32;
    let mut last_bitrate_request: Option<u32> = None;
    let mut clock = CaptureClock::default();

    'rebuild: loop {
        if failures > 0 {
            // Back off while the desktop is unavailable (UAC prompt, lock screen, mode switch).
            let delay = Duration::from_millis((100 * failures as u64).min(1000));
            match commands.recv_timeout(delay) {
                Ok(PipelineCommand::Stop)
                | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
                Ok(PipelineCommand::SelectDisplay(id)) => config.display_id = Some(id),
                Ok(PipelineCommand::Reconfigure(video)) => config.video = video,
                _ => {}
            }
        }

        let outputs = gpu::outputs();
        let Some(output) = config
            .display_id
            .as_ref()
            .and_then(|id| outputs.iter().find(|o| &o.id == id))
            .or_else(|| outputs.iter().find(|o| o.primary))
            .or(outputs.first())
            .cloned()
        else {
            failures += 1;
            continue;
        };
        let _ = events.try_send(PipelineEvent::Displays {
            displays: display_infos(&outputs),
            active: output.id.clone(),
        });
        *display_rect.write() = output.rect;

        let built = (|| -> anyhow::Result<(Gpu, DesktopDuplicator, Nv12Converter, Encoder, AppliedVideo)> {
            let gpu = Gpu::for_adapter(&output.adapter)?;
            let duplicator = DesktopDuplicator::new(&gpu, &output)?;
            let native = (duplicator.width, duplicator.height);
            let (width, height) = codec::resolve_size(&config.video, native, &config.client);
            let fps = codec::resolve_fps(&config.video, output.refresh_hz, config.client.refresh_hz);
            let target = codec::resolve_bitrate(&config.video, (width, height), fps, config.codec);
            let spec = codec::profile_spec(config.video.profile);
            {
                let mut s = stats.lock();
                // The BWE floor protects from unusable video, not from a
                // quality preference. A 200 Mb/s ceiling must still be able
                // to adapt to a 10–15 Mb/s iPad Wi-Fi link, and a weak link
                // below that must never be outrun by the encoder.
                s.min_kbps = MIN_ADAPTIVE_KBPS.min(target);
                s.max_kbps = match config.video.bitrate {
                    crate::config::BitrateMode::Auto => spec.max_kbps.max(target),
                    _ => target,
                };
            }
            let converter = Nv12Converter::new(&gpu, native.0, native.1, width, height, fps)?;
            let quality = match config.video.profile {
                crate::config::LatencyProfile::LowLatency => 0,
                crate::config::LatencyProfile::BatterySaver => 20,
                crate::config::LatencyProfile::Balanced => 50,
                crate::config::LatencyProfile::MaxQuality => 90,
            };
            let mut fallback = config.fallback_reason.clone();
            let initial_target = config.initial_bitrate_kbps.min(target);
            let encoder = if config.hardware {
                match MfEncoder::new(&gpu, config.codec, width, height, fps, target, quality) {
                    Ok(encoder) => Encoder::Hardware(encoder),
                    Err(error) if config.codec == CodecId::H264 => {
                        tracing::warn!(%error, "hardware encoder unavailable, using software");
                        fallback = Some("Hardware encoding is unavailable; using software encoding.".into());
                        Encoder::Software(Box::new(SoftwareEncoder::new(width, height, fps, initial_target.min(20_000))?), vec![], vec![])
                    }
                    Err(error) => return Err(error),
                }
            } else {
                Encoder::Software(Box::new(SoftwareEncoder::new(width, height, fps, initial_target.min(20_000))?), vec![], vec![])
            };
            let mut encoder = encoder;
            // Prevent the initial keyframe from being generated at the maximum
            // quality ceiling before the iPad reports its real available rate.
            encoder.set_bitrate(initial_target);
            let applied = AppliedVideo {
                codec: config.codec,
                encoder: encoder.name(),
                hardware: matches!(encoder, Encoder::Hardware(_)),
                width,
                height,
                fps,
                target_kbps: encoder.bitrate(),
                fallback_reason: fallback,
            };
            Ok((gpu, duplicator, converter, encoder, applied))
        })();

        let (gpu, mut duplicator, mut converter, mut encoder, applied) = match built {
            Ok(parts) => parts,
            Err(error) => {
                failures += 1;
                if failures == 1 || failures.is_multiple_of(20) {
                    tracing::warn!(%error, "capture pipeline unavailable");
                }
                if failures == 30 {
                    let _ =
                        events.try_send(PipelineEvent::Failure(format!("capture failed: {error}")));
                }
                continue;
            }
        };
        failures = 0;
        tracing::info!(encoder = %applied.encoder, width = applied.width, height = applied.height, fps = applied.fps, kbps = applied.target_kbps, "pipeline ready");
        let interval = Duration::from_secs_f64(1.0 / applied.fps.max(1) as f64);
        let _ = events.try_send(PipelineEvent::Applied(applied.clone()));
        if let Some(kbps) = last_bitrate_request {
            encoder.set_bitrate(kbps);
        }

        let mut last_encode = Instant::now() - IDLE_REFRESH;
        let mut force_keyframe = true;
        let mut dirty = false;
        // One duplicate submission per real frame pushes it out of encoders that only release
        // frame N when frame N+1 arrives.
        let mut flushed = false;
        let mut last_cursor = CursorState::default();
        let mut window = (Instant::now(), 0u32, 0u32, 0f32, 0f32);
        // When the desktop image waiting to be encoded was acquired.
        let mut acquired_at = Instant::now();
        // Since when a due frame has been held back because the send path is busy.
        let mut holding_since: Option<Instant> = None;
        let mut held = Duration::ZERO;

        loop {
            while let Ok(command) = commands.try_recv() {
                match command {
                    PipelineCommand::Stop => return,
                    PipelineCommand::Keyframe => force_keyframe = true,
                    PipelineCommand::Bitrate(kbps) => {
                        last_bitrate_request = Some(kbps);
                        encoder.set_bitrate(kbps);
                    }
                    PipelineCommand::Reconfigure(video) => {
                        config.video = video;
                        continue 'rebuild;
                    }
                    PipelineCommand::SelectDisplay(id) => {
                        if outputs.iter().any(|o| o.id == id) && id != output.id {
                            config.display_id = Some(id);
                            continue 'rebuild;
                        }
                    }
                }
            }
            if commands.is_empty() && sink.video.is_closed() {
                return;
            }

            // Collect hardware output as soon as it is ready instead of at the next capture.
            let mut awaiting = false;
            if let Encoder::Hardware(hw) = &mut encoder {
                if hw.awaiting_output() {
                    match hw.poll() {
                        Ok(frames) => {
                            for frame in frames {
                                let captured_at = sink.captured_at(&frame);
                                if force_keyframe && frame.keyframe {
                                    force_keyframe = false;
                                }
                                window.2 += 1;
                                if !sink.send(frame.data, frame.keyframe, captured_at) {
                                    force_keyframe = true;
                                }
                            }
                        }
                        Err(error) => {
                            tracing::warn!(%error, "encoder output failed; rebuilding pipeline");
                            failures = 1;
                            continue 'rebuild;
                        }
                    }
                    awaiting = hw.awaiting_output();
                }
            }
            if !awaiting {
                flushed = false;
            }

            let since = last_encode.elapsed();
            let mut timeout = if dirty {
                interval.saturating_sub(since)
            } else {
                IDLE_REFRESH
                    .saturating_sub(since)
                    .min(Duration::from_millis(50))
            };
            if awaiting {
                timeout = timeout.min(OUTPUT_POLL);
            }
            if holding_since.is_some() {
                // A frame is due but held back: look at the send path again shortly, or take a
                // newer desktop image as soon as one is composed.
                timeout = HOLD_POLL;
            }
            let capture_start = Instant::now();
            // Round up: a zero-millisecond wait for a sub-millisecond remainder only spins.
            let timeout_ms = timeout.as_micros().div_ceil(1000).min(100) as u32;
            match duplicator.acquire(timeout_ms) {
                Ok(Acquire::Frame) => {
                    dirty = true;
                    acquired_at = Instant::now();
                    window.1 += 1;
                }
                Ok(Acquire::NoChange) => {}
                Ok(Acquire::AccessLost) => {
                    failures = 1;
                    continue 'rebuild;
                }
                Err(error) => {
                    tracing::debug!(%error, "acquire failed");
                    failures = 1;
                    continue 'rebuild;
                }
            }
            let capture_ms = capture_start.elapsed().as_secs_f32() * 1000.0;

            if duplicator.cursor != last_cursor {
                last_cursor = duplicator.cursor;
                let _ = events.try_send(PipelineEvent::Cursor {
                    state: last_cursor,
                    width: duplicator.width,
                    height: duplicator.height,
                });
            }
            if let Some(shape) = duplicator.new_shape.take() {
                let _ = events.try_send(PipelineEvent::CursorShape(shape));
            }

            let since = last_encode.elapsed();
            let flush = awaiting && !flushed && !dirty && since >= interval;
            let due = duplicator.has_frame
                && ((dirty && since + Duration::from_millis(1) >= interval)
                    || since >= IDLE_REFRESH
                    || (force_keyframe && since >= interval)
                    || flush);
            if !due {
                if let Some(started) = holding_since.take() {
                    held += started.elapsed();
                }
                continue;
            }

            // Latest wins: while earlier frames are still waiting to leave the PC, encoding
            // another one would only queue it behind them. Wait, then encode the newest image.
            let now = Instant::now();
            let (in_channel, channel_full) = sink.pending();
            if send_path_busy(
                backlog.queued_frames().saturating_add(in_channel),
                backlog.oldest_age(now),
                channel_full,
                interval,
            ) {
                let started = *holding_since.get_or_insert(now);
                if channel_full || now.duration_since(started) < MAX_HOLD {
                    continue;
                }
            }
            if let Some(started) = holding_since.take() {
                held += now.duration_since(started);
            }

            let encode_start = Instant::now();
            // A new desktop image carries the time it was captured; a refresh of an unchanged
            // image is stamped now.
            let captured_at = clock.stamp(if dirty { acquired_at } else { encode_start });
            let result: anyhow::Result<Vec<(Vec<u8>, bool, Instant)>> = (|| {
                let nv12 = converter.convert(&duplicator.frame)?;
                match &mut encoder {
                    Encoder::Hardware(hw) => {
                        let timestamp = (captured_at.duration_since(epoch).as_nanos() / 100) as i64;
                        Ok(hw
                            .encode(&nv12, timestamp, force_keyframe)?
                            .into_iter()
                            .map(|f| {
                                let at = sink.captured_at(&f);
                                (f.data, f.keyframe, at)
                            })
                            .collect())
                    }
                    Encoder::Software(sw, y, uv) => {
                        converter.read_nv12(&gpu, &nv12, y, uv)?;
                        let (data, keyframe) = sw.encode_nv12(y, uv, force_keyframe)?;
                        Ok(vec![(data, keyframe, captured_at)])
                    }
                }
            })();
            let encode_ms = encode_start.elapsed().as_secs_f32() * 1000.0;
            last_encode = Instant::now();
            // A busy asynchronous encoder refused the frame: submit it again next interval.
            dirty = matches!(&encoder, Encoder::Hardware(hw) if hw.input_dropped());
            flushed = flush;

            match result {
                Ok(frames) => {
                    window.3 += capture_ms;
                    window.4 += encode_ms;
                    for (data, keyframe, at) in frames {
                        if data.is_empty() {
                            continue;
                        }
                        if force_keyframe && keyframe {
                            force_keyframe = false;
                        }
                        window.2 += 1;
                        if !sink.send(data, keyframe, at) {
                            // The network side is congested: the next frame must be a keyframe.
                            force_keyframe = true;
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "encode failed; rebuilding pipeline");
                    failures = 1;
                    continue 'rebuild;
                }
            }

            if window.0.elapsed() >= Duration::from_secs(1) {
                let secs = window.0.elapsed().as_secs_f32();
                let mut s = stats.lock();
                s.stats.held_ms = held.as_secs_f32() * 1000.0 / secs;
                held = Duration::ZERO;
                s.stats.capture_fps = window.1 as f32 / secs;
                s.stats.encode_fps = window.2 as f32 / secs;
                s.stats.capture_ms = if window.2 > 0 {
                    window.3 / window.2 as f32
                } else {
                    0.0
                };
                s.stats.encode_ms = if window.2 > 0 {
                    window.4 / window.2 as f32
                } else {
                    0.0
                };
                s.stats.target_kbps = encoder.bitrate();
                s.stats.encoder = encoder.name();
                s.stats.encoder_state = "running".into();
                s.stats.codec = config.codec.as_str().into();
                s.stats.width = converter.out_width;
                s.stats.height = converter.out_height;
                window = (Instant::now(), 0, 0, 0.0, 0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_micros(16_667);

    #[test]
    fn an_idle_send_path_accepts_a_new_frame() {
        assert!(!send_path_busy(0, None, false, FRAME));
        // A frame that is leaving within its interval does not hold the next one.
        assert!(!send_path_busy(
            1,
            Some(Duration::from_millis(5)),
            false,
            FRAME
        ));
    }

    #[test]
    fn a_backed_up_send_path_holds_the_next_frame() {
        assert!(send_path_busy(1, Some(FRAME), false, FRAME));
        assert!(send_path_busy(
            2,
            Some(Duration::from_millis(1)),
            false,
            FRAME
        ));
        assert!(send_path_busy(0, None, true, FRAME));
    }

    #[test]
    fn capture_times_are_strictly_increasing() {
        let mut clock = CaptureClock::default();
        let t0 = Instant::now();
        assert_eq!(clock.stamp(t0), t0);
        // The same image re-encoded (encoder refused it, or a refresh) gets a later time.
        let again = clock.stamp(t0);
        assert!(again > t0);
        // An earlier candidate never goes back in time.
        assert!(clock.stamp(t0 - Duration::from_millis(5)) > again);
        let later = t0 + Duration::from_millis(16);
        assert_eq!(clock.stamp(later), later);
    }
}
