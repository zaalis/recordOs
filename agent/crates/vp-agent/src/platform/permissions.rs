//! macOS privacy permissions (TCC) the agent depends on.
//!
//! - Screen Recording: required by ScreenCaptureKit to stream the display.
//! - Accessibility: required to post mouse, keyboard and pen events to other applications.
//!
//! Both are granted by the user in System Settings → Privacy & Security; an application cannot
//! grant them to itself. The first request shows the system prompt.

use super::ffi;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    pub screen_recording: bool,
    pub accessibility: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    ScreenRecording,
    Accessibility,
    LocalNetwork,
}

impl Permission {
    pub fn parse(value: &str) -> Option<Permission> {
        match value {
            "screen-recording" => Some(Permission::ScreenRecording),
            "accessibility" => Some(Permission::Accessibility),
            "local-network" => Some(Permission::LocalNetwork),
            _ => None,
        }
    }

    fn pane(self) -> i32 {
        match self {
            Permission::ScreenRecording => 0,
            Permission::Accessibility => 1,
            Permission::LocalNetwork => 2,
        }
    }
}

pub fn screen_recording_allowed() -> bool {
    unsafe { ffi::vp_screen_capture_allowed() }
}

pub fn accessibility_allowed() -> bool {
    unsafe { ffi::vp_accessibility_allowed(false) }
}

pub fn status() -> PermissionStatus {
    PermissionStatus {
        screen_recording: screen_recording_allowed(),
        accessibility: accessibility_allowed(),
    }
}

/// Shows the system prompt when macOS still allows it, otherwise opens the matching pane of
/// System Settings so the user can switch Vision-Pro on.
pub fn request(permission: Permission) {
    let granted = match permission {
        Permission::ScreenRecording => unsafe { ffi::vp_screen_capture_request() },
        Permission::Accessibility => unsafe { ffi::vp_accessibility_allowed(true) },
        Permission::LocalNetwork => false,
    };
    if !granted {
        open_settings(permission);
    }
}

pub fn open_settings(permission: Permission) {
    unsafe { ffi::vp_open_privacy_settings(permission.pane()) }
}
