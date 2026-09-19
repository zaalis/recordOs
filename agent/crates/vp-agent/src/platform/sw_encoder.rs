//! Software H.264 fallback (OpenH264) for PCs without a usable hardware encoder.

use crate::media::codec::EncoderCapability;
use openh264::encoder::{
    BitRate, Complexity, Encoder, EncoderConfig, FrameRate, RateControlMode, SpsPpsStrategy,
    UsageType,
};
use openh264::formats::YUVSource;
use openh264::OpenH264API;

pub fn capability() -> EncoderCapability {
    EncoderCapability {
        codec: crate::config::CodecId::H264,
        hardware: false,
        name: "OpenH264 (software)".into(),
    }
}

struct I420<'a> {
    width: usize,
    height: usize,
    y: &'a [u8],
    u: &'a [u8],
    v: &'a [u8],
}

impl YUVSource for I420<'_> {
    fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }
    fn strides(&self) -> (usize, usize, usize) {
        (self.width, self.width / 2, self.width / 2)
    }
    fn y(&self) -> &[u8] {
        self.y
    }
    fn u(&self) -> &[u8] {
        self.u
    }
    fn v(&self) -> &[u8] {
        self.v
    }
}

pub struct SoftwareEncoder {
    encoder: Encoder,
    width: usize,
    height: usize,
    u: Vec<u8>,
    v: Vec<u8>,
    bitrate_kbps: u32,
    fps: u32,
}

impl SoftwareEncoder {
    pub fn new(
        width: u32,
        height: u32,
        fps: u32,
        bitrate_kbps: u32,
    ) -> anyhow::Result<SoftwareEncoder> {
        let encoder = Self::build(fps, bitrate_kbps)?;
        Ok(SoftwareEncoder {
            encoder,
            width: width as usize,
            height: height as usize,
            u: vec![],
            v: vec![],
            bitrate_kbps,
            fps,
        })
    }

    fn build(fps: u32, bitrate_kbps: u32) -> anyhow::Result<Encoder> {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(4))
            .unwrap_or(2) as u16;
        let config = EncoderConfig::new()
            .bitrate(BitRate::from_bps(bitrate_kbps.saturating_mul(1000)))
            .max_frame_rate(FrameRate::from_hz(fps as f32))
            .usage_type(UsageType::ScreenContentRealTime)
            .rate_control_mode(RateControlMode::Bitrate)
            .sps_pps_strategy(SpsPpsStrategy::ConstantId)
            .complexity(Complexity::Low)
            .skip_frames(false)
            .num_threads(threads);
        Ok(Encoder::with_api_config(
            OpenH264API::from_source(),
            config,
        )?)
    }

    pub fn set_bitrate(&mut self, kbps: u32) {
        if kbps.abs_diff(self.bitrate_kbps) * 5 > self.bitrate_kbps {
            // OpenH264 has no cheap runtime bitrate setter in this binding; rebuild on large changes.
            if let Ok(encoder) = Self::build(self.fps, kbps) {
                self.encoder = encoder;
                self.bitrate_kbps = kbps;
            }
        }
    }

    pub fn bitrate_kbps(&self) -> u32 {
        self.bitrate_kbps
    }

    /// Encodes NV12 planes (Y full size, interleaved UV half size).
    pub fn encode_nv12(
        &mut self,
        y: &[u8],
        uv: &[u8],
        force_keyframe: bool,
    ) -> anyhow::Result<(Vec<u8>, bool)> {
        let (w, h) = (self.width, self.height);
        let cw = w / 2;
        let ch = h / 2;
        self.u.resize(cw * ch, 0);
        self.v.resize(cw * ch, 0);
        for row in 0..ch {
            let src = &uv[row * w..row * w + cw * 2];
            for col in 0..cw {
                self.u[row * cw + col] = src[col * 2];
                self.v[row * cw + col] = src[col * 2 + 1];
            }
        }
        if force_keyframe {
            self.encoder.force_intra_frame();
        }
        let source = I420 {
            width: w,
            height: h,
            y,
            u: &self.u,
            v: &self.v,
        };
        let bitstream = self.encoder.encode(&source)?;
        let keyframe = matches!(
            bitstream.frame_type(),
            openh264::encoder::FrameType::IDR | openh264::encoder::FrameType::I
        );
        Ok((bitstream.to_vec(), keyframe))
    }
}
