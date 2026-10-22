//! Remote control of this PC: what a session's input is mapped to and how it reaches Windows.
//!
//! Nothing is captured or streamed from this PC. The iPad is a drawing tablet / trackpad: its
//! input arrives inside the session's end-to-end encrypted channel (see `signaling`), is decoded
//! against a fixed schema (`input`) and handed to the platform injector.

use crate::config::{PenMode, Settings};
use crate::input::{InputCategory, InputEvent, SequenceFilter};
use crate::protocol::{AgentCaps, DisplayInfo};
use std::time::{Duration, Instant};

/// Input events accepted per second and session; anything above is dropped.
pub const MAX_INPUT_PER_SEC: u32 = 3000;
/// A session sending this many undecodable messages is ended.
pub const MAX_INVALID_INPUT: u32 = 500;

pub trait ControlEngine: Send + Sync {
    fn capabilities(&self) -> AgentCaps;
    /// Starts injecting for one session, mapped to the configured display.
    fn start(&self, settings: &Settings) -> anyhow::Result<Box<dyn ControlHandle>>;
}

/// Dropping the handle releases every key, button, pen and touch contact still held.
pub trait ControlHandle: Send + Sync {
    /// Queues one event. Presses and releases wait briefly when the queue is full; moves are dropped.
    fn inject(&self, event: InputEvent);
    /// Maps input to another display. Returns false for an unknown display.
    fn select_display(&self, id: &str) -> bool;
    /// Displays of this PC and the one input is mapped to.
    fn displays(&self) -> (Vec<DisplayInfo>, String);
}

#[derive(Debug, Clone, Copy)]
pub struct InputPolicy {
    pub keyboard: bool,
    pub mouse: bool,
    pub pen: bool,
    pub touch: bool,
    pub pen_as_mouse: bool,
}

impl InputPolicy {
    pub fn from_settings(settings: &Settings) -> InputPolicy {
        InputPolicy {
            keyboard: settings.input.allow_keyboard,
            mouse: settings.input.allow_mouse,
            pen: settings.input.allow_pen,
            touch: settings.input.allow_touch,
            pen_as_mouse: settings.input.pen_mode == PenMode::Mouse,
        }
    }

    pub fn allows(&self, event: &InputEvent) -> bool {
        match event.category() {
            InputCategory::Mouse => self.mouse,
            InputCategory::Keyboard => self.keyboard,
            InputCategory::Pen => self.pen,
            InputCategory::Touch => self.touch,
            InputCategory::System => true,
        }
    }
}

/// Per-session input admission: rate limit, realtime ordering and malformed-message accounting.
pub struct InputGate {
    window: (Instant, u32),
    realtime: SequenceFilter,
    pub accepted: u32,
    pub invalid: u32,
}

impl Default for InputGate {
    fn default() -> Self {
        InputGate {
            window: (Instant::now(), 0),
            realtime: SequenceFilter::default(),
            accepted: 0,
            invalid: 0,
        }
    }
}

impl InputGate {
    /// Decodes one input message. `realtime` messages (hover moves) are latest-wins: an older
    /// sequence number than the last one accepted is dropped.
    pub fn admit(
        &mut self,
        data: &[u8],
        realtime: bool,
        policy: &InputPolicy,
    ) -> Option<InputEvent> {
        let now = Instant::now();
        if now.duration_since(self.window.0) >= Duration::from_secs(1) {
            self.window = (now, 0);
        }
        self.window.1 += 1;
        if self.window.1 > MAX_INPUT_PER_SEC {
            return None;
        }
        let Some((seq, event)) = crate::input::decode(data) else {
            self.invalid += 1;
            return None;
        };
        if realtime && !self.realtime.accept(seq) {
            return None;
        }
        if !policy.allows(&event) {
            return None;
        }
        self.accepted += 1;
        Some(event)
    }

    pub fn exhausted(&self) -> bool {
        self.invalid > MAX_INVALID_INPUT
    }
}

/// Engine for systems without input injection (tests): accepts sessions, discards input.
pub struct NullControlEngine;

struct NullControl;

impl ControlHandle for NullControl {
    fn inject(&self, _event: InputEvent) {}
    fn select_display(&self, _id: &str) -> bool {
        false
    }
    fn displays(&self) -> (Vec<DisplayInfo>, String) {
        (vec![], String::new())
    }
}

impl ControlEngine for NullControlEngine {
    fn capabilities(&self) -> AgentCaps {
        AgentCaps {
            displays: vec![],
            active_display_id: String::new(),
            pen: false,
            touch: false,
        }
    }

    fn start(&self, _settings: &Settings) -> anyhow::Result<Box<dyn ControlHandle>> {
        Ok(Box::new(NullControl))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> InputPolicy {
        InputPolicy::from_settings(&Settings::default())
    }

    fn move_abs(seq: u16) -> Vec<u8> {
        let mut data = vec![crate::input::KIND_MOVE_ABS, 0];
        data.extend_from_slice(&seq.to_le_bytes());
        data.extend_from_slice(&0.5f32.to_le_bytes());
        data.extend_from_slice(&0.25f32.to_le_bytes());
        data
    }

    #[test]
    fn realtime_input_is_latest_wins_and_garbage_is_counted() {
        let mut gate = InputGate::default();
        assert!(gate.admit(&move_abs(5), true, &policy()).is_some());
        assert!(gate.admit(&move_abs(4), true, &policy()).is_none());
        assert!(gate.admit(&move_abs(4), false, &policy()).is_some());
        assert!(gate.admit(&[0xff, 0, 0, 0], false, &policy()).is_none());
        assert_eq!((gate.accepted, gate.invalid), (2, 1));
    }

    #[test]
    fn disabled_categories_are_dropped() {
        let mut settings = Settings::default();
        settings.input.allow_mouse = false;
        let mut gate = InputGate::default();
        assert!(gate
            .admit(&move_abs(1), false, &InputPolicy::from_settings(&settings))
            .is_none());
        assert_eq!(gate.invalid, 0);
    }
}
