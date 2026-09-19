//! Windows media engine: probes encoders and displays, plans the stream and wires capture,
//! encoding, WebRTC and input injection together for each session.

use super::gpu;
use super::inject::InjectorHandle;
use super::pipeline::{self, PipelineConfig};
use crate::config::NetworkMode;
use crate::media::codec::{self, EncoderCapability};
use crate::media::rtc::{self, PipelineCommand, PipelineStats, RtcContext};
use crate::media::{MediaContext, MediaEngine, MediaHandle, MediaRequest, MediaStarted};
use crate::protocol::{AgentCaps, AppliedVideo, EncoderInfo};
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::RECT;

pub struct WindowsMediaEngine {
    encoders: Mutex<Option<(Instant, Vec<EncoderCapability>)>>,
}

impl WindowsMediaEngine {
    pub fn new() -> WindowsMediaEngine {
        super::system::enable_dpi_awareness();
        WindowsMediaEngine {
            encoders: Mutex::new(None),
        }
    }

    fn encoders(&self) -> Vec<EncoderCapability> {
        let mut cache = self.encoders.lock();
        if let Some((at, list)) = cache.as_ref() {
            if at.elapsed() < Duration::from_secs(120) {
                return list.clone();
            }
        }
        let primary_luid = gpu::outputs()
            .first()
            .and_then(|o| unsafe { o.adapter.GetDesc1().ok() })
            .map(|d| (d.AdapterLuid.LowPart, d.AdapterLuid.HighPart));
        let mut list = super::mf_encoder::probe(primary_luid);
        list.push(super::sw_encoder::capability());
        *cache = Some((Instant::now(), list.clone()));
        list
    }
}

impl Default for WindowsMediaEngine {
    fn default() -> Self {
        Self::new()
    }
}

struct WindowsMediaHandle {
    stop: tokio::sync::watch::Sender<bool>,
    pipeline: crossbeam_channel::Sender<PipelineCommand>,
    capture_thread: Option<std::thread::JoinHandle<()>>,
    injector: Option<InjectorHandle>,
}

impl MediaHandle for WindowsMediaHandle {
    fn stop(&mut self) {
        let _ = self.stop.send(true);
        let _ = self.pipeline.send(PipelineCommand::Stop);
        if let Some(thread) = self.capture_thread.take() {
            let _ = thread.join();
        }
        // Dropping the injector releases every held key, button, pen and touch contact.
        self.injector.take();
    }
}

impl Drop for WindowsMediaHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

impl MediaEngine for WindowsMediaEngine {
    fn capabilities(&self) -> AgentCaps {
        let outputs = gpu::outputs();
        let displays = pipeline::display_infos(&outputs);
        let max_fps = outputs.iter().map(|o| o.refresh_hz).max().unwrap_or(60);
        AgentCaps {
            encoders: self
                .encoders()
                .into_iter()
                .map(|e| EncoderInfo {
                    codec: e.codec,
                    hardware: e.hardware,
                    name: e.name,
                })
                .collect(),
            active_display_id: displays
                .iter()
                .find(|d| d.primary)
                .or(displays.first())
                .map(|d| d.id.clone())
                .unwrap_or_default(),
            displays,
            pen: true,
            touch: true,
            max_fps,
        }
    }

    fn start(&self, request: MediaRequest, context: MediaContext) -> anyhow::Result<MediaStarted> {
        let settings = context.settings.clone();
        let offer = request.offer;
        let relay = request.relay;
        let video = if settings.streaming.allow_client_overrides {
            offer.video
        } else {
            settings.streaming.video
        };
        let offered = codec::offered_codecs(&offer.sdp);
        let encoders = self.encoders();
        let choice = codec::choose_codec(
            video.codec,
            video.profile,
            &offered,
            &offer.codecs,
            &encoders,
            settings.streaming.encoder,
        )
        .ok_or_else(|| {
            anyhow::anyhow!("no video codec is supported by both this iPad and this PC")
        })?;

        let outputs = gpu::outputs();
        let output = settings
            .display
            .display_id
            .as_ref()
            .and_then(|id| outputs.iter().find(|o| &o.id == id))
            .or_else(|| outputs.iter().find(|o| o.primary))
            .or(outputs.first())
            .ok_or_else(|| anyhow::anyhow!("no display is available for capture"))?;
        let size = codec::resolve_size(&video, (output.width(), output.height()), &offer.display);
        let fps = codec::resolve_fps(&video, output.refresh_hz, offer.display.refresh_hz);
        let kbps = codec::resolve_bitrate(&video, size, fps, choice.codec);

        // A relayed iPad only has TURN candidates: LAN host candidates would never be reachable.
        // A cable session offers the cable address alone, so video and input never leave it; a
        // local-network session never offers a cable address.
        let mut ips: Vec<std::net::Ipv4Addr> = match (&relay, request.bind) {
            (Some(_), _) => vec![],
            (None, Some(cable)) => vec![cable],
            (None, None) => crate::net::lan_interfaces()
                .into_iter()
                .map(|i| i.ip)
                .filter(|ip| !request.exclude.contains(ip))
                .collect(),
        };
        if relay.is_none()
            && request.bind.is_none()
            && (settings.advanced.developer_loopback || ips.is_empty())
        {
            ips.push(std::net::Ipv4Addr::LOCALHOST);
        }
        if settings.network.mode == NetworkMode::LocalOnly {
            ips.retain(|ip| crate::net::is_local_address((*ip).into()));
        }
        let negotiated = rtc::negotiate(&offer.sdp, choice.codec, &ips, kbps, relay)?;
        let answer_sdp = negotiated.answer.clone();

        let display_rect: Arc<RwLock<RECT>> = Arc::new(RwLock::new(output.rect));
        let injector = InjectorHandle::spawn(display_rect.clone(), rtc::input_policy(&settings));
        // Keep at most two encoded frames waiting for the network task. Under
        // congestion, newer video replaces stale video instead of increasing
        // what the user perceives as mouse-to-image latency. Cursor and control
        // events have their own queue: sharing the two video slots with cursor
        // updates dropped frames (and forced keyframes) whenever the mouse moved.
        let (video_tx, video_rx) = tokio::sync::mpsc::channel(2);
        let (events_tx, events_rx) = tokio::sync::mpsc::channel(64);
        let (commands_tx, commands_rx) = crossbeam_channel::unbounded();
        let stats = Arc::new(Mutex::new(PipelineStats::default()));
        let initial_bitrate_kbps = rtc::initial_bwe_kbps(kbps);
        let capture_thread = pipeline::spawn(
            PipelineConfig {
                codec: choice.codec,
                hardware: choice.encoder.hardware,
                fallback_reason: choice.fallback_reason.clone(),
                video,
                client: offer.display,
                display_id: Some(output.id.clone()),
                prevent_sleep: settings.display.prevent_sleep,
                initial_bitrate_kbps,
            },
            video_tx,
            events_tx,
            commands_rx,
            display_rect,
            stats.clone(),
        );

        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let rtc_context = RtcContext {
            codec: choice.codec,
            video: video_rx,
            events: events_rx,
            pipeline: commands_tx.clone(),
            injector: injector.sender.clone(),
            control_allowed: context.control_allowed.clone(),
            info: context.info.clone(),
            on_change: context.on_change.clone(),
            stop: stop_rx,
            stats,
            settings,
        };
        tokio::runtime::Handle::current().spawn(rtc::run(negotiated, rtc_context));

        let applied = AppliedVideo {
            codec: choice.codec,
            encoder: choice.encoder.name.clone(),
            hardware: choice.encoder.hardware,
            width: size.0,
            height: size.1,
            fps,
            target_kbps: kbps,
            fallback_reason: choice.fallback_reason,
        };
        tracing::info!(device = %request.device_name, codec = choice.codec.as_str(), encoder = %applied.encoder, "streaming session starting");
        Ok(MediaStarted {
            answer_sdp,
            applied,
            handle: Box::new(WindowsMediaHandle {
                stop: stop_tx,
                pipeline: commands_tx,
                capture_thread: Some(capture_thread),
                injector: Some(injector),
            }),
        })
    }
}
