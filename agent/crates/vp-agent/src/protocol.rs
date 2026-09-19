//! Strictly validated wire messages (mirror of packages/protocol/src/signaling.ts and media.ts).

use crate::config::{CodecId, VideoSettings};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_DEVICE_NAME: usize = 48;

#[derive(Debug, thiserror::Error)]
#[error("invalid message: {0}")]
pub struct Invalid(pub &'static str);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClientInit {
    pub t: String,
    pub v: u32,
    pub mode: String,
    pub client_eph: String,
    #[serde(rename = "nC")]
    pub n_c: String,
    pub device_id: String,
    pub device_pub: String,
    pub device_name: String,
    pub device_kind: String,
}

impl ClientInit {
    pub fn validate(&self) -> Result<(), Invalid> {
        if self.t != "init" {
            return Err(Invalid("type"));
        }
        if self.v != PROTOCOL_VERSION {
            return Err(Invalid("version"));
        }
        if self.mode != "pair" && self.mode != "auth" {
            return Err(Invalid("mode"));
        }
        if self.device_id.len() != 22
            || self.device_name.is_empty()
            || self.device_name.chars().count() > MAX_DEVICE_NAME
        {
            return Err(Invalid("device"));
        }
        if !matches!(self.device_kind.as_str(), "ipad" | "tablet" | "browser") {
            return Err(Invalid("kind"));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientProve {
    pub t: String,
    pub sig: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairProof {
    pub kind: String,
    pub mac: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairRequest {
    pub t: String,
    #[serde(default)]
    pub proof: Option<PairProof>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClientDisplay {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClientCodec {
    pub codec: CodecId,
    pub power_efficient: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RtcOffer {
    pub t: String,
    pub sdp: String,
    pub video: VideoSettings,
    pub display: ClientDisplay,
    pub codecs: Vec<ClientCodec>,
}

impl RtcOffer {
    pub fn validate(&self) -> Result<(), Invalid> {
        if self.sdp.len() < 10 || self.sdp.len() > 48_000 {
            return Err(Invalid("sdp"));
        }
        validate_video(&self.video)?;
        let d = self.display;
        if !(100..=8192).contains(&d.width)
            || !(100..=8192).contains(&d.height)
            || !(24.0..=240.0).contains(&d.refresh_hz)
        {
            return Err(Invalid("display"));
        }
        if self.codecs.len() > 8 {
            return Err(Invalid("codecs"));
        }
        Ok(())
    }
}

pub fn validate_video(video: &VideoSettings) -> Result<(), Invalid> {
    if let Some(w) = video.custom_width {
        if !(320..=7680).contains(&w) {
            return Err(Invalid("customWidth"));
        }
    }
    if let Some(h) = video.custom_height {
        if !(240..=4320).contains(&h) {
            return Err(Invalid("customHeight"));
        }
    }
    if let Some(k) = video.custom_kbps {
        if !(500..=200_000).contains(&k) {
            return Err(Invalid("customKbps"));
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeviceRotate {
    pub t: String,
    pub new_pub: String,
    pub sig: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub refresh_hz: u32,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncoderInfo {
    pub codec: CodecId,
    pub hardware: bool,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCaps {
    pub encoders: Vec<EncoderInfo>,
    pub displays: Vec<DisplayInfo>,
    pub active_display_id: String,
    pub pen: bool,
    pub touch: bool,
    pub max_fps: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppliedVideo {
    pub codec: CodecId,
    pub encoder: String,
    pub hardware: bool,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub target_kbps: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStats {
    pub capture_fps: f32,
    pub encode_fps: f32,
    pub sent_fps: f32,
    pub capture_ms: f32,
    pub encode_ms: f32,
    pub target_kbps: u32,
    pub sent_kbps: u32,
    pub bwe_kbps: u32,
    pub encoder: String,
    pub encoder_state: String,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub input_events_per_sec: f32,
    pub input_process_ms: f32,
}

/// Control data channel messages from the iPad.
#[derive(Debug, Deserialize)]
#[serde(tag = "t", deny_unknown_fields)]
pub enum ControlToAgent {
    #[serde(rename = "ping")]
    Ping { id: u32, t0: f64 },
    #[serde(rename = "video.settings")]
    VideoSettings { video: VideoSettings },
    #[serde(rename = "display.select")]
    DisplaySelect {
        #[serde(rename = "displayId")]
        display_id: String,
    },
    #[serde(rename = "keyframe")]
    Keyframe {},
    #[serde(rename = "app.watch")]
    AppWatch { enabled: bool },
}
