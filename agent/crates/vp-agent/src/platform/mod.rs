//! Windows integration: DPAPI, display enumeration, input injection, app detection, USB links.

pub mod displays;
pub mod dpapi;
pub mod engine;
pub mod inject;
pub mod system;
pub mod usb;

use crate::control::ControlEngine;
use std::sync::Arc;

/// Control engine for this system.
pub fn control_engine() -> Arc<dyn ControlEngine> {
    Arc::new(engine::WindowsControlEngine::new())
}
