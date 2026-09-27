//! Hardware video encoding with VideoToolbox (Apple silicon media engine, or Intel Quick Sync on
//! Intel Macs). Captured IOSurfaces are submitted directly — no CPU copy — in real-time mode with
//! low-latency rate control, no B-frames and on-demand IDR frames. Output is Annex B.

use super::capture::Frame;
use super::ffi;
use crate::config::CodecId;
use crate::media::codec::EncoderCapability;
use std::ffi::{c_char, c_void};
use std::ptr::NonNull;

/// Chip name used in encoder labels ("Apple M3 Pro").
fn chip_name() -> String {
    ffi::read_string(256, |out, len| unsafe { ffi::vp_cpu_brand(out, len) })
        .map(|brand| brand.trim().to_string())
        .unwrap_or_else(|| "Mac".into())
}

fn label(codec: CodecId) -> String {
    format!("{} {} (VideoToolbox)", chip_name(), codec.display())
}

/// Hardware encoders available on this Mac. AV1 encoding has no hardware path on macOS.
pub fn probe() -> Vec<EncoderCapability> {
    let mask = unsafe { ffi::vp_hardware_encoders() };
    let mut list = vec![];
    for (bit, codec) in [(1u32, CodecId::H264), (2u32, CodecId::Hevc)] {
        if mask & bit != 0 {
            list.push(EncoderCapability {
                codec,
                hardware: true,
                name: label(codec),
            });
        }
    }
    list
}

pub struct VtEncoder {
    raw: NonNull<c_void>,
    pub name: String,
    pub codec: CodecId,
    bitrate_kbps: u32,
}

// The VideoToolbox session is only driven from the pipeline thread that owns this value.
unsafe impl Send for VtEncoder {}

impl VtEncoder {
    pub fn new(
        codec: CodecId,
        width: u32,
        height: u32,
        fps: u32,
        bitrate_kbps: u32,
        prioritize_speed: bool,
    ) -> anyhow::Result<VtEncoder> {
        let codec_id = match codec {
            CodecId::H264 => ffi::VP_CODEC_H264,
            CodecId::Hevc => ffi::VP_CODEC_HEVC,
            CodecId::Av1 => anyhow::bail!("AV1 hardware encoding is not available on macOS"),
        };
        let mut error = [0 as c_char; 512];
        let raw = unsafe {
            ffi::vp_encoder_create(
                codec_id,
                width,
                height,
                fps,
                bitrate_kbps,
                prioritize_speed,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        let Some(raw) = NonNull::new(raw) else {
            let message = ffi::c_buffer_to_string(&error);
            anyhow::bail!(if message.is_empty() {
                format!(
                    "VideoToolbox could not create a {} encoder",
                    codec.display()
                )
            } else {
                message
            });
        };
        let hardware = unsafe { ffi::vp_encoder_is_hardware(raw.as_ptr()) };
        let mut name = label(codec);
        if !hardware {
            name = format!("{} (software fallback)", name);
        }
        Ok(VtEncoder {
            raw,
            name,
            codec,
            bitrate_kbps,
        })
    }

    pub fn set_bitrate(&mut self, kbps: u32) {
        if kbps == self.bitrate_kbps {
            return;
        }
        self.bitrate_kbps = kbps;
        unsafe { ffi::vp_encoder_set_bitrate(self.raw.as_ptr(), kbps) }
    }

    pub fn bitrate_kbps(&self) -> u32 {
        self.bitrate_kbps
    }

    /// Encodes one frame synchronously and returns the Annex B access unit.
    pub fn encode(
        &mut self,
        frame: &Frame,
        pts_us: i64,
        force_keyframe: bool,
    ) -> anyhow::Result<Option<(Vec<u8>, bool)>> {
        let mut data: *const u8 = std::ptr::null();
        let mut len = 0usize;
        let mut keyframe = false;
        let status = unsafe {
            ffi::vp_encoder_encode(
                self.raw.as_ptr(),
                frame.as_ptr(),
                pts_us,
                force_keyframe,
                &mut data,
                &mut len,
                &mut keyframe,
            )
        };
        anyhow::ensure!(
            status == 0,
            "VideoToolbox encode failed (OSStatus {status})"
        );
        if data.is_null() || len == 0 {
            return Ok(None);
        }
        let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
        Ok(Some((bytes, keyframe)))
    }
}

impl Drop for VtEncoder {
    fn drop(&mut self) {
        unsafe { ffi::vp_encoder_destroy(self.raw.as_ptr()) }
    }
}
