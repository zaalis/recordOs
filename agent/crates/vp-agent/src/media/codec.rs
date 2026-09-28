//! Automatic stream planning: codec selection against what the iPad offers and what this PC can
//! encode, plus resolution, frame rate and bitrate derived from the latency profile.
//! A codec is never forced if either side cannot handle it.

use crate::config::{
    BitrateMode, CodecId, CodecSetting, EncoderPreference, FpsSetting, LatencyProfile,
    ResolutionMode, VideoSettings,
};
use crate::protocol::{ClientCodec, ClientDisplay};

#[derive(Debug, Clone, PartialEq)]
pub struct EncoderCapability {
    pub codec: CodecId,
    pub hardware: bool,
    pub name: String,
}

pub struct ProfileSpec {
    pub max_auto_height: Option<u32>,
    pub auto_fps: u32,
    pub bits_per_pixel: f64,
    pub min_kbps: u32,
    pub max_kbps: u32,
    pub codec_preference: &'static [CodecId],
}

pub fn profile_spec(profile: LatencyProfile) -> ProfileSpec {
    use CodecId::*;
    match profile {
        LatencyProfile::Balanced => ProfileSpec {
            max_auto_height: Some(1440),
            auto_fps: 60,
            bits_per_pixel: 0.1,
            min_kbps: 4_000,
            max_kbps: 40_000,
            codec_preference: &[Hevc, H264, Av1],
        },
        LatencyProfile::LowLatency => ProfileSpec {
            max_auto_height: Some(1080),
            auto_fps: 120,
            bits_per_pixel: 0.08,
            min_kbps: 4_000,
            max_kbps: 30_000,
            codec_preference: &[H264, Hevc, Av1],
        },
        LatencyProfile::MaxQuality => ProfileSpec {
            max_auto_height: None,
            auto_fps: 60,
            bits_per_pixel: 0.16,
            min_kbps: 10_000,
            max_kbps: 200_000,
            codec_preference: &[Av1, Hevc, H264],
        },
        LatencyProfile::BatterySaver => ProfileSpec {
            max_auto_height: Some(1080),
            auto_fps: 30,
            bits_per_pixel: 0.07,
            min_kbps: 2_000,
            max_kbps: 10_000,
            codec_preference: &[Hevc, H264],
        },
    }
}

/// Video codecs present in an SDP offer (`a=rtpmap:<pt> H264/90000`, `H265`, `AV1`).
pub fn offered_codecs(sdp: &str) -> Vec<CodecId> {
    let mut found = vec![];
    for line in sdp.lines() {
        let Some(rest) = line.trim().strip_prefix("a=rtpmap:") else {
            continue;
        };
        let Some(encoding) = rest.split_whitespace().nth(1) else {
            continue;
        };
        let name = encoding
            .split('/')
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        let codec = match name.as_str() {
            "H264" => CodecId::H264,
            "H265" => CodecId::Hevc,
            "AV1" | "AV1X" => CodecId::Av1,
            _ => continue,
        };
        if !found.contains(&codec) {
            found.push(codec);
        }
    }
    found
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodecChoice {
    pub codec: CodecId,
    pub encoder: EncoderCapability,
    pub fallback_reason: Option<String>,
}

pub fn choose_codec(
    setting: CodecSetting,
    profile: LatencyProfile,
    offered: &[CodecId],
    client: &[ClientCodec],
    encoders: &[EncoderCapability],
    preference: EncoderPreference,
) -> Option<CodecChoice> {
    let usable: Vec<&EncoderCapability> = encoders
        .iter()
        .filter(|e| match preference {
            EncoderPreference::Auto => true,
            EncoderPreference::HardwareOnly => e.hardware,
            EncoderPreference::Software => !e.hardware,
        })
        .collect();
    let client_efficient = |codec: CodecId| {
        client
            .iter()
            .find(|c| c.codec == codec)
            .map(|c| c.power_efficient)
    };
    let best_encoder = |codec: CodecId| -> Option<&EncoderCapability> {
        if !offered.contains(&codec)
            || client_efficient(codec) == Some(false) && codec != CodecId::H264
        {
            return None;
        }
        usable
            .iter()
            .copied()
            .filter(|e| e.codec == codec)
            .max_by_key(|e| e.hardware)
    };

    let requested = match setting {
        CodecSetting::Auto => None,
        CodecSetting::H264 => Some(CodecId::H264),
        CodecSetting::Hevc => Some(CodecId::Hevc),
        CodecSetting::Av1 => Some(CodecId::Av1),
    };
    let mut fallback_reason = None;
    if let Some(codec) = requested {
        if let Some(encoder) = best_encoder(codec) {
            return Some(CodecChoice {
                codec,
                encoder: encoder.clone(),
                fallback_reason: None,
            });
        }
        let ipad_ok = offered.contains(&codec) && client_efficient(codec) != Some(false);
        fallback_reason = Some(if !ipad_ok {
            format!(
                "{} isn't supported by this iPad. Using the best available codec.",
                codec.display()
            )
        } else {
            format!(
                "This PC can't encode {}. Using the best available codec.",
                codec.display()
            )
        });
    }

    let spec = profile_spec(profile);
    // Hardware encoders in profile order first, then any hardware encoder, then software.
    let ordered =
        spec.codec_preference
            .iter()
            .copied()
            .chain([CodecId::H264, CodecId::Hevc, CodecId::Av1]);
    let mut software: Option<CodecChoice> = None;
    for codec in ordered {
        if let Some(encoder) = best_encoder(codec) {
            let choice = CodecChoice {
                codec,
                encoder: encoder.clone(),
                fallback_reason: fallback_reason.clone(),
            };
            if encoder.hardware {
                return Some(choice);
            }
            software.get_or_insert(choice);
        }
    }
    software
}

fn even(value: f64) -> u32 {
    ((value.round() as u32) / 2 * 2).max(2)
}

/// Output size preserving the captured display's aspect ratio. Never upscales.
pub fn resolve_size(
    settings: &VideoSettings,
    native: (u32, u32),
    client: &ClientDisplay,
) -> (u32, u32) {
    let (nw, nh) = (native.0.max(2), native.1.max(2));
    let aspect = nw as f64 / nh as f64;
    let from_height = |h: u32| {
        let h = h.min(nh) as f64;
        (even(h * aspect), even(h))
    };
    let size = match settings.resolution {
        ResolutionMode::Native => (nw & !1, nh & !1),
        ResolutionMode::P720 => from_height(720),
        ResolutionMode::P1080 => from_height(1080),
        ResolutionMode::P1440 => from_height(1440),
        ResolutionMode::P2160 => from_height(2160),
        ResolutionMode::P4320 => from_height(4320),
        ResolutionMode::Custom => {
            let bw = settings.custom_width.unwrap_or(nw).min(nw) as f64;
            let bh = settings.custom_height.unwrap_or(nh).min(nh) as f64;
            let scale = (bw / nw as f64).min(bh / nh as f64);
            (even(nw as f64 * scale), even(nh as f64 * scale))
        }
        ResolutionMode::Auto => {
            // Largest size that is actually visible on the iPad when letterboxed.
            let fit = (client.width as f64 / nw as f64).min(client.height as f64 / nh as f64);
            let visible_h = (nh as f64 * fit).round() as u32;
            let cap = profile_spec(settings.profile).max_auto_height.unwrap_or(nh);
            from_height(visible_h.min(cap).max(480))
        }
    };
    (size.0.max(320), size.1.max(240))
}

pub fn resolve_fps(settings: &VideoSettings, display_refresh: u32, client_refresh: f64) -> u32 {
    let client_cap = if client_refresh >= 110.0 {
        120
    } else if client_refresh >= 85.0 {
        90
    } else {
        60
    };
    let display_cap = display_refresh.max(30);
    let wanted = match settings.fps {
        FpsSetting::Auto => profile_spec(settings.profile).auto_fps,
        FpsSetting::Fixed(v) => v,
    };
    wanted.min(client_cap).min(display_cap).max(24)
}

pub fn resolve_bitrate(
    settings: &VideoSettings,
    size: (u32, u32),
    fps: u32,
    codec: CodecId,
) -> u32 {
    let spec = profile_spec(settings.profile);
    match settings.bitrate {
        BitrateMode::Custom => settings.custom_kbps.unwrap_or(15_000).clamp(500, 200_000),
        BitrateMode::Low => 5_000,
        BitrateMode::Medium => 15_000,
        BitrateMode::High => 30_000,
        // 200 Mbps is the ceiling for a direct, strong local LAN. TWCC still
        // lowers this promptly if the iPad Wi-Fi cannot sustain it.
        BitrateMode::Max => 200_000,
        BitrateMode::Auto => {
            let efficiency = if codec == CodecId::H264 { 1.0 } else { 0.7 };
            // Screen content is mostly static; fps contributes sub-linearly.
            let motion = (fps as f64 / 60.0).sqrt() * 60.0;
            let kbps =
                size.0 as f64 * size.1 as f64 * motion * spec.bits_per_pixel * efficiency / 1000.0;
            (kbps as u32).clamp(spec.min_kbps, spec.max_kbps)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoders() -> Vec<EncoderCapability> {
        vec![
            EncoderCapability {
                codec: CodecId::H264,
                hardware: true,
                name: "NVIDIA H.264".into(),
            },
            EncoderCapability {
                codec: CodecId::Hevc,
                hardware: true,
                name: "NVIDIA HEVC".into(),
            },
            EncoderCapability {
                codec: CodecId::H264,
                hardware: false,
                name: "OpenH264".into(),
            },
        ]
    }

    #[test]
    fn parses_offer() {
        let sdp =
            "v=0\r\na=rtpmap:96 H264/90000\r\na=rtpmap:98 H265/90000\r\na=rtpmap:100 VP8/90000\r\n";
        assert_eq!(offered_codecs(sdp), vec![CodecId::H264, CodecId::Hevc]);
    }

    #[test]
    fn never_forces_unsupported_codec() {
        let choice = choose_codec(
            CodecSetting::Av1,
            LatencyProfile::Balanced,
            &[CodecId::H264, CodecId::Hevc],
            &[],
            &encoders(),
            EncoderPreference::Auto,
        )
        .unwrap();
        assert_eq!(choice.codec, CodecId::Hevc);
        assert!(choice.fallback_reason.unwrap().contains("iPad"));
        let choice = choose_codec(
            CodecSetting::Auto,
            LatencyProfile::LowLatency,
            &[CodecId::H264, CodecId::Hevc],
            &[],
            &encoders(),
            EncoderPreference::Auto,
        )
        .unwrap();
        assert_eq!(choice.codec, CodecId::H264);
        assert!(choice.encoder.hardware);
        let choice = choose_codec(
            CodecSetting::Auto,
            LatencyProfile::Balanced,
            &[CodecId::H264],
            &[],
            &encoders(),
            EncoderPreference::Software,
        )
        .unwrap();
        assert!(!choice.encoder.hardware);
        assert!(choose_codec(
            CodecSetting::Auto,
            LatencyProfile::Balanced,
            &[CodecId::Av1],
            &[],
            &encoders(),
            EncoderPreference::Auto
        )
        .is_none());
    }

    #[test]
    fn sizes_preserve_aspect_and_never_upscale() {
        let client = ClientDisplay {
            width: 2388,
            height: 1668,
            refresh_hz: 120.0,
        };
        // This case verifies the responsive Auto-resolution cap independently
        // from an explicit user choice.
        let mut s = VideoSettings {
            resolution: ResolutionMode::Auto,
            ..VideoSettings::default()
        };
        assert_eq!(resolve_size(&s, (3840, 2160), &client), (2388, 1342));
        s.resolution = ResolutionMode::P1080;
        assert_eq!(resolve_size(&s, (1920, 1080), &client), (1920, 1080));
        s.resolution = ResolutionMode::P1440;
        assert_eq!(resolve_size(&s, (1920, 1080), &client), (1920, 1080));
        s.resolution = ResolutionMode::P2160;
        assert_eq!(resolve_size(&s, (3840, 2160), &client), (3840, 2160));
        s.resolution = ResolutionMode::P4320;
        assert_eq!(resolve_size(&s, (3840, 2160), &client), (3840, 2160));
        s.resolution = ResolutionMode::Native;
        assert_eq!(resolve_size(&s, (3440, 1440), &client), (3440, 1440));
        assert_eq!(resolve_fps(&s, 60, 120.0), 60);
        assert_eq!(resolve_fps(&s, 144, 60.0), 60);
        s.fps = FpsSetting::Fixed(120);
        assert_eq!(resolve_fps(&s, 144, 120.0), 120);
        assert_eq!(resolve_fps(&s, 144, 60.0), 60);
    }

    #[test]
    fn maximum_quality_ceiling_is_two_hundred_mbps() {
        let settings = VideoSettings {
            profile: LatencyProfile::MaxQuality,
            bitrate: BitrateMode::Max,
            ..VideoSettings::default()
        };
        assert_eq!(
            resolve_bitrate(&settings, (3840, 2160), 60, CodecId::Hevc),
            200_000
        );
        assert_eq!(profile_spec(LatencyProfile::MaxQuality).max_kbps, 200_000);
    }
}
