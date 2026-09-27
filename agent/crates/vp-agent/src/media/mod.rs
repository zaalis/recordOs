//! Media: codec negotiation, capture → encode → WebRTC pipeline, input dispatch.

pub mod codec;
pub mod engine;
pub mod rtc;

use crate::config::Settings;
use crate::protocol::{AgentCaps, AppliedVideo, RtcOffer};
use crate::session::SessionInfo;
use parking_lot::RwLock;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub struct MediaRequest {
    pub offer: RtcOffer,
    pub session_id: String,
    pub device_name: String,
    /// Present when the iPad reached this Mac through the relay.
    pub relay: Option<crate::relay::RelayMediaLink>,
    /// Only address offered as ICE candidate (the USB cable the session arrived on), so video
    /// and input stay on the cable.
    pub bind: Option<std::net::Ipv4Addr>,
    /// Addresses never offered on a local-network session (cable addresses).
    pub exclude: Vec<std::net::Ipv4Addr>,
}

#[derive(Clone)]
pub struct MediaContext {
    /// Cleared instantly when the device is switched to view-only.
    pub control_allowed: Arc<AtomicBool>,
    pub info: Arc<RwLock<SessionInfo>>,
    pub settings: Settings,
    pub on_change: Arc<dyn Fn() + Send + Sync>,
}

pub struct MediaStarted {
    pub answer_sdp: String,
    pub applied: AppliedVideo,
    pub handle: Box<dyn MediaHandle>,
}

/// Dropping the handle stops capture, encoding and the peer connection.
pub trait MediaHandle: Send {
    fn stop(&mut self);
}

pub trait MediaEngine: Send + Sync {
    fn capabilities(&self) -> AgentCaps;
    fn start(&self, request: MediaRequest, context: MediaContext) -> anyhow::Result<MediaStarted>;
}
