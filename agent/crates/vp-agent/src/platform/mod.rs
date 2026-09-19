//! Windows integration: DPAPI, screen capture, hardware encoding, input injection, app detection.

pub mod capture;
pub mod convert;
pub mod dpapi;
pub mod engine;
pub mod gpu;
pub mod inject;
pub mod mf_encoder;
pub mod pipeline;
pub mod sw_encoder;
pub mod system;
pub mod usb;

use crate::media::MediaEngine;
use std::sync::Arc;

/// Media engine for this system.
pub fn media_engine() -> Arc<dyn MediaEngine> {
    Arc::new(engine::WindowsMediaEngine::new())
}
