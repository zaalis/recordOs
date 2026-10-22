//! Persistent agent settings (%APPDATA%\Vision-Pro\settings.json).
//! Every field has a secure default; unknown fields are ignored and missing ones take defaults.

use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkMode {
    /// Only devices on the local network may connect. Recommended.
    LocalOnly,
    /// Connections from outside the LAN are allowed through the relay, with extra rules.
    RemoteAccess,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PenMode {
    /// Native Windows Ink pen with pressure and tilt.
    WindowsInk,
    /// Pencil behaves like a mouse.
    Mouse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetworkSettings {
    pub mode: NetworkMode,
    pub https_port: u16,
    pub advertise_mdns: bool,
    /// Vision-Pro relay used in Remote Access mode, normalized to `wss://host[:port]`.
    pub relay_url: Option<String>,
    /// Access token required by private relays.
    pub relay_token: Option<String>,
    /// Detect tablets plugged in with a cable and carry their sessions over that cable.
    pub usb_direct: bool,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            mode: NetworkMode::RemoteAccess,
            https_port: 47800,
            advertise_mdns: false,
            relay_url: Some("wss://vision-pro.34-73-186-88.sslip.io".into()),
            relay_token: None,
            usb_direct: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SecuritySettings {
    /// Every connection (not only pairing) must be confirmed on the PC.
    pub confirm_every_connection: bool,
    /// Sessions end after this long and silently re-authenticate.
    pub session_max_minutes: u32,
    /// Trusted devices that have not connected for this many days lose trust.
    pub trust_expiry_days: u32,
    pub lockout_threshold: u32,
    pub lockout_minutes: u32,
    pub pairing_timeout_seconds: u32,
    pub notify_on_connect: bool,
    /// Keyboard shortcut that immediately ends every session.
    pub emergency_shortcut: String,
}

impl Default for SecuritySettings {
    fn default() -> Self {
        Self {
            confirm_every_connection: false,
            session_max_minutes: 12 * 60,
            trust_expiry_days: 90,
            lockout_threshold: 5,
            lockout_minutes: 15,
            pairing_timeout_seconds: 120,
            notify_on_connect: true,
            emergency_shortcut: "Ctrl+Alt+Shift+Backspace".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InputSettings {
    pub allow_keyboard: bool,
    pub allow_mouse: bool,
    pub allow_pen: bool,
    pub allow_touch: bool,
    pub pen_mode: PenMode,
    pub detect_active_app: bool,
}

impl Default for InputSettings {
    fn default() -> Self {
        Self {
            allow_keyboard: true,
            allow_mouse: true,
            allow_pen: true,
            allow_touch: true,
            pen_mode: PenMode::WindowsInk,
            detect_active_app: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DisplaySettings {
    /// Display streamed by default; `None` = primary display.
    pub display_id: Option<String>,
    pub prevent_sleep: bool,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            display_id: None,
            prevent_sleep: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AdvancedSettings {
    /// Loopback-only HTTP endpoint for development and testing. Never reachable from the network.
    pub developer_loopback: bool,
    pub verbose_logging: bool,
    pub start_service_on_launch: bool,
    pub launch_at_login: bool,
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            developer_loopback: false,
            verbose_logging: false,
            start_service_on_launch: true,
            launch_at_login: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema: u32,
    pub computer_name: String,
    pub network: NetworkSettings,
    pub security: SecuritySettings,
    pub input: InputSettings,
    pub display: DisplaySettings,
    pub advanced: AdvancedSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: 4,
            computer_name: default_computer_name(),
            network: NetworkSettings::default(),
            security: SecuritySettings::default(),
            input: InputSettings::default(),
            display: DisplaySettings::default(),
            advanced: AdvancedSettings::default(),
        }
    }
}

pub fn default_computer_name() -> String {
    let user = std::env::var("USERNAME").ok().filter(|u| !u.is_empty());
    match user {
        Some(user) => {
            let mut chars = user.chars();
            let first = chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>())
                .unwrap_or_default();
            format!("{}{}'s PC", first, chars.as_str())
        }
        None => gethostname::gethostname().to_string_lossy().to_string(),
    }
}

impl Settings {
    fn path() -> PathBuf {
        paths::data_dir().join("settings.json")
    }

    pub fn load() -> Settings {
        let path = Self::path();
        match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Settings>(&bytes) {
                Ok(mut settings) => {
                    settings.network.mode = NetworkMode::RemoteAccess;
                    settings.network.advertise_mdns = false;
                    settings.network.usb_direct = false;
                    if settings.network.relay_url.is_none() {
                        settings.network.relay_url = NetworkSettings::default().relay_url;
                    }
                    settings.sanitize();
                    settings
                }
                Err(error) => {
                    tracing::warn!(%error, "settings file is invalid, using defaults");
                    let _ = std::fs::rename(&path, path.with_extension("invalid.json"));
                    Settings::default()
                }
            },
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        paths::ensure_data_dir()?;
        let bytes = serde_json::to_vec_pretty(self).expect("settings serialize");
        paths::write_atomic(&Self::path(), &bytes)
    }

    /// Clamps values into safe ranges regardless of what is on disk or sent by the UI.
    pub fn sanitize(&mut self) {
        self.computer_name = crate::util::sanitize_name(&self.computer_name, 48);
        if self.network.https_port < 1024 {
            self.network.https_port = 47800;
        }
        let s = &mut self.security;
        s.session_max_minutes = s.session_max_minutes.clamp(5, 7 * 24 * 60);
        s.trust_expiry_days = s.trust_expiry_days.clamp(1, 3650);
        s.lockout_threshold = s.lockout_threshold.clamp(3, 20);
        s.lockout_minutes = s.lockout_minutes.clamp(1, 24 * 60);
        s.pairing_timeout_seconds = s.pairing_timeout_seconds.clamp(30, 600);
        self.network.relay_url = self
            .network
            .relay_url
            .as_deref()
            .and_then(crate::relay::normalize_url);
        self.network.relay_token = self
            .network
            .relay_token
            .as_deref()
            .map(str::trim)
            .filter(|t| (16..=256).contains(&t.len()) && t.chars().all(|c| c.is_ascii_graphic()))
            .map(str::to_string);
    }
}

#[cfg(test)]
mod tests {
    use super::Settings;

    #[test]
    fn relay_settings_are_sanitized() {
        let mut settings = Settings::default();
        settings.network.relay_url = Some(" https://Relay.example.com/ ".into());
        settings.network.relay_token = Some("short".into());
        settings.sanitize();
        assert_eq!(
            settings.network.relay_url.as_deref(),
            Some("wss://relay.example.com")
        );
        assert_eq!(settings.network.relay_token, None);
        settings.network.relay_url = Some("ws://relay.example.com".into());
        settings.network.relay_token = Some("0123456789abcdef".into());
        settings.sanitize();
        assert_eq!(settings.network.relay_url, None);
        assert_eq!(
            settings.network.relay_token.as_deref(),
            Some("0123456789abcdef")
        );
    }

    #[test]
    fn internet_defaults_disable_local_discovery_and_usb() {
        assert!(!Settings::default().network.usb_direct);
        assert_eq!(
            Settings::default().network.mode,
            super::NetworkMode::RemoteAccess
        );
        assert!(!Settings::default().network.advertise_mdns);
        let older: Settings =
            serde_json::from_str(r#"{"network":{"mode":"local-only","httpsPort":47800}}"#).unwrap();
        assert!(!older.network.usb_direct);
    }

    #[test]
    fn settings_from_the_streaming_versions_still_load() {
        let older: Settings = serde_json::from_str(
            r#"{"schema":3,"network":{"mode":"local-only","httpsPort":47800,"allowNearbyPairing":true,"stunServers":[]},
                "streaming":{"video":{"profile":"balanced","resolution":"1080p","fps":60,"bitrate":"auto","codec":"h264"}},
                "input":{"allowPen":false}}"#,
        )
        .unwrap();
        assert!(!older.input.allow_pen);
        assert_eq!(older.network.https_port, 47800);
    }
}
