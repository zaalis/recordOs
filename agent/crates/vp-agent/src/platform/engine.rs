//! macOS media engine: probes encoders and displays, plans the stream and wires capture,
//! encoding, WebRTC and input injection together for each session.

use super::display;
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

pub struct MacMediaEngine {
    encoders: Mutex<Option<(Instant, Vec<EncoderCapability>)>>,
}

impl MacMediaEngine {
    pub fn new() -> MacMediaEngine {
        MacMediaEngine {
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
        let mut list = super::vt_encoder::probe();
        list.push(super::sw_encoder::capability());
        *cache = Some((Instant::now(), list.clone()));
        list
    }
}

impl Default for MacMediaEngine {
    fn default() -> Self {
        Self::new()
    }
}

struct MacMediaHandle {
    stop: tokio::sync::watch::Sender<bool>,
    pipeline: crossbeam_channel::Sender<PipelineCommand>,
    capture_thread: Option<std::thread::JoinHandle<()>>,
    injector: Option<InjectorHandle>,
}

impl MediaHandle for MacMediaHandle {
    fn stop(&mut self) {
        let _ = self.stop.send(true);
        let _ = self.pipeline.send(PipelineCommand::Stop);
        if let Some(thread) = self.capture_thread.take() {
            let _ = thread.join();
        }
        // Dropping the injector releases every held key, button and pen contact.
        self.injector.take();
    }
}

impl Drop for MacMediaHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

impl MediaEngine for MacMediaEngine {
    fn capabilities(&self) -> AgentCaps {
        let outputs = display::outputs();
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
            // macOS has no touchscreen input: direct touch is translated to pointer events.
            touch: false,
            max_fps,
        }
    }

    fn start(&self, request: MediaRequest, context: MediaContext) -> anyhow::Result<MediaStarted> {
        if !super::permissions::screen_recording_allowed() {
            anyhow::bail!(
                "Screen Recording permission is required. Allow Vision-Pro in System Settings → Privacy & Security → Screen & System Audio Recording."
            );
        }
        let settings = context.settings.clone();
        let offer = request.offer;
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
            anyhow::anyhow!("no video codec is supported by both this iPad and this Mac")
        })?;

        let outputs = display::outputs();
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

        let mut ips: Vec<std::net::Ipv4Addr> = crate::net::lan_interfaces()
            .into_iter()
            .map(|i| i.ip)
            .collect();
        if settings.advanced.developer_loopback || ips.is_empty() {
            ips.push(std::net::Ipv4Addr::LOCALHOST);
        }
        if settings.network.mode == NetworkMode::LocalOnly {
            ips.retain(|ip| crate::net::is_local_address((*ip).into()));
        }
        let negotiated = rtc::negotiate(&offer.sdp, choice.codec, &ips, kbps)?;
        let answer_sdp = negotiated.answer.clone();

        let display_rect = Arc::new(RwLock::new(output.bounds));
        let injector = InjectorHandle::spawn(display_rect.clone(), rtc::input_policy(&settings));
        // Keep at most two encoded frames waiting for the network task. Under
        // congestion, newer video replaces stale video instead of increasing
        // what the user perceives as pointer-to-image latency.
        let (events_tx, events_rx) = tokio::sync::mpsc::channel(2);
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
            events_tx,
            commands_rx,
            display_rect,
            stats.clone(),
        );

        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let rtc_context = RtcContext {
            codec: choice.codec,
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
            handle: Box::new(MacMediaHandle {
                stop: stop_tx,
                pipeline: commands_tx,
                capture_thread: Some(capture_thread),
                injector: Some(injector),
            }),
        })
    }
}
