//! Vision-Pro macOS agent.
//!
//! Security model, protocol and architecture are documented in `docs/` at the repository root.

pub mod agent;
pub mod config;
pub mod crypto;
pub mod discovery;
pub mod events;
pub mod identity;
pub mod input;
pub mod media;
pub mod net;
pub mod pairing;
pub mod paths;
pub mod platform;
pub mod protocol;
pub mod relay;
pub mod security;
pub mod server;
pub mod session;
pub mod signaling;
pub mod store;
pub mod tls;
pub mod usb;
pub mod util;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PRODUCT_NAME: &str = "Vision-Pro";
