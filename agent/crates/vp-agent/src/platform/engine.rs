//! Windows control engine: lists the displays and runs one input injector per session, mapped to
//! the display the session selected.

use super::displays::{self, Output};
use super::inject::{InjectCommand, InjectorHandle};
use crate::config::Settings;
use crate::control::{ControlEngine, ControlHandle, InputPolicy};
use crate::input::InputEvent;
use crate::protocol::{AgentCaps, DisplayInfo};
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;
use std::time::Duration;
use windows::Win32::Foundation::RECT;

pub struct WindowsControlEngine;

impl WindowsControlEngine {
    pub fn new() -> WindowsControlEngine {
        super::system::enable_dpi_awareness();
        WindowsControlEngine
    }
}

impl Default for WindowsControlEngine {
    fn default() -> Self {
        Self::new()
    }
}

pub fn display_infos(outputs: &[Output]) -> Vec<DisplayInfo> {
    outputs
        .iter()
        .map(|o| DisplayInfo {
            id: o.id.clone(),
            name: o.name.clone(),
            width: o.width(),
            height: o.height(),
            x: o.rect.left,
            y: o.rect.top,
            refresh_hz: o.refresh_hz,
            primary: o.primary,
        })
        .collect()
}

impl ControlEngine for WindowsControlEngine {
    fn capabilities(&self) -> AgentCaps {
        let displays = display_infos(&displays::outputs());
        AgentCaps {
            active_display_id: displays
                .iter()
                .find(|d| d.primary)
                .or(displays.first())
                .map(|d| d.id.clone())
                .unwrap_or_default(),
            displays,
            pen: true,
            touch: true,
        }
    }

    fn start(&self, settings: &Settings) -> anyhow::Result<Box<dyn ControlHandle>> {
        let outputs = displays::outputs();
        let output = settings
            .display
            .display_id
            .as_ref()
            .and_then(|id| outputs.iter().find(|o| &o.id == id))
            .or_else(|| outputs.iter().find(|o| o.primary))
            .or(outputs.first())
            .ok_or_else(|| anyhow::anyhow!("no display is attached to this PC"))?;
        let rect: Arc<RwLock<RECT>> = Arc::new(RwLock::new(output.rect));
        let injector = InjectorHandle::spawn(
            rect.clone(),
            InputPolicy::from_settings(settings),
            settings.display.prevent_sleep,
        );
        Ok(Box::new(WindowsControl {
            active: Mutex::new(output.id.clone()),
            rect,
            injector,
        }))
    }
}

struct WindowsControl {
    injector: InjectorHandle,
    rect: Arc<RwLock<RECT>>,
    active: Mutex<String>,
}

impl ControlHandle for WindowsControl {
    fn inject(&self, event: InputEvent) {
        let is_move = matches!(
            event,
            InputEvent::MoveAbs { .. } | InputEvent::MoveRel { .. }
        );
        match self.injector.sender.try_send(InjectCommand::Event(event)) {
            Ok(()) => {}
            Err(crossbeam_channel::TrySendError::Full(command)) if !is_move => {
                // Never drop presses and releases: wait briefly instead.
                let _ = self
                    .injector
                    .sender
                    .send_timeout(command, Duration::from_millis(20));
            }
            Err(_) => {}
        }
    }

    fn select_display(&self, id: &str) -> bool {
        let Some(output) = displays::outputs().into_iter().find(|o| o.id == id) else {
            return false;
        };
        *self.rect.write() = output.rect;
        *self.active.lock() = output.id;
        true
    }

    fn displays(&self) -> (Vec<DisplayInfo>, String) {
        (
            display_infos(&displays::outputs()),
            self.active.lock().clone(),
        )
    }
}
