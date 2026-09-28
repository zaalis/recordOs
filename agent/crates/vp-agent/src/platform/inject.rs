//! Input injection on a dedicated thread: SendInput for mouse/keyboard, synthetic pointer devices
//! for Windows Ink pen (pressure, tilt, rotation, eraser, barrel) and multi-touch.
//! Every held key, button, pen and touch contact is tracked and released when a session ends.

use crate::input::{
    InputEvent, MouseButton, PenPhase, PenSample, TouchContact, TouchPhase, EXTENDED,
};
use crossbeam_channel::{Receiver, Sender};
use parking_lot::RwLock;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::Controls::{
    CreateSyntheticPointerDevice, DestroySyntheticPointerDevice, HSYNTHETICPOINTERDEVICE,
    POINTER_FEEDBACK_DEFAULT, POINTER_FEEDBACK_INDIRECT, POINTER_TYPE_INFO, POINTER_TYPE_INFO_0,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN,
    MOUSEEVENTF_XUP, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY,
};
use windows::Win32::UI::Input::Pointer::{
    InjectSyntheticPointerInput, POINTER_FLAGS, POINTER_FLAG_CANCELED, POINTER_FLAG_DOWN,
    POINTER_FLAG_FIRSTBUTTON, POINTER_FLAG_INCONTACT, POINTER_FLAG_INRANGE, POINTER_FLAG_PRIMARY,
    POINTER_FLAG_UP, POINTER_FLAG_UPDATE, POINTER_INFO, POINTER_PEN_INFO, POINTER_TOUCH_INFO,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, PEN_FLAG_BARREL, PEN_FLAG_ERASER, PEN_FLAG_INVERTED, PEN_MASK_PRESSURE,
    PEN_MASK_ROTATION, PEN_MASK_TILT_X, PEN_MASK_TILT_Y, PT_PEN, PT_TOUCH, TOUCH_MASK_CONTACTAREA,
    TOUCH_MASK_PRESSURE, XBUTTON1, XBUTTON2,
};

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

/// Target display rectangle in virtual-desktop physical pixels (updated when the display changes).
pub type SharedRect = Arc<RwLock<RECT>>;

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
                super::system::prioritize_current_thread();
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

struct Injector {
    display: SharedRect,
    policy: InputPolicy,
    held_keys: HashSet<u16>,
    held_buttons: HashSet<u8>,
    pen_device: Option<HSYNTHETICPOINTERDEVICE>,
    pen_in_range: bool,
    pen_in_contact: bool,
    pen_last: Option<PenSample>,
    touch_device: Option<HSYNTHETICPOINTERDEVICE>,
    touches: BTreeMap<u8, TouchContact>,
    wheel_accum: (f32, f32),
    rel_accum: (f64, f64),
}

fn mouse_input(dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn key_input(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) {
    if inputs.is_empty() {
        return;
    }
    unsafe {
        SendInput(inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

fn is_extended_vk(vk: u16) -> bool {
    vk & EXTENDED != 0
        || matches!(
            vk & 0xff,
            0x21..=0x28 | 0x2d | 0x2e | 0x5b | 0x5c | 0x5d | 0x6f | 0xa3 | 0xa5 | 0x90 | 0x2c
        )
}

impl Injector {
    fn new(display: SharedRect, policy: InputPolicy) -> Injector {
        Injector {
            display,
            policy,
            held_keys: HashSet::new(),
            held_buttons: HashSet::new(),
            pen_device: None,
            pen_in_range: false,
            pen_in_contact: false,
            pen_last: None,
            touch_device: None,
            touches: BTreeMap::new(),
            wheel_accum: (0.0, 0.0),
            rel_accum: (0.0, 0.0),
        }
    }

    fn run(&mut self, receiver: Receiver<InjectCommand>) {
        while let Ok(command) = receiver.recv() {
            match command {
                InjectCommand::Event(event) => self.handle(event),
                InjectCommand::Policy(policy) => {
                    self.policy = policy;
                }
                InjectCommand::Stop => break,
            }
        }
        self.release_all();
        unsafe {
            if let Some(device) = self.pen_device.take() {
                DestroySyntheticPointerDevice(device);
            }
            if let Some(device) = self.touch_device.take() {
                DestroySyntheticPointerDevice(device);
            }
        }
    }

    fn to_screen(&self, x: f32, y: f32) -> POINT {
        let rect = *self.display.read();
        let w = (rect.right - rect.left).max(1) as f32;
        let h = (rect.bottom - rect.top).max(1) as f32;
        POINT {
            x: rect.left + ((x * (w - 1.0)).round() as i32).clamp(0, w as i32 - 1),
            y: rect.top + ((y * (h - 1.0)).round() as i32).clamp(0, h as i32 - 1),
        }
    }

    fn absolute_move(&self, point: POINT) -> INPUT {
        let desktop = super::gpu::virtual_desktop();
        let vw = (desktop.right - desktop.left).max(2) as i64;
        let vh = (desktop.bottom - desktop.top).max(2) as i64;
        let nx = ((point.x - desktop.left) as i64 * 65535 / (vw - 1)) as i32;
        let ny = ((point.y - desktop.top) as i64 * 65535 / (vh - 1)) as i32;
        mouse_input(
            nx,
            ny,
            0,
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        )
    }

    fn handle(&mut self, event: InputEvent) {
        match event {
            InputEvent::MoveAbs { x, y } if self.policy.mouse => {
                let point = self.to_screen(x, y);
                send(&[self.absolute_move(point)]);
            }
            InputEvent::MoveRel { dx, dy } if self.policy.mouse => {
                let rect = *self.display.read();
                self.rel_accum.0 += dx as f64 * (rect.right - rect.left) as f64;
                self.rel_accum.1 += dy as f64 * (rect.bottom - rect.top) as f64;
                let (ix, iy) = (
                    self.rel_accum.0.trunc() as i32,
                    self.rel_accum.1.trunc() as i32,
                );
                if ix == 0 && iy == 0 {
                    return;
                }
                self.rel_accum.0 -= ix as f64;
                self.rel_accum.1 -= iy as f64;
                let mut current = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut current);
                }
                let target = POINT {
                    x: (current.x + ix).clamp(rect.left, rect.right - 1),
                    y: (current.y + iy).clamp(rect.top, rect.bottom - 1),
                };
                send(&[self.absolute_move(target)]);
            }
            InputEvent::Button { button, down, at } if self.policy.mouse => {
                let mut inputs = Vec::with_capacity(2);
                if let Some((x, y)) = at {
                    inputs.push(self.absolute_move(self.to_screen(x, y)));
                }
                let (flags, data) = match (button, down) {
                    (MouseButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
                    (MouseButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
                    (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
                    (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
                    (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
                    (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
                    (MouseButton::Back, true) => (MOUSEEVENTF_XDOWN, XBUTTON1 as u32),
                    (MouseButton::Back, false) => (MOUSEEVENTF_XUP, XBUTTON1 as u32),
                    (MouseButton::Forward, true) => (MOUSEEVENTF_XDOWN, XBUTTON2 as u32),
                    (MouseButton::Forward, false) => (MOUSEEVENTF_XUP, XBUTTON2 as u32),
                };
                let id = button as u8;
                if down {
                    self.held_buttons.insert(id);
                } else if !self.held_buttons.remove(&id) {
                    // Never send an "up" for a button this session did not press.
                    send(&inputs);
                    return;
                }
                inputs.push(mouse_input(0, 0, data, flags));
                send(&inputs);
            }
            InputEvent::Wheel { dx, dy, at } if self.policy.mouse => {
                let mut inputs = Vec::with_capacity(3);
                if let Some((x, y)) = at {
                    inputs.push(self.absolute_move(self.to_screen(x, y)));
                }
                self.wheel_accum.0 += dx;
                self.wheel_accum.1 += dy;
                let vertical = self.wheel_accum.1.trunc() as i32;
                let horizontal = self.wheel_accum.0.trunc() as i32;
                if vertical != 0 {
                    self.wheel_accum.1 -= vertical as f32;
                    // Positive dy scrolls content down, which is a negative wheel delta on Windows.
                    inputs.push(mouse_input(0, 0, (-vertical) as u32, MOUSEEVENTF_WHEEL));
                }
                if horizontal != 0 {
                    self.wheel_accum.0 -= horizontal as f32;
                    inputs.push(mouse_input(0, 0, horizontal as u32, MOUSEEVENTF_HWHEEL));
                }
                send(&inputs);
            }
            InputEvent::Key { vk, down } if self.policy.keyboard => {
                if down {
                    self.held_keys.insert(vk);
                } else if !self.held_keys.remove(&vk) {
                    return;
                }
                send(&[self.key(vk, down)]);
            }
            InputEvent::Combo(keys) if self.policy.keyboard => {
                let mut inputs = vec![];
                let pressed: Vec<u16> = keys
                    .iter()
                    .copied()
                    .filter(|vk| !self.held_keys.contains(vk))
                    .collect();
                for vk in &pressed {
                    inputs.push(self.key(*vk, true));
                }
                for vk in pressed.iter().rev() {
                    inputs.push(self.key(*vk, false));
                }
                send(&inputs);
            }
            InputEvent::Text(units) if self.policy.keyboard => {
                let mut inputs = Vec::with_capacity(units.len() * 2);
                for unit in units {
                    if unit == 0x0a {
                        inputs.push(self.key(0x0d, true));
                        inputs.push(self.key(0x0d, false));
                    } else {
                        inputs.push(key_input(0, unit, KEYEVENTF_UNICODE));
                        inputs.push(key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                    }
                }
                send(&inputs);
            }
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

    fn key(&self, vk: u16, down: bool) -> INPUT {
        let base = vk & 0xff;
        let scan = unsafe { MapVirtualKeyW(base as u32, MAPVK_VK_TO_VSC) } as u16;
        let mut flags = KEYBD_EVENT_FLAGS(0);
        if is_extended_vk(vk) {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        key_input(base, scan, flags)
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
                self.handle(InputEvent::Button {
                    button: MouseButton::Left,
                    down: false,
                    at,
                });
                self.handle(InputEvent::Button {
                    button: MouseButton::Right,
                    down: false,
                    at,
                });
            }
            PenPhase::Leave => {}
        }
    }

    fn pen(&mut self, sample: PenSample) {
        if self.pen_device.is_none() {
            match unsafe { CreateSyntheticPointerDevice(PT_PEN, 1, POINTER_FEEDBACK_INDIRECT) } {
                Ok(device) => self.pen_device = Some(device),
                Err(error) => {
                    tracing::warn!(%error, "pen injection unavailable; falling back to mouse");
                    self.policy.pen_as_mouse = true;
                    self.pen_as_mouse(sample);
                    return;
                }
            }
        }
        // Hover samples travel on the unordered real-time channel and are only sent while the
        // pencil is lifted. One overtaken by the stroke's Down would report the pen out of
        // contact and cut the stroke in two: during contact it can only be stale.
        if matches!(sample.phase, PenPhase::Hover) && self.pen_in_contact {
            return;
        }
        // Windows expects the pen to be in range before contact.
        if matches!(sample.phase, PenPhase::Down) && !self.pen_in_range {
            self.inject_pen(PenSample {
                phase: PenPhase::Hover,
                pressure: 0.0,
                ..sample
            });
        }
        self.inject_pen(sample);
    }

    fn inject_pen(&mut self, sample: PenSample) {
        let Some(device) = self.pen_device else {
            return;
        };
        let flags = match sample.phase {
            PenPhase::Hover => POINTER_FLAG_INRANGE | POINTER_FLAG_UPDATE,
            PenPhase::Down => {
                POINTER_FLAG_INRANGE
                    | POINTER_FLAG_INCONTACT
                    | POINTER_FLAG_DOWN
                    | POINTER_FLAG_FIRSTBUTTON
            }
            PenPhase::Move if self.pen_in_contact => {
                POINTER_FLAG_INRANGE
                    | POINTER_FLAG_INCONTACT
                    | POINTER_FLAG_UPDATE
                    | POINTER_FLAG_FIRSTBUTTON
            }
            PenPhase::Move => POINTER_FLAG_INRANGE | POINTER_FLAG_UPDATE,
            PenPhase::Up => POINTER_FLAG_INRANGE | POINTER_FLAG_UP,
            PenPhase::Leave => POINTER_FLAG_UPDATE,
            PenPhase::Cancel => POINTER_FLAG_UP | POINTER_FLAG_CANCELED,
        };
        if matches!(
            sample.phase,
            PenPhase::Up | PenPhase::Leave | PenPhase::Cancel
        ) && !self.pen_in_range
            && !self.pen_in_contact
        {
            return;
        }
        let point = self.to_screen(sample.x, sample.y);
        let mut pen_flags = 0;
        if sample.barrel {
            pen_flags |= PEN_FLAG_BARREL;
        }
        if sample.eraser {
            pen_flags |= if matches!(sample.phase, PenPhase::Down | PenPhase::Move)
                && self.pen_in_contact
                || sample.phase == PenPhase::Down
            {
                PEN_FLAG_ERASER
            } else {
                PEN_FLAG_INVERTED
            };
        }
        let info = POINTER_TYPE_INFO {
            r#type: PT_PEN,
            Anonymous: POINTER_TYPE_INFO_0 {
                penInfo: POINTER_PEN_INFO {
                    pointerInfo: POINTER_INFO {
                        pointerType: PT_PEN,
                        pointerFlags: flags | POINTER_FLAG_PRIMARY,
                        ptPixelLocation: point,
                        ..Default::default()
                    },
                    penFlags: pen_flags,
                    penMask: PEN_MASK_PRESSURE
                        | PEN_MASK_ROTATION
                        | PEN_MASK_TILT_X
                        | PEN_MASK_TILT_Y,
                    pressure: (sample.pressure * 1024.0).round().clamp(0.0, 1024.0) as u32,
                    rotation: sample.twist as u32,
                    tiltX: sample.tilt_x as i32,
                    tiltY: sample.tilt_y as i32,
                },
            },
        };
        match unsafe { InjectSyntheticPointerInput(device, &[info]) } {
            Ok(()) => {
                match sample.phase {
                    PenPhase::Hover | PenPhase::Move => self.pen_in_range = true,
                    PenPhase::Down => {
                        self.pen_in_range = true;
                        self.pen_in_contact = true;
                    }
                    PenPhase::Up => self.pen_in_contact = false,
                    PenPhase::Leave | PenPhase::Cancel => {
                        self.pen_in_range = false;
                        self.pen_in_contact = false;
                    }
                }
                self.pen_last = Some(sample);
            }
            Err(error) => tracing::debug!(%error, "pen injection failed"),
        }
    }

    fn touch(&mut self, contacts: Vec<TouchContact>) {
        if self.touch_device.is_none() {
            match unsafe {
                CreateSyntheticPointerDevice(
                    PT_TOUCH,
                    crate::input::MAX_TOUCH_CONTACTS as u32,
                    POINTER_FEEDBACK_DEFAULT,
                )
            } {
                Ok(device) => self.touch_device = Some(device),
                Err(error) => {
                    tracing::warn!(%error, "touch injection unavailable");
                    return;
                }
            }
        }
        let Some(device) = self.touch_device else {
            return;
        };
        for contact in &contacts {
            match contact.phase {
                TouchPhase::Down | TouchPhase::Move => {
                    if contact.phase == TouchPhase::Move && !self.touches.contains_key(&contact.id)
                    {
                        continue;
                    }
                    self.touches.insert(contact.id, *contact);
                }
                TouchPhase::Up | TouchPhase::Cancel => {
                    if let Some(existing) = self.touches.get_mut(&contact.id) {
                        *existing = *contact;
                    }
                }
            }
        }
        let changed: HashSet<u8> = contacts.iter().map(|c| c.id).collect();
        let primary = self.touches.keys().next().copied();
        let frame: Vec<POINTER_TYPE_INFO> = self
            .touches
            .values()
            .map(|contact| {
                let point = self.to_screen(contact.x, contact.y);
                let mut flags: POINTER_FLAGS = match contact.phase {
                    TouchPhase::Down if changed.contains(&contact.id) => {
                        POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT
                    }
                    TouchPhase::Up => POINTER_FLAG_UP,
                    TouchPhase::Cancel => POINTER_FLAG_UP | POINTER_FLAG_CANCELED,
                    _ => POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
                };
                if Some(contact.id) == primary {
                    flags |= POINTER_FLAG_PRIMARY;
                }
                POINTER_TYPE_INFO {
                    r#type: PT_TOUCH,
                    Anonymous: POINTER_TYPE_INFO_0 {
                        touchInfo: POINTER_TOUCH_INFO {
                            pointerInfo: POINTER_INFO {
                                pointerType: PT_TOUCH,
                                pointerId: contact.id as u32,
                                pointerFlags: flags,
                                ptPixelLocation: point,
                                ..Default::default()
                            },
                            touchMask: TOUCH_MASK_CONTACTAREA | TOUCH_MASK_PRESSURE,
                            rcContact: RECT {
                                left: point.x - 4,
                                top: point.y - 4,
                                right: point.x + 4,
                                bottom: point.y + 4,
                            },
                            pressure: (contact.pressure.max(0.3) * 1024.0) as u32,
                            ..Default::default()
                        },
                    },
                }
            })
            .collect();
        if !frame.is_empty() {
            if let Err(error) = unsafe { InjectSyntheticPointerInput(device, &frame) } {
                tracing::debug!(%error, "touch injection failed");
            }
        }
        // After a Down frame, the contact continues as Move; Up/Cancel contacts are removed.
        self.touches
            .retain(|_, c| !matches!(c.phase, TouchPhase::Up | TouchPhase::Cancel));
        for contact in self.touches.values_mut() {
            contact.phase = TouchPhase::Move;
        }
    }

    fn release_all(&mut self) {
        let keys: Vec<u16> = self.held_keys.drain().collect();
        let inputs: Vec<INPUT> = keys.iter().map(|vk| self.key(*vk, false)).collect();
        send(&inputs);
        let buttons: Vec<u8> = self.held_buttons.drain().collect();
        for id in buttons {
            let (flags, data) = match id {
                0 => (MOUSEEVENTF_LEFTUP, 0),
                1 => (MOUSEEVENTF_RIGHTUP, 0),
                2 => (MOUSEEVENTF_MIDDLEUP, 0),
                3 => (MOUSEEVENTF_XUP, XBUTTON1 as u32),
                _ => (MOUSEEVENTF_XUP, XBUTTON2 as u32),
            };
            send(&[mouse_input(0, 0, data, flags)]);
        }
        if let Some(last) = self.pen_last {
            if self.pen_in_contact {
                self.inject_pen(PenSample {
                    phase: PenPhase::Up,
                    pressure: 0.0,
                    ..last
                });
            }
            if self.pen_in_range {
                self.inject_pen(PenSample {
                    phase: PenPhase::Leave,
                    pressure: 0.0,
                    ..last
                });
            }
        }
        if !self.touches.is_empty() {
            let ups: Vec<TouchContact> = self
                .touches
                .values()
                .map(|c| TouchContact {
                    phase: TouchPhase::Cancel,
                    ..*c
                })
                .collect();
            self.touch(ups);
        }
        self.wheel_accum = (0.0, 0.0);
    }
}
