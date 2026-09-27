//! Vision-Pro key codes → macOS virtual key codes (`kVK_*` from HIToolbox/Events.h).
//!
//! The wire protocol keeps one fixed, validated key table shared with the iPad
//! (packages/protocol/src/keys.ts). Its numeric values follow the classic PC virtual-key layout,
//! which is simply a stable identifier here: this module translates every entry to the physical
//! Mac key, and the `Cmd` entry (0x5B) is the ⌘ Command key.

use crate::input::EXTENDED;

// CGEventFlags (Quartz Event Services).
pub const FLAG_ALPHA_SHIFT: u64 = 0x0001_0000;
pub const FLAG_SHIFT: u64 = 0x0002_0000;
pub const FLAG_CONTROL: u64 = 0x0004_0000;
pub const FLAG_OPTION: u64 = 0x0008_0000;
pub const FLAG_COMMAND: u64 = 0x0010_0000;
pub const FLAG_NUMERIC_PAD: u64 = 0x0020_0000;
pub const FLAG_FN: u64 = 0x0080_0000;

// Device-dependent bits for the left-hand modifiers (NX_DEVICEL*KEYMASK). Some applications
// distinguish left from right modifiers and expect these alongside the generic flag.
const DEVICE_LEFT_CONTROL: u64 = 0x0000_0001;
const DEVICE_LEFT_SHIFT: u64 = 0x0000_0002;
const DEVICE_LEFT_COMMAND: u64 = 0x0000_0008;
const DEVICE_LEFT_OPTION: u64 = 0x0000_0020;

// NX_KEYTYPE_* system-defined media keys.
pub const MEDIA_SOUND_UP: i32 = 0;
pub const MEDIA_SOUND_DOWN: i32 = 1;
pub const MEDIA_MUTE: i32 = 7;
pub const MEDIA_PLAY: i32 = 16;
pub const MEDIA_NEXT: i32 = 17;
pub const MEDIA_PREVIOUS: i32 = 18;

pub const KEY_RETURN: u16 = 0x24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacKey {
    /// Regular key. `flags` are the implicit flags real hardware adds (fn, numeric pad).
    Key { code: u16, flags: u64 },
    /// Modifier key: posted as a "flags changed" event.
    Modifier { code: u16, flag: u64 },
    /// Caps Lock toggles instead of being held.
    CapsLock,
    /// System media key (volume, playback).
    Media(i32),
}

const fn key(code: u16) -> Option<MacKey> {
    Some(MacKey::Key { code, flags: 0 })
}

const fn fn_key(code: u16) -> Option<MacKey> {
    Some(MacKey::Key {
        code,
        flags: FLAG_FN,
    })
}

const fn arrow(code: u16) -> Option<MacKey> {
    Some(MacKey::Key {
        code,
        flags: FLAG_FN | FLAG_NUMERIC_PAD,
    })
}

const fn keypad(code: u16) -> Option<MacKey> {
    Some(MacKey::Key {
        code,
        flags: FLAG_NUMERIC_PAD,
    })
}

const fn modifier(code: u16, flag: u64) -> Option<MacKey> {
    Some(MacKey::Modifier { code, flag })
}

/// Generic flag plus the matching left-device bit for a modifier.
pub fn modifier_mask(flag: u64) -> u64 {
    flag | match flag {
        FLAG_SHIFT => DEVICE_LEFT_SHIFT,
        FLAG_CONTROL => DEVICE_LEFT_CONTROL,
        FLAG_OPTION => DEVICE_LEFT_OPTION,
        FLAG_COMMAND => DEVICE_LEFT_COMMAND,
        _ => 0,
    }
}

pub fn translate(vk: u16) -> Option<MacKey> {
    if vk & EXTENDED != 0 {
        return match vk & 0xff {
            0x0d => keypad(0x4c), // keypad Enter
            0x6f => keypad(0x4b), // keypad Divide
            _ => None,
        };
    }
    match vk {
        0x08 => key(0x33),                    // Backspace → Delete
        0x09 => key(0x30),                    // Tab
        0x0d => key(KEY_RETURN),              // Return
        0x10 => modifier(0x38, FLAG_SHIFT),   // Shift
        0x11 => modifier(0x3b, FLAG_CONTROL), // Control
        0x12 => modifier(0x3a, FLAG_OPTION),  // Option
        0x13 => fn_key(0x71),                 // Pause → F15
        0x14 => Some(MacKey::CapsLock),       // Caps Lock
        0x1b => key(0x35),                    // Escape
        0x20 => key(0x31),                    // Space
        0x21 => fn_key(0x74),                 // Page Up
        0x22 => fn_key(0x79),                 // Page Down
        0x23 => fn_key(0x77),                 // End
        0x24 => fn_key(0x73),                 // Home
        0x25 => arrow(0x7b),                  // Left
        0x26 => arrow(0x7e),                  // Up
        0x27 => arrow(0x7c),                  // Right
        0x28 => arrow(0x7d),                  // Down
        0x2c => fn_key(0x69),                 // Print Screen → F13
        0x2d => fn_key(0x72),                 // Insert → Help
        0x2e => fn_key(0x75),                 // Delete → Forward Delete
        0x30 => key(0x1d),
        0x31 => key(0x12),
        0x32 => key(0x13),
        0x33 => key(0x14),
        0x34 => key(0x15),
        0x35 => key(0x17),
        0x36 => key(0x16),
        0x37 => key(0x1a),
        0x38 => key(0x1c),
        0x39 => key(0x19),
        0x41 => key(0x00),                    // A
        0x42 => key(0x0b),                    // B
        0x43 => key(0x08),                    // C
        0x44 => key(0x02),                    // D
        0x45 => key(0x0e),                    // E
        0x46 => key(0x03),                    // F
        0x47 => key(0x05),                    // G
        0x48 => key(0x04),                    // H
        0x49 => key(0x22),                    // I
        0x4a => key(0x26),                    // J
        0x4b => key(0x28),                    // K
        0x4c => key(0x25),                    // L
        0x4d => key(0x2e),                    // M
        0x4e => key(0x2d),                    // N
        0x4f => key(0x1f),                    // O
        0x50 => key(0x23),                    // P
        0x51 => key(0x0c),                    // Q
        0x52 => key(0x0f),                    // R
        0x53 => key(0x01),                    // S
        0x54 => key(0x11),                    // T
        0x55 => key(0x20),                    // U
        0x56 => key(0x09),                    // V
        0x57 => key(0x0d),                    // W
        0x58 => key(0x07),                    // X
        0x59 => key(0x10),                    // Y
        0x5a => key(0x06),                    // Z
        0x5b => modifier(0x37, FLAG_COMMAND), // ⌘ Command
        0x5d => key(0x6e),                    // Context menu key
        0x60 => keypad(0x52),
        0x61 => keypad(0x53),
        0x62 => keypad(0x54),
        0x63 => keypad(0x55),
        0x64 => keypad(0x56),
        0x65 => keypad(0x57),
        0x66 => keypad(0x58),
        0x67 => keypad(0x59),
        0x68 => keypad(0x5b),
        0x69 => keypad(0x5c),
        0x6a => keypad(0x43), // keypad ×
        0x6b => keypad(0x45), // keypad +
        0x6d => keypad(0x4e), // keypad −
        0x6e => keypad(0x41), // keypad .
        0x6f => keypad(0x4b), // keypad ÷
        0x70 => fn_key(0x7a), // F1
        0x71 => fn_key(0x78), // F2
        0x72 => fn_key(0x63), // F3
        0x73 => fn_key(0x76), // F4
        0x74 => fn_key(0x60), // F5
        0x75 => fn_key(0x61), // F6
        0x76 => fn_key(0x62), // F7
        0x77 => fn_key(0x64), // F8
        0x78 => fn_key(0x65), // F9
        0x79 => fn_key(0x6d), // F10
        0x7a => fn_key(0x67), // F11
        0x7b => fn_key(0x6f), // F12
        0x7c => fn_key(0x69), // F13
        0x7d => fn_key(0x6b), // F14
        0x7e => fn_key(0x71), // F15
        0x7f => fn_key(0x6a), // F16
        0x80 => fn_key(0x40), // F17
        0x81 => fn_key(0x4f), // F18
        0x82 => fn_key(0x50), // F19
        0x83 => fn_key(0x5a), // F20
        0x90 => keypad(0x47), // Num Lock → keypad Clear
        0x91 => fn_key(0x6b), // Scroll Lock → F14
        0xad => Some(MacKey::Media(MEDIA_MUTE)),
        0xae => Some(MacKey::Media(MEDIA_SOUND_DOWN)),
        0xaf => Some(MacKey::Media(MEDIA_SOUND_UP)),
        0xb0 => Some(MacKey::Media(MEDIA_NEXT)),
        0xb1 => Some(MacKey::Media(MEDIA_PREVIOUS)),
        0xb2 | 0xb3 => Some(MacKey::Media(MEDIA_PLAY)),
        0xba => key(0x29), // ;
        0xbb => key(0x18), // =
        0xbc => key(0x2b), // ,
        0xbd => key(0x1b), // -
        0xbe => key(0x2f), // .
        0xbf => key(0x2c), // /
        0xc0 => key(0x32), // `
        0xdb => key(0x21), // [
        0xdc => key(0x2a), // \
        0xdd => key(0x1e), // ]
        0xde => key(0x27), // '
        0xe2 => key(0x0a), // ISO § key
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::is_allowed_vk;

    #[test]
    fn modifiers_map_to_mac_modifiers() {
        assert_eq!(
            translate(0x5b),
            Some(MacKey::Modifier {
                code: 0x37,
                flag: FLAG_COMMAND
            })
        );
        assert_eq!(
            translate(0x12),
            Some(MacKey::Modifier {
                code: 0x3a,
                flag: FLAG_OPTION
            })
        );
        assert_eq!(modifier_mask(FLAG_COMMAND), FLAG_COMMAND | 0x08);
    }

    #[test]
    fn letters_digits_and_navigation() {
        assert_eq!(translate(0x5a), key(0x06));
        assert_eq!(translate(0x30), key(0x1d));
        assert_eq!(translate(0x0d | EXTENDED), keypad(0x4c));
        assert_eq!(translate(0x26), arrow(0x7e));
        assert_eq!(translate(0xb3), Some(MacKey::Media(MEDIA_PLAY)));
    }

    #[test]
    fn allowlisted_keys_translate_except_pc_only_keys() {
        // F21–F24 and the numeric separator have no Mac equivalent and are dropped.
        let pc_only = [0x6c, 0x84, 0x85, 0x86, 0x87];
        for vk in 0u16..0x200 {
            if is_allowed_vk(vk) && !pc_only.contains(&vk) {
                assert!(translate(vk).is_some(), "vk {vk:#x} has no Mac key");
            }
        }
    }
}
