//! macOS integration: ScreenCaptureKit capture, VideoToolbox encoding, Quartz event injection,
//! Keychain-protected secrets, displays, privacy permissions and foreground-app detection.
//!
//! The Apple frameworks are reached through a small Objective-C bridge (`native/vp_macos.m`);
//! this module wraps it in safe Rust types.

#[cfg(not(target_os = "macos"))]
compile_error!("Vision-Pro for macOS builds only for macOS targets (aarch64-apple-darwin, x86_64-apple-darwin).");

mod ffi;

pub mod capture;
pub mod display;
pub mod engine;
pub mod inject;
pub mod keychain;
pub mod keymap;
pub mod permissions;
pub mod pipeline;
pub mod sw_encoder;
pub mod system;
pub mod vt_encoder;

use crate::media::MediaEngine;
use std::sync::Arc;

/// Media engine for this Mac.
pub fn media_engine() -> Arc<dyn MediaEngine> {
    Arc::new(engine::MacMediaEngine::new())
}
