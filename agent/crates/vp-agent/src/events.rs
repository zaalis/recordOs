//! Events broadcast to the desktop UI (and any other observer).

use crate::security::AuditEntry;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingRequestInfo {
    pub id: String,
    pub device_id: String,
    pub device_name: String,
    pub device_kind: String,
    pub ip: String,
    /// Six digits shown on both screens; the user checks they match.
    pub sas: String,
    /// "nearby", "qr", "code" or "reconfirm" (known device whose trust expired, or confirm-every-connection).
    pub method: String,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentEvent {
    #[serde(rename_all = "camelCase")]
    ServiceChanged {
        running: bool,
        error: Option<String>,
    },
    PairingRequested(PairingRequestInfo),
    #[serde(rename_all = "camelCase")]
    PairingResolved {
        id: String,
        outcome: String,
    },
    SessionsChanged,
    DevicesChanged,
    SettingsChanged,
    ConnectWindowChanged,
    RelayChanged,
    /// A tablet or a cable network interface appeared or disappeared.
    UsbChanged,
    /// This Mac's local addresses changed (certificate and discovery were refreshed).
    NetworkChanged,
    Audit(AuditEntry),
}
