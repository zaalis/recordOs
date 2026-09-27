//! Persistent agent settings (~/Library/Application Support/Vision-Pro/settings.json).
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
pub enum LatencyProfile {
    Balanced,
    LowLatency,
    MaxQuality,
    BatterySaver,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CodecId {
    H264,
    Hevc,
    Av1,
}

impl CodecId {
    pub fn as_str(self) -> &'static str {
        match self {
            CodecId::H264 => "h264",
            CodecId::Hevc => "hevc",
            CodecId::Av1 => "av1",
        }
    }
    pub fn display(self) -> &'static str {
        match self {
            CodecId::H264 => "H.264",
            CodecId::Hevc => "HEVC",
            CodecId::Av1 => "AV1",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CodecSetting {
    Auto,
    H264,
    Hevc,
    Av1,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResolutionMode {
    Auto,
    #[serde(rename = "720p")]
    P720,
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "1440p")]
    P1440,
    #[serde(rename = "2160p")]
    P2160,
    #[serde(rename = "4320p")]
    P4320,
    Native,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BitrateMode {
    Auto,
    Low,
    Medium,
    High,
    Max,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FpsSetting {
    Auto,
    Fixed(u32),
}

impl Serialize for FpsSetting {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            FpsSetting::Auto => s.serialize_str("auto"),
            FpsSetting::Fixed(v) => s.serialize_u32(*v),
        }
    }
}

impl<'de> Deserialize<'de> for FpsSetting {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(d)?;
        match value {
            serde_json::Value::String(s) if s == "auto" => Ok(FpsSetting::Auto),
            serde_json::Value::Number(n) => match n.as_u64() {
                Some(v @ (30 | 60 | 90 | 120)) => Ok(FpsSetting::Fixed(v as u32)),
                _ => Err(serde::de::Error::custom("unsupported fps")),
            },
            _ => Err(serde::de::Error::custom("invalid fps")),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VideoSettings {
    pub profile: LatencyProfile,
    pub resolution: ResolutionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_height: Option<u32>,
    pub fps: FpsSetting,
    pub bitrate: BitrateMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_kbps: Option<u32>,
    pub codec: CodecSetting,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            // 1080p H.264 at 30 FPS is the reliable baseline for interactive
            // iPad control in Safari and Chrome. Higher modes remain explicit.
            profile: LatencyProfile::Balanced,
            resolution: ResolutionMode::P1080,
            custom_width: None,
            custom_height: None,
            fps: FpsSetting::Fixed(30),
            bitrate: BitrateMode::Auto,
            custom_kbps: None,
            codec: CodecSetting::H264,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EncoderPreference {
    /// Hardware first (VideoToolbox: Apple silicon media engine or Intel Quick Sync), software fallback.
    Auto,
    HardwareOnly,
    Software,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PenMode {
    /// Native macOS tablet pen with pressure, tilt and rotation (Photoshop, Blender, Krita…).
    Tablet,
    /// Pencil behaves like a mouse.
    Mouse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetworkSettings {
    pub mode: NetworkMode,
    pub https_port: u16,
    pub advertise_mdns: bool,
    /// Allow "Nearby computers → Connect" pairing requests without a code (confirmation on the Mac is still required).
    pub allow_nearby_pairing: bool,
    /// Vision-Pro relay used in Remote Access mode, normalized to `wss://host[:port]`.
    pub relay_url: Option<String>,
    /// Access token required by private relays.
    pub relay_token: Option<String>,
    /// STUN servers used only in Remote Access mode.
    pub stun_servers: Vec<String>,
    /// Detect tablets plugged in with a cable and stream to them over that cable.
    pub usb_direct: bool,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            mode: NetworkMode::LocalOnly,
            https_port: 47800,
            advertise_mdns: true,
            allow_nearby_pairing: true,
            relay_url: None,
            relay_token: None,
            stun_servers: vec![],
            usb_direct: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SecuritySettings {
    /// Every connection (not only pairing) must be confirmed on the Mac.
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
            // ⌘ ⌥ ⇧ ⌫ — registered as a global hotkey by the desktop app.
            emergency_shortcut: "Super+Alt+Shift+Backspace".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StreamingSettings {
    pub video: VideoSettings,
    pub encoder: EncoderPreference,
    /// Allow iPads to change quality settings during a session.
    pub allow_client_overrides: bool,
}

impl Default for StreamingSettings {
    fn default() -> Self {
        Self {
            video: VideoSettings::default(),
            encoder: EncoderPreference::Auto,
            allow_client_overrides: true,
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
            pen_mode: PenMode::Tablet,
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
    pub streaming: StreamingSettings,
    pub input: InputSettings,
    pub display: DisplaySettings,
    pub advanced: AdvancedSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: 2,
            computer_name: default_computer_name(),
            network: NetworkSettings::default(),
            security: SecuritySettings::default(),
            streaming: StreamingSettings::default(),
            input: InputSettings::default(),
            display: DisplaySettings::default(),
            advanced: AdvancedSettings::default(),
        }
    }
}

/// The name chosen in System Settings → General → About ("Bryan's MacBook Pro").
pub fn default_computer_name() -> String {
    if let Some(name) = crate::platform::system::computer_name() {
        return name;
    }
    let user = std::env::var("USER").ok().filter(|u| !u.is_empty());
    match user {
        Some(user) => {
            let mut chars = user.chars();
            let first = chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>())
                .unwrap_or_default();
            format!("{}{}'s Mac", first, chars.as_str())
        }
        None => gethostname::gethostname()
            .to_string_lossy()
            .trim_end_matches(".local")
            .to_string(),
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
                    let migrated = settings.migrate_streaming_defaults();
                    settings.sanitize();
                    if migrated {
                        let _ = settings.save();
                    }
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

    /// Preserve manual streaming choices, but replace only the exact former
    /// balanced/auto preset that shipped before the stable 1080p baseline.
    fn migrate_streaming_defaults(&mut self) -> bool {
        if self.schema >= 2 {
            return false;
        }
        let former_default = VideoSettings {
            profile: LatencyProfile::Balanced,
            resolution: ResolutionMode::Auto,
            custom_width: None,
            custom_height: None,
            fps: FpsSetting::Auto,
            bitrate: BitrateMode::Auto,
            custom_kbps: None,
            codec: CodecSetting::Auto,
        };
        if self.streaming.video == former_default {
            self.streaming.video = VideoSettings::default();
        }
        self.schema = 2;
        true
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
        self.network
            .stun_servers
            .retain(|s| s.starts_with("stun:") && s.len() < 256);
        self.network.stun_servers.truncate(4);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BitrateMode, CodecSetting, FpsSetting, LatencyProfile, ResolutionMode, Settings,
        VideoSettings,
    };

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
    fn usb_direct_is_on_by_default_and_for_older_settings_files() {
        assert!(Settings::default().network.usb_direct);
        let older: Settings =
            serde_json::from_str(r#"{"network":{"mode":"local-only","httpsPort":47800}}"#).unwrap();
        assert!(older.network.usb_direct);
    }

    #[test]
    fn fresh_install_uses_responsive_video_quality() {
        let video = Settings::default().streaming.video;
        assert_eq!(video.profile, LatencyProfile::Balanced);
        assert_eq!(video.resolution, ResolutionMode::P1080);
        assert_eq!(video.fps, FpsSetting::Fixed(30));
        assert_eq!(video.bitrate, BitrateMode::Auto);
        assert_eq!(video.codec, CodecSetting::H264);
    }

    #[test]
    fn migrates_only_the_former_balanced_auto_default() {
        let mut settings = Settings::default();
        settings.schema = 1;
        settings.streaming.video = VideoSettings {
            profile: LatencyProfile::Balanced,
            resolution: ResolutionMode::Auto,
            custom_width: None,
            custom_height: None,
            fps: FpsSetting::Auto,
            bitrate: BitrateMode::Auto,
            custom_kbps: None,
            codec: CodecSetting::Auto,
        };
        assert!(settings.migrate_streaming_defaults());
        assert_eq!(settings.streaming.video, VideoSettings::default());

        settings.schema = 1;
        settings.streaming.video.resolution = ResolutionMode::P1440;
        assert!(settings.migrate_streaming_defaults());
        assert_eq!(settings.streaming.video.resolution, ResolutionMode::P1440);
    }
}
