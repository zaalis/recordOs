//! Windows media engine (capture → encode → WebRTC). Filled in by the platform pipeline.

use super::{MediaContext, MediaEngine, MediaRequest, MediaStarted};
use crate::protocol::AgentCaps;

/// Engine used when streaming is unavailable (tests, unsupported systems).
pub struct NullMediaEngine;

impl MediaEngine for NullMediaEngine {
    fn capabilities(&self) -> AgentCaps {
        AgentCaps {
            encoders: vec![],
            displays: vec![],
            active_display_id: String::new(),
            pen: false,
            touch: false,
            max_fps: 60,
        }
    }

    fn start(
        &self,
        _request: MediaRequest,
        _context: MediaContext,
    ) -> anyhow::Result<MediaStarted> {
        anyhow::bail!("streaming is not available on this system")
    }
}
