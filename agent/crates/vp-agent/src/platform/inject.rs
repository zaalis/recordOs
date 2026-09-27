//! Input injection on a dedicated thread with Quartz Event Services: mouse (with real drags,
//! double-clicks and deltas), keyboard with explicit modifier flags, pixel-precise scrolling,
//! Unicode text, media keys, a native tablet pen (pressure, tilt, rotation, eraser) for the
//! Apple Pencil, and trackpad-style magnify for pinch.
//!
//! Every held key, button and pen contact is tracked and released when a session ends.
//! Injection requires the Accessibility permission (System Settings → Privacy & Security).

use super::display::Rect;
use super::ffi;
use super::keymap::{self, MacKey};
use crate::input::{
    GesturePhase, InputEvent, MouseButton, PenPhase, PenSample, TouchContact, TouchPhase,
};
use crossbeam_channel::{Receiver, Sender};
use parking_lot::RwLock;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct InputPolicy {
    pub keyboard: bool,
    pub mouse: bool,
    pub pen: bool,
    pub touch: bool,
    pub pen_as_mouse: bool,
}

pub enum InjectCommand {
    Event(InputEvent),
    Policy(InputPolicy),
    Stop,
}

/// Target display in global points (updated when the streamed display changes).
pub type SharedRect = Arc<RwLock<Rect>>;

pub struct InjectorHandle {
    pub sender: Sender<InjectCommand>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl InjectorHandle {
    pub fn spawn(display: SharedRect, policy: InputPolicy) -> InjectorHandle {
        let (sender, receiver) = crossbeam_channel::bounded::<InjectCommand>(4096);
        let thread = std::thread::Builder::new()
            .name("vp-input".into())
            .spawn(move || {
                if !super::permissions::accessibility_allowed() {
                    tracing::warn!("Accessibility permission is missing: macOS will ignore injected input until Vision-Pro is allowed in System Settings → Privacy & Security → Accessibility");
                }
                let mut injector = Injector::new(display, policy);
                injector.run(receiver);
            })
            .expect("input thread");
        InjectorHandle {
            sender,
            thread: Some(thread),
        }
    }
}

impl Drop for InjectorHandle {
    fn drop(&mut self) {
        let _ = self.sender.send(InjectCommand::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

const BUTTON_LEFT: i32 = 0;
const BUTTON_RIGHT: i32 = 1;

fn button_id(button: MouseButton) -> i32 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Right => 1,
        MouseButton::Middle => 2,
        MouseButton::Back => 3,
        MouseButton::Forward => 4,
    }
}

/// Wheel units from the iPad (120 = one notch, like a mouse wheel) → macOS pixels.
const PIXELS_PER_WHEEL_UNIT: f32 = 0.6;

/// Recognises double and triple clicks the way AppKit does: same button, within the system
/// double-click interval, and within a few points of the previous click.
struct ClickTracker {
    interval: Duration,
    last: Option<(i32, Instant, f64, f64, i32)>,
}

impl ClickTracker {
    fn new() -> ClickTracker {
        let seconds = unsafe { ffi::vp_double_click_interval() };
        ClickTracker {
            interval: Duration::from_secs_f64(seconds.clamp(0.15, 5.0)),
            last: None,
        }
    }

    fn press(&mut self, button: i32, x: f64, y: f64) -> i32 {
        let count = match self.last {
            Some((last_button, at, lx, ly, n))
                if last_button == button
                    && at.elapsed() <= self.interval
                    && (x - lx).abs() <= 4.0
                    && (y - ly).abs() <= 4.0 =>
            {
                n + 1
            }
            _ => 1,
        };
        self.last = Some((button, Instant::now(), x, y, count));
        count
    }
}

struct Injector {
    display: SharedRect,
    policy: InputPolicy,
    held_keys: HashSet<u16>,
    /// Held modifier flags keyed by the Vision-Pro key that pressed them.
    modifiers: HashMap<u16, u64>,
    caps_lock: bool,
    /// Held mouse buttons → click count used for the matching "up".
    held_buttons: BTreeMap<i32, i32>,
    clicks: ClickTracker,
    pen_in_range: bool,
    pen_in_contact: bool,
    pen_eraser: bool,
    pen_button: i32,
    pen_last: Option<PenSample>,
    touch_primary: Option<u8>,
    wheel_accum: (f32, f32),
    rel_accum: (f64, f64),
    magnifying: bool,
    last_point: (f64, f64),
}

impl Injector {
    fn new(display: SharedRect, policy: InputPolicy) -> Injector {
        Injector {
            display,
            policy,
            held_keys: HashSet::new(),
            modifiers: HashMap::new(),
            caps_lock: false,
            held_buttons: BTreeMap::new(),
            clicks: ClickTracker::new(),
            pen_in_range: false,
            pen_in_contact: false,
            pen_eraser: false,
            pen_button: BUTTON_LEFT,
            pen_last: None,
            touch_primary: None,
            wheel_accum: (0.0, 0.0),
            rel_accum: (0.0, 0.0),
            magnifying: false,
            last_point: (0.0, 0.0),
        }
    }

    fn run(&mut self, receiver: Receiver<InjectCommand>) {
        while let Ok(command) = receiver.recv() {
            match command {
                InjectCommand::Event(event) => self.handle(event),
                InjectCommand::Policy(policy) => self.policy = policy,
                InjectCommand::Stop => break,
            }
        }
        self.release_all();
    }

    fn flags(&self) -> u64 {
        let mut flags = self.modifiers.values().fold(0, |acc, flag| acc | flag);
        if self.caps_lock {
            flags |= keymap::FLAG_ALPHA_SHIFT;
        }
        flags
    }

    /// Lowest held button, so pointer moves become the matching drag events.
    fn held_button(&self) -> i32 {
        if self.pen_in_contact {
            return self.pen_button;
        }
        self.held_buttons.keys().next().copied().unwrap_or(-1)
    }

    /// Normalised (0..1) position on the streamed display → global points.
    fn to_screen(&self, x: f32, y: f32) -> (f64, f64) {
        let rect = *self.display.read();
        let px = rect.x + (x as f64 * rect.width).clamp(0.0, (rect.width - 0.5).max(0.0));
        let py = rect.y + (y as f64 * rect.height).clamp(0.0, (rect.height - 0.5).max(0.0));
        (px, py)
    }

    fn move_to(&mut self, point: (f64, f64)) {
        self.last_point = point;
        unsafe { ffi::vp_mouse_move(point.0, point.1, self.held_button(), self.flags()) }
    }

    fn cursor(&self) -> (f64, f64) {
        let (mut x, mut y) = self.last_point;
        unsafe {
            ffi::vp_cursor_location(&mut x, &mut y);
        }
        (x, y)
    }

    fn handle(&mut self, event: InputEvent) {
        match event {
            InputEvent::MoveAbs { x, y } if self.policy.mouse => {
                let point = self.to_screen(x, y);
                self.move_to(point);
            }
            InputEvent::MoveRel { dx, dy } if self.policy.mouse => {
                let rect = *self.display.read();
                self.rel_accum.0 += dx as f64 * rect.width;
                self.rel_accum.1 += dy as f64 * rect.height;
                // Quartz coordinates are fractional points: move in quarter-point steps.
                let step = |v: f64| (v * 4.0).trunc() / 4.0;
                let (sx, sy) = (step(self.rel_accum.0), step(self.rel_accum.1));
                if sx == 0.0 && sy == 0.0 {
                    return;
                }
                self.rel_accum.0 -= sx;
                self.rel_accum.1 -= sy;
                let (cx, cy) = self.cursor();
                let target = (
                    (cx + sx).clamp(rect.x, (rect.right() - 0.5).max(rect.x)),
                    (cy + sy).clamp(rect.y, (rect.bottom() - 0.5).max(rect.y)),
                );
                self.move_to(target);
            }
            InputEvent::Button { button, down, at } if self.policy.mouse => {
                if let Some((x, y)) = at {
                    let point = self.to_screen(x, y);
                    self.move_to(point);
                }
                let id = button_id(button);
                let point = if at.is_some() {
                    self.last_point
                } else {
                    self.cursor()
                };
                let clicks = if down {
                    let clicks = self.clicks.press(id, point.0, point.1);
                    self.held_buttons.insert(id, clicks);
                    clicks
                } else {
                    // Never send an "up" for a button this session did not press.
                    let Some(clicks) = self.held_buttons.remove(&id) else {
                        return;
                    };
                    clicks
                };
                self.last_point = point;
                unsafe { ffi::vp_mouse_button(point.0, point.1, id, down, clicks, self.flags()) }
            }
            InputEvent::Wheel {
                dx,
                dy,
                precise,
                at,
            } if self.policy.mouse => {
                if let Some((x, y)) = at {
                    let point = self.to_screen(x, y);
                    self.move_to(point);
                }
                // Positive dy scrolls toward the end of the document; macOS expresses that as a
                // negative wheel value (positive = toward the top / left).
                let (vertical, horizontal) = if precise {
                    self.wheel_accum.0 += -dy * PIXELS_PER_WHEEL_UNIT;
                    self.wheel_accum.1 += -dx * PIXELS_PER_WHEEL_UNIT;
                    (self.wheel_accum.0.trunc(), self.wheel_accum.1.trunc())
                } else {
                    self.wheel_accum.0 += -dy / 120.0;
                    self.wheel_accum.1 += -dx / 120.0;
                    (self.wheel_accum.0.trunc(), self.wheel_accum.1.trunc())
                };
                self.wheel_accum.0 -= vertical;
                self.wheel_accum.1 -= horizontal;
                if vertical != 0.0 || horizontal != 0.0 {
                    unsafe {
                        ffi::vp_scroll(vertical as i32, horizontal as i32, !precise, self.flags())
                    }
                }
            }
            InputEvent::Magnify { phase, delta, at } if self.policy.mouse => {
                if let Some((x, y)) = at {
                    let point = self.to_screen(x, y);
                    if phase == GesturePhase::Begin {
                        // Gestures are delivered to the window under the pointer.
                        self.move_to(point);
                    }
                    self.last_point = point;
                }
                let point = self.last_point;
                match phase {
                    GesturePhase::Begin => {
                        if self.magnifying {
                            unsafe { ffi::vp_magnify(2, 0.0, point.0, point.1) }
                        }
                        self.magnifying = true;
                        unsafe { ffi::vp_magnify(0, 0.0, point.0, point.1) }
                    }
                    GesturePhase::Change if self.magnifying => unsafe {
                        ffi::vp_magnify(1, delta as f64, point.0, point.1)
                    },
                    GesturePhase::End if self.magnifying => {
                        self.magnifying = false;
                        unsafe { ffi::vp_magnify(2, 0.0, point.0, point.1) }
                    }
                    _ => {}
                }
            }
            InputEvent::Key { vk, down } if self.policy.keyboard => {
                if down {
                    self.held_keys.insert(vk);
                } else if !self.held_keys.remove(&vk) {
                    return;
                }
                self.key(vk, down);
            }
            InputEvent::Combo(keys) if self.policy.keyboard => {
                let pressed: Vec<u16> = keys
                    .iter()
                    .copied()
                    .filter(|vk| !self.held_keys.contains(vk))
                    .collect();
                for vk in &pressed {
                    self.key(*vk, true);
                }
                for vk in pressed.iter().rev() {
                    self.key(*vk, false);
                }
            }
            InputEvent::Text(units) if self.policy.keyboard => self.text(&units),
            InputEvent::Pen(sample) if self.policy.pen => {
                if self.policy.pen_as_mouse {
                    self.pen_as_mouse(sample);
                } else {
                    self.pen(sample);
                }
            }
            InputEvent::Touch(contacts) if self.policy.touch => self.touch(contacts),
            InputEvent::ReleaseAll => self.release_all(),
            _ => {}
        }
    }

    fn key(&mut self, vk: u16, down: bool) {
        let Some(mac) = keymap::translate(vk) else {
            return;
        };
        match mac {
            MacKey::Modifier { code, flag } => {
                if down {
                    self.modifiers.insert(vk, keymap::modifier_mask(flag));
                } else {
                    self.modifiers.remove(&vk);
                }
                unsafe { ffi::vp_key(code, down, self.flags(), true) }
            }
            MacKey::Key { code, flags } => unsafe {
                ffi::vp_key(code, down, self.flags() | flags, false)
            },
            MacKey::CapsLock => {
                if down {
                    self.caps_lock = !self.caps_lock;
                }
                unsafe { ffi::vp_key(0x39, down, self.flags(), true) }
            }
            MacKey::Media(key) => unsafe { ffi::vp_media_key(key, down) },
        }
    }

    fn text(&mut self, units: &[u16]) {
        let mut run: Vec<u16> = Vec::with_capacity(units.len());
        let flush = |run: &mut Vec<u16>| {
            if !run.is_empty() {
                unsafe { ffi::vp_text(run.as_ptr(), run.len()) }
                run.clear();
            }
        };
        for &unit in units {
            if unit == 0x0a {
                flush(&mut run);
                unsafe {
                    ffi::vp_key(keymap::KEY_RETURN, true, 0, false);
                    ffi::vp_key(keymap::KEY_RETURN, false, 0, false);
                }
            } else {
                run.push(unit);
            }
        }
        flush(&mut run);
    }

    fn pen_as_mouse(&mut self, sample: PenSample) {
        let at = Some((sample.x, sample.y));
        let button = if sample.barrel || sample.eraser {
            MouseButton::Right
        } else {
            MouseButton::Left
        };
        match sample.phase {
            PenPhase::Hover | PenPhase::Move => self.handle(InputEvent::MoveAbs {
                x: sample.x,
                y: sample.y,
            }),
            PenPhase::Down => self.handle(InputEvent::Button {
                button,
                down: true,
                at,
            }),
            PenPhase::Up | PenPhase::Cancel => {
                for button in [MouseButton::Left, MouseButton::Right] {
                    self.handle(InputEvent::Button {
                        button,
                        down: false,
                        at,
                    });
                }
            }
            PenPhase::Leave => {}
        }
    }

    /// Announces the stylus to macOS (pen or eraser end) before it is used.
    fn pen_proximity(&mut self, eraser: bool) {
        if self.pen_in_range && self.pen_eraser == eraser {
            return;
        }
        if self.pen_in_range {
            unsafe { ffi::vp_tablet_proximity(false, self.pen_eraser) }
        }
        unsafe { ffi::vp_tablet_proximity(true, eraser) }
        self.pen_in_range = true;
        self.pen_eraser = eraser;
    }

    fn pen(&mut self, sample: PenSample) {
        let point = self.to_screen(sample.x, sample.y);
        // Tablet tilt is normalised to -1…1; the iPad sends degrees.
        let tilt_x = (sample.tilt_x as f64 / 90.0).clamp(-1.0, 1.0);
        let tilt_y = (sample.tilt_y as f64 / 90.0).clamp(-1.0, 1.0);
        let rotation = sample.twist as f64;
        let point_event = |phase: i32, button: i32, pressure: f32, flags: u64| unsafe {
            ffi::vp_tablet_point(
                point.0,
                point.1,
                phase,
                button,
                pressure as f64,
                tilt_x,
                tilt_y,
                rotation,
                flags,
            )
        };
        match sample.phase {
            PenPhase::Hover => {
                self.pen_proximity(sample.eraser);
                point_event(0, BUTTON_LEFT, 0.0, self.flags());
            }
            PenPhase::Down => {
                self.pen_proximity(sample.eraser);
                if self.pen_in_contact {
                    point_event(3, self.pen_button, 0.0, self.flags());
                }
                // The Pencil's squeeze/barrel gesture acts as the secondary (right) button.
                self.pen_button = if sample.barrel {
                    BUTTON_RIGHT
                } else {
                    BUTTON_LEFT
                };
                self.pen_in_contact = true;
                point_event(1, self.pen_button, sample.pressure, self.flags());
            }
            PenPhase::Move => {
                self.pen_proximity(sample.eraser);
                if self.pen_in_contact {
                    point_event(2, self.pen_button, sample.pressure, self.flags());
                } else {
                    point_event(0, BUTTON_LEFT, 0.0, self.flags());
                }
            }
            PenPhase::Up => {
                if self.pen_in_contact {
                    self.pen_in_contact = false;
                    point_event(3, self.pen_button, 0.0, self.flags());
                }
            }
            PenPhase::Leave | PenPhase::Cancel => {
                if self.pen_in_contact {
                    self.pen_in_contact = false;
                    point_event(3, self.pen_button, 0.0, self.flags());
                }
                if self.pen_in_range {
                    self.pen_in_range = false;
                    unsafe { ffi::vp_tablet_proximity(false, self.pen_eraser) }
                }
            }
        }
        self.last_point = point;
        self.pen_last = Some(sample);
    }

    /// macOS has no touchscreen injection: the first finger drives the pointer like a mouse.
    fn touch(&mut self, contacts: Vec<TouchContact>) {
        for contact in contacts {
            let at = Some((contact.x, contact.y));
            match contact.phase {
                TouchPhase::Down if self.touch_primary.is_none() => {
                    self.touch_primary = Some(contact.id);
                    self.handle(InputEvent::Button {
                        button: MouseButton::Left,
                        down: true,
                        at,
                    });
                }
                TouchPhase::Move if self.touch_primary == Some(contact.id) => {
                    self.handle(InputEvent::MoveAbs {
                        x: contact.x,
                        y: contact.y,
                    });
                }
                TouchPhase::Up | TouchPhase::Cancel if self.touch_primary == Some(contact.id) => {
                    self.touch_primary = None;
                    self.handle(InputEvent::Button {
                        button: MouseButton::Left,
                        down: false,
                        at,
                    });
                }
                _ => {}
            }
        }
    }

    fn release_all(&mut self) {
        if self.magnifying {
            self.magnifying = false;
            let point = self.last_point;
            unsafe { ffi::vp_magnify(2, 0.0, point.0, point.1) }
        }
        if let Some(last) = self.pen_last {
            if self.pen_in_contact || self.pen_in_range {
                self.pen(PenSample {
                    phase: PenPhase::Cancel,
                    pressure: 0.0,
                    ..last
                });
            }
        }
        let point = self.cursor();
        let buttons: Vec<(i32, i32)> = std::mem::take(&mut self.held_buttons).into_iter().collect();
        for (id, clicks) in buttons {
            unsafe { ffi::vp_mouse_button(point.0, point.1, id, false, clicks, self.flags()) }
        }
        self.touch_primary = None;
        // Regular keys first, then modifiers, so no stray shortcut fires on the way out.
        let keys: Vec<u16> = self.held_keys.drain().collect();
        let (modifiers, regular): (Vec<u16>, Vec<u16>) = keys
            .into_iter()
            .partition(|vk| matches!(keymap::translate(*vk), Some(MacKey::Modifier { .. })));
        for vk in regular.into_iter().chain(modifiers) {
            self.key(vk, false);
        }
        self.modifiers.clear();
        self.wheel_accum = (0.0, 0.0);
        self.rel_accum = (0.0, 0.0);
    }
}
