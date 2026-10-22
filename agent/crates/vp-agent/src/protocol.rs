//! Strictly validated wire messages (mirror of packages/protocol/src/signaling.ts and control.ts).

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
pub struct AgentCaps {
    pub displays: Vec<DisplayInfo>,
    pub active_display_id: String,
    pub pen: bool,
    pub touch: bool,
}

/// Session messages from the iPad (inside the encrypted channel), besides `session.close` and
/// `device.rotate`.
#[derive(Debug, Deserialize)]
#[serde(tag = "t", deny_unknown_fields)]
pub enum ControlToAgent {
    /// Reliable, ordered input: one binary event (see `input`), base64url.
    #[serde(rename = "in")]
    Input { d: String },
    /// Latest-wins input (hover moves): an event older than the last one accepted is dropped.
    #[serde(rename = "rt")]
    Realtime { d: String },
    /// `rttMs`: the round trip the iPad measured on the previous ping, shown on the PC.
    #[serde(rename = "ping")]
    Ping {
        id: u32,
        t0: f64,
        #[serde(default, rename = "rttMs")]
        rtt_ms: Option<f32>,
    },
    #[serde(rename = "display.select")]
    DisplaySelect {
        #[serde(rename = "displayId")]
        display_id: String,
    },
    #[serde(rename = "app.watch")]
    AppWatch { enabled: bool },
}

#[cfg(test)]
mod tests {
    use super::ControlToAgent;

    #[test]
    fn session_messages_are_strict() {
        let ping: ControlToAgent =
            serde_json::from_str(r#"{"t":"ping","id":3,"t0":12.5,"rttMs":4.2}"#).unwrap();
        assert!(matches!(
            ping,
            ControlToAgent::Ping {
                id: 3,
                rtt_ms: Some(_),
                ..
            }
        ));
        assert!(serde_json::from_str::<ControlToAgent>(r#"{"t":"in","d":"AQ"}"#).is_ok());
        assert!(serde_json::from_str::<ControlToAgent>(r#"{"t":"in","d":"AQ","x":1}"#).is_err());
        assert!(
            serde_json::from_str::<ControlToAgent>(r#"{"t":"rtc.offer","sdp":"v=0"}"#).is_err()
        );
    }
}
