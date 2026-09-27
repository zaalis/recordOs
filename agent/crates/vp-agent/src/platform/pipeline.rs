//! Capture → encode loop on a dedicated thread.
//!
//! Frames are encoded as soon as ScreenCaptureKit delivers them (never aligned to a fixed grid),
//! throttled to the target frame rate. When the desktop is idle the last frame is re-sent twice
//! per second. A stopped stream (display unplugged, resolution change, sleep, lock screen, revoked
//! permission) or an invalidated encoder rebuilds the pipeline automatically.

use super::capture::{Acquire, CursorState, CursorTracker, Frame, ScreenCapture};
use super::display::{self, Output};
use super::inject::SharedRect;
use super::sw_encoder::SoftwareEncoder;
use super::vt_encoder::VtEncoder;
use crate::config::{CodecId, VideoSettings};
use crate::media::codec;
use crate::media::rtc::{EncodedVideo, PipelineCommand, PipelineEvent, PipelineStats};
use crate::protocol::{AppliedVideo, ClientDisplay, DisplayInfo};
use crossbeam_channel::Receiver;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

const IDLE_REFRESH: Duration = Duration::from_millis(500);
/// The pointer is not part of the video, so the loop wakes often enough to follow it at ~120 Hz.
const CURSOR_POLL: Duration = Duration::from_millis(8);

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
}

enum Encoder {
    Hardware(VtEncoder),
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
            x: o.bounds.x.round() as i32,
            y: o.bounds.y.round() as i32,
            refresh_hz: o.refresh_hz,
            primary: o.primary,
        })
        .collect()
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
            let _wake = config.prevent_sleep.then(super::system::WakeLock::acquire);
            run(config, video, events, commands, display_rect, stats);
        })
        .expect("capture thread")
}

fn build_encoder(
    config: &PipelineConfig,
    width: u32,
    height: u32,
    fps: u32,
    target: u32,
    initial_target: u32,
    fallback: &mut Option<String>,
) -> anyhow::Result<Encoder> {
    let software = || -> anyhow::Result<Encoder> {
        Ok(Encoder::Software(
            Box::new(SoftwareEncoder::new(
                width,
                height,
                fps,
                initial_target.min(20_000),
            )?),
            vec![],
            vec![],
        ))
    };
    if !config.hardware {
        return software();
    }
    let prioritize_speed = matches!(
        config.video.profile,
        crate::config::LatencyProfile::LowLatency | crate::config::LatencyProfile::BatterySaver
    );
    match VtEncoder::new(config.codec, width, height, fps, target, prioritize_speed) {
        Ok(encoder) => Ok(Encoder::Hardware(encoder)),
        Err(error) if config.codec == CodecId::H264 => {
            tracing::warn!(%error, "hardware encoder unavailable, using software");
            *fallback = Some("Hardware encoding is unavailable; using software encoding.".into());
            software()
        }
        Err(error) => Err(error),
    }
}

fn run(
    mut config: PipelineConfig,
    video_out: tokio::sync::mpsc::Sender<EncodedVideo>,
    events: tokio::sync::mpsc::Sender<PipelineEvent>,
    commands: Receiver<PipelineCommand>,
    display_rect: SharedRect,
    stats: Arc<Mutex<PipelineStats>>,
) {
    let epoch = Instant::now();
    let mut failures = 0u32;
    let mut last_bitrate_request: Option<u32> = None;
    let mut cursor = CursorTracker::default();

    'rebuild: loop {
        if failures > 0 {
            // Back off while the desktop is unavailable (lock screen, sleep, display change).
            let delay = Duration::from_millis((100 * failures as u64).min(1000));
            match commands.recv_timeout(delay) {
                Ok(PipelineCommand::Stop)
                | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
                Ok(PipelineCommand::SelectDisplay(id)) => config.display_id = Some(id),
                Ok(PipelineCommand::Reconfigure(video)) => config.video = video,
                _ => {}
            }
        }

        let outputs = display::outputs();
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
        *display_rect.write() = output.bounds;

        let built = (|| -> anyhow::Result<(ScreenCapture, Encoder, AppliedVideo)> {
            let native = (output.width(), output.height());
            let (width, height) = codec::resolve_size(&config.video, native, &config.client);
            let fps =
                codec::resolve_fps(&config.video, output.refresh_hz, config.client.refresh_hz);
            let target = codec::resolve_bitrate(&config.video, (width, height), fps, config.codec);
            let spec = codec::profile_spec(config.video.profile);
            {
                let mut s = stats.lock();
                // The BWE floor protects from unusable video, not from a
                // quality preference. A 200 Mb/s ceiling must still be able
                // to adapt to a 10–15 Mb/s iPad Wi-Fi link.
                s.min_kbps = target.clamp(2_000, 4_000);
                s.max_kbps = match config.video.bitrate {
                    crate::config::BitrateMode::Auto => spec.max_kbps.max(target),
                    _ => target,
                };
            }
            let capture = ScreenCapture::start(&output, width, height, fps)?;
            let mut fallback = config.fallback_reason.clone();
            let initial_target = config.initial_bitrate_kbps.min(target);
            let mut encoder = build_encoder(
                &config,
                width,
                height,
                fps,
                target,
                initial_target,
                &mut fallback,
            )?;
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
            Ok((capture, encoder, applied))
        })();

        let (mut capture, mut encoder, applied) = match built {
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
        tracing::info!(display = %output.name, encoder = %applied.encoder, width = applied.width, height = applied.height, fps = applied.fps, kbps = applied.target_kbps, "pipeline ready");
        let interval = Duration::from_secs_f64(1.0 / applied.fps.max(1) as f64);
        let _ = events.try_send(PipelineEvent::Applied(applied.clone()));
        if let Some(kbps) = last_bitrate_request {
            encoder.set_bitrate(kbps);
        }

        let mut last_encode = Instant::now() - IDLE_REFRESH;
        let mut force_keyframe = true;
        let mut dirty = false;
        let mut current: Option<Frame> = None;
        let mut last_cursor = CursorState::default();
        let mut last_pts_us = -1i64;
        let mut window = (Instant::now(), 0u32, 0u32, 0f32, 0f32);

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
            if commands.is_empty() && video_out.is_closed() {
                return;
            }

            let since = last_encode.elapsed();
            let timeout = if dirty {
                interval.saturating_sub(since)
            } else {
                IDLE_REFRESH.saturating_sub(since)
            }
            .min(CURSOR_POLL);
            let capture_start = Instant::now();
            match capture.next(timeout.as_millis().clamp(0, 100) as u32) {
                Acquire::Frame(frame) => {
                    current = Some(frame);
                    dirty = true;
                    window.1 += 1;
                }
                Acquire::NoChange => {}
                Acquire::Stopped => {
                    tracing::info!("screen capture stopped; rebuilding");
                    failures = 1;
                    continue 'rebuild;
                }
            }
            let capture_ms = capture_start.elapsed().as_secs_f32() * 1000.0;

            let (state, shape) = cursor.poll(&output);
            if let Some(shape) = shape {
                let _ = events.try_send(PipelineEvent::CursorShape(shape));
            }
            if state != last_cursor {
                last_cursor = state;
                let _ = events.try_send(PipelineEvent::Cursor {
                    state,
                    width: output.width(),
                    height: output.height(),
                });
            }

            let Some(frame) = current.as_ref() else {
                continue;
            };
            let since = last_encode.elapsed();
            let due = (dirty && since + Duration::from_millis(1) >= interval)
                || since >= IDLE_REFRESH
                || (force_keyframe && since >= interval);
            if !due {
                continue;
            }

            let encode_start = Instant::now();
            let captured_at = Instant::now();
            let rtp_time = (captured_at.duration_since(epoch).as_micros() as u64) * 9 / 100;
            // VideoToolbox requires strictly increasing presentation timestamps.
            let pts_us =
                (captured_at.duration_since(epoch).as_micros() as i64).max(last_pts_us + 1);
            last_pts_us = pts_us;
            let result: anyhow::Result<Option<(Vec<u8>, bool)>> = match &mut encoder {
                Encoder::Hardware(hw) => hw.encode(frame, pts_us, force_keyframe),
                Encoder::Software(sw, y, uv) => frame
                    .copy_nv12(capture.width, capture.height, y, uv)
                    .and_then(|()| sw.encode_nv12(y, uv, force_keyframe))
                    .map(Some),
            };
            let encode_ms = encode_start.elapsed().as_secs_f32() * 1000.0;
            last_encode = Instant::now();
            dirty = false;

            match result {
                Ok(Some((data, keyframe))) if !data.is_empty() => {
                    if force_keyframe && keyframe {
                        force_keyframe = false;
                    }
                    window.2 += 1;
                    window.3 += capture_ms;
                    window.4 += encode_ms;
                    let video = EncodedVideo {
                        data,
                        keyframe,
                        captured_at,
                        rtp_time,
                    };
                    // Video has its own two-slot queue; cursor and control events never take it.
                    if video_out.try_send(video).is_err() {
                        // The network side is congested: the next frame must be a keyframe.
                        force_keyframe = true;
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(%error, "encode failed; rebuilding pipeline");
                    failures = 1;
                    continue 'rebuild;
                }
            }

            if window.0.elapsed() >= Duration::from_secs(1) {
                let secs = window.0.elapsed().as_secs_f32();
                let mut s = stats.lock();
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
                s.stats.width = capture.width;
                s.stats.height = capture.height;
                window = (Instant::now(), 0, 0, 0.0, 0.0);
            }
        }
    }
}
