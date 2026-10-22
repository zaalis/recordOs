//! Binary input decoding and validation (mirror of packages/protocol/src/input.ts).
//! Every message is length-checked, every float must be finite and in range, and keys must be in
//! the allowlist. Anything else is dropped. There is no way to express a system command.

pub const KIND_MOVE_ABS: u8 = 0x01;
pub const KIND_MOVE_REL: u8 = 0x02;
pub const KIND_BUTTON: u8 = 0x03;
pub const KIND_WHEEL: u8 = 0x04;
pub const KIND_KEY: u8 = 0x05;
pub const KIND_TEXT: u8 = 0x06;
pub const KIND_PEN: u8 = 0x07;
pub const KIND_TOUCH: u8 = 0x08;
pub const KIND_COMBO: u8 = 0x09;
pub const KIND_RELEASE_ALL: u8 = 0x0a;

pub const EXTENDED: u16 = 0x100;
pub const MAX_TEXT_UNITS: usize = 256;
pub const MAX_COMBO_KEYS: usize = 6;
pub const MAX_TOUCH_CONTACTS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenPhase {
    Hover,
    Down,
    Move,
    Up,
    Leave,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase {
    Down,
    Move,
    Up,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenSample {
    pub phase: PenPhase,
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub tilt_x: i8,
    pub tilt_y: i8,
    pub twist: u16,
    pub barrel: bool,
    pub eraser: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TouchContact {
    pub id: u8,
    pub phase: TouchPhase,
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    MoveAbs {
        x: f32,
        y: f32,
    },
    MoveRel {
        dx: f32,
        dy: f32,
    },
    Button {
        button: MouseButton,
        down: bool,
        at: Option<(f32, f32)>,
    },
    Wheel {
        dx: f32,
        dy: f32,
        at: Option<(f32, f32)>,
    },
    Key {
        vk: u16,
        down: bool,
    },
    Text(Vec<u16>),
    Pen(PenSample),
    Touch(Vec<TouchContact>),
    Combo(Vec<u16>),
    ReleaseAll,
}

impl InputEvent {
    pub fn category(&self) -> InputCategory {
        match self {
            InputEvent::MoveAbs { .. }
            | InputEvent::MoveRel { .. }
            | InputEvent::Button { .. }
            | InputEvent::Wheel { .. } => InputCategory::Mouse,
            InputEvent::Key { .. } | InputEvent::Text(_) | InputEvent::Combo(_) => {
                InputCategory::Keyboard
            }
            InputEvent::Pen(_) => InputCategory::Pen,
            InputEvent::Touch(_) => InputCategory::Touch,
            InputEvent::ReleaseAll => InputCategory::System,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputCategory {
    Mouse,
    Keyboard,
    Pen,
    Touch,
    System,
}

/// Windows virtual keys accepted from the iPad (see packages/protocol/src/keys.ts).
pub fn is_allowed_vk(vk: u16) -> bool {
    let base = vk & 0xff;
    let extended = vk & EXTENDED != 0;
    if vk & !0x1ff != 0 {
        return false;
    }
    if extended {
        return matches!(base, 0x0d | 0x6f);
    }
    matches!(base,
        0x08 | 0x09 | 0x0d | 0x10 | 0x11 | 0x12 | 0x13 | 0x14 | 0x1b | 0x20..=0x28 | 0x2c | 0x2d | 0x2e |
        0x30..=0x39 | 0x41..=0x5a | 0x5b | 0x5d | 0x60..=0x6f | 0x70..=0x87 | 0x90 | 0x91 |
        0xad..=0xb3 | 0xba..=0xc0 | 0xdb..=0xde | 0xe2
    )
}

fn f32_at(data: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn u16_at(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

fn unit(value: f32) -> Option<f32> {
    (value.is_finite() && (0.0..=1.0).contains(&value)).then_some(value)
}

fn position(data: &[u8], offset: usize) -> Option<Option<(f32, f32)>> {
    let x = f32_at(data, offset);
    let y = f32_at(data, offset + 4);
    if x.is_nan() && y.is_nan() {
        return Some(None);
    }
    Some(Some((unit(x)?, unit(y)?)))
}

/// Decodes one input message. Returns the sequence number and the event, or `None` if invalid.
pub fn decode(data: &[u8]) -> Option<(u16, InputEvent)> {
    if data.len() < 4 {
        return None;
    }
    let kind = data[0];
    let flags = data[1];
    let seq = u16_at(data, 2);
    let event = match kind {
        KIND_MOVE_ABS if data.len() == 12 => InputEvent::MoveAbs {
            x: unit(f32_at(data, 4))?,
            y: unit(f32_at(data, 8))?,
        },
        KIND_MOVE_REL if data.len() == 12 => {
            let dx = f32_at(data, 4);
            let dy = f32_at(data, 8);
            if !dx.is_finite() || !dy.is_finite() || dx.abs() > 1.0 || dy.abs() > 1.0 {
                return None;
            }
            InputEvent::MoveRel { dx, dy }
        }
        KIND_BUTTON if data.len() == 16 => {
            let button = match data[4] {
                0 => MouseButton::Left,
                1 => MouseButton::Right,
                2 => MouseButton::Middle,
                3 => MouseButton::Back,
                4 => MouseButton::Forward,
                _ => return None,
            };
            let down = match data[5] {
                0 => false,
                1 => true,
                _ => return None,
            };
            InputEvent::Button {
                button,
                down,
                at: position(data, 8)?,
            }
        }
        KIND_WHEEL if data.len() == 20 => {
            let dx = f32_at(data, 4);
            let dy = f32_at(data, 8);
            if !dx.is_finite() || !dy.is_finite() || dx.abs() > 12_000.0 || dy.abs() > 12_000.0 {
                return None;
            }
            let _precise = flags & 1 != 0;
            InputEvent::Wheel {
                dx,
                dy,
                at: position(data, 12)?,
            }
        }
        KIND_KEY if data.len() == 8 => {
            let vk = u16_at(data, 4);
            if !is_allowed_vk(vk) || data[6] > 1 {
                return None;
            }
            InputEvent::Key {
                vk,
                down: data[6] == 1,
            }
        }
        KIND_TEXT => {
            if data.len() < 6 {
                return None;
            }
            let count = u16_at(data, 4) as usize;
            if count == 0 || count > MAX_TEXT_UNITS || data.len() != 6 + count * 2 {
                return None;
            }
            let units: Vec<u16> = (0..count).map(|i| u16_at(data, 6 + i * 2)).collect();
            // Reject control characters except tab and newline.
            if units
                .iter()
                .any(|&u| (u < 0x20 && u != 0x09 && u != 0x0a) || u == 0x7f)
            {
                return None;
            }
            InputEvent::Text(units)
        }
        KIND_PEN if data.len() == 24 => {
            let phase = match data[4] {
                0 => PenPhase::Hover,
                1 => PenPhase::Down,
                2 => PenPhase::Move,
                3 => PenPhase::Up,
                4 => PenPhase::Leave,
                5 => PenPhase::Cancel,
                _ => return None,
            };
            let buttons = data[5];
            if buttons & !0x03 != 0 {
                return None;
            }
            let tilt_x = data[6] as i8;
            let tilt_y = data[7] as i8;
            let twist = u16_at(data, 20);
            if !(-90..=90).contains(&tilt_x) || !(-90..=90).contains(&tilt_y) || twist >= 360 {
                return None;
            }
            InputEvent::Pen(PenSample {
                phase,
                x: unit(f32_at(data, 8))?,
                y: unit(f32_at(data, 12))?,
                pressure: unit(f32_at(data, 16))?,
                tilt_x,
                tilt_y,
                twist,
                barrel: buttons & 1 != 0,
                eraser: buttons & 2 != 0,
            })
        }
        KIND_TOUCH => {
            if data.len() < 8 {
                return None;
            }
            let count = data[4] as usize;
            if count == 0 || count > MAX_TOUCH_CONTACTS || data.len() != 8 + count * 16 {
                return None;
            }
            let mut contacts = Vec::with_capacity(count);
            for i in 0..count {
                let o = 8 + i * 16;
                let phase = match data[o + 1] {
                    1 => TouchPhase::Down,
                    2 => TouchPhase::Move,
                    3 => TouchPhase::Up,
                    5 => TouchPhase::Cancel,
                    _ => return None,
                };
                let id = data[o];
                if id as usize >= 32 || contacts.iter().any(|c: &TouchContact| c.id == id) {
                    return None;
                }
                contacts.push(TouchContact {
                    id,
                    phase,
                    x: unit(f32_at(data, o + 4))?,
                    y: unit(f32_at(data, o + 8))?,
                    pressure: unit(f32_at(data, o + 12))?,
                });
            }
            InputEvent::Touch(contacts)
        }
        KIND_COMBO => {
            if data.len() < 6 {
                return None;
            }
            let count = data[4] as usize;
            if count == 0 || count > MAX_COMBO_KEYS || data.len() != 6 + count * 2 {
                return None;
            }
            let keys: Vec<u16> = (0..count).map(|i| u16_at(data, 6 + i * 2)).collect();
            if !keys.iter().all(|&vk| is_allowed_vk(vk)) {
                return None;
            }
            InputEvent::Combo(keys)
        }
        KIND_RELEASE_ALL if data.len() == 4 => InputEvent::ReleaseAll,
        _ => return None,
    };
    Some((seq, event))
}

/// Tracks sequence numbers of latest-wins (realtime) input so stale moves are discarded.
#[derive(Default)]
pub struct SequenceFilter {
    last: Option<u16>,
}

impl SequenceFilter {
    pub fn accept(&mut self, seq: u16) -> bool {
        match self.last {
            Some(last) => {
                let delta = seq.wrapping_sub(last);
                if delta == 0 || delta > 0x8000 {
                    return false;
                }
                self.last = Some(seq);
                true
            }
            None => {
                self.last = Some(seq);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(kind: u8, len: usize) -> Vec<u8> {
        let mut v = vec![0u8; len];
        v[0] = kind;
        v
    }

    #[test]
    fn decodes_valid_messages() {
        let mut m = header(KIND_MOVE_ABS, 12);
        m[4..8].copy_from_slice(&0.25f32.to_le_bytes());
        m[8..12].copy_from_slice(&0.75f32.to_le_bytes());
        assert_eq!(
            decode(&m).unwrap().1,
            InputEvent::MoveAbs { x: 0.25, y: 0.75 }
        );

        let mut b = header(KIND_BUTTON, 16);
        b[4] = 1;
        b[5] = 1;
        b[8..12].copy_from_slice(&f32::NAN.to_le_bytes());
        b[12..16].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(
            decode(&b).unwrap().1,
            InputEvent::Button {
                button: MouseButton::Right,
                down: true,
                at: None
            }
        );

        let mut c = header(KIND_COMBO, 10);
        c[4] = 2;
        c[6..8].copy_from_slice(&0x11u16.to_le_bytes());
        c[8..10].copy_from_slice(&0x5au16.to_le_bytes());
        assert_eq!(decode(&c).unwrap().1, InputEvent::Combo(vec![0x11, 0x5a]));
    }

    #[test]
    fn rejects_invalid_messages() {
        let mut m = header(KIND_MOVE_ABS, 12);
        m[4..8].copy_from_slice(&f32::INFINITY.to_le_bytes());
        assert!(decode(&m).is_none());
        m[4..8].copy_from_slice(&1.5f32.to_le_bytes());
        assert!(decode(&m).is_none());
        assert!(decode(&header(KIND_MOVE_ABS, 13)).is_none());
        let mut k = header(KIND_KEY, 8);
        k[4..6].copy_from_slice(&0x5cu16.to_le_bytes()); // right Windows key is not in the allowlist
        assert!(decode(&k).is_none());
        k[4..6].copy_from_slice(&0xffffu16.to_le_bytes());
        assert!(decode(&k).is_none());
        let mut t = header(KIND_TEXT, 8);
        t[4] = 1;
        t[6] = 0x07; // bell control character
        assert!(decode(&t).is_none());
        assert!(decode(&header(0x77, 4)).is_none());
        let mut touch = header(KIND_TOUCH, 8 + 32);
        touch[4] = 2;
        touch[9] = 1;
        touch[25] = 1; // duplicate id 0
        assert!(decode(&touch).is_none());
    }

    #[test]
    fn sequence_filter_discards_stale() {
        let mut f = SequenceFilter::default();
        assert!(f.accept(10));
        assert!(!f.accept(9));
        assert!(!f.accept(10));
        assert!(f.accept(11));
        assert!(!f.accept(0xffff));
        let mut w = SequenceFilter::default();
        assert!(w.accept(0xfffe));
        assert!(w.accept(1));
    }
}
