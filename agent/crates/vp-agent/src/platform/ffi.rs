//! Raw bindings to the Objective-C bridge in `native/vp_macos.m` (contract: `native/vp_macos.h`).
//! Everything here is `unsafe`; the safe wrappers live in the sibling modules.

#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_void};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct vp_display_t {
    pub id: u32,
    pub name: [c_char; 128],
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub refresh_hz: u32,
    pub primary: bool,
    pub builtin: bool,
}

impl Default for vp_display_t {
    fn default() -> Self {
        vp_display_t {
            id: 0,
            name: [0; 128],
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            pixel_width: 0,
            pixel_height: 0,
            refresh_hz: 0,
            primary: false,
            builtin: false,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct vp_cursor_info_t {
    pub width: u32,
    pub height: u32,
    pub hot_x: i32,
    pub hot_y: i32,
    pub hash: u64,
}

pub const VP_CODEC_H264: i32 = 0;
pub const VP_CODEC_HEVC: i32 = 1;

extern "C" {
    pub fn vp_displays(out: *mut vp_display_t, capacity: i32) -> i32;

    pub fn vp_screen_capture_allowed() -> bool;
    pub fn vp_screen_capture_request() -> bool;
    pub fn vp_accessibility_allowed(prompt: bool) -> bool;
    pub fn vp_open_privacy_settings(pane: i32);

    pub fn vp_capture_start(
        display_id: u32,
        width: u32,
        height: u32,
        fps: u32,
        error: *mut c_char,
        error_len: usize,
    ) -> *mut c_void;
    pub fn vp_capture_next(capture: *mut c_void, timeout_ms: u32, frame: *mut *mut c_void) -> i32;
    pub fn vp_capture_stop(capture: *mut c_void);
    pub fn vp_frame_retain(frame: *mut c_void) -> *mut c_void;
    pub fn vp_frame_release(frame: *mut c_void);
    pub fn vp_frame_copy_nv12(
        frame: *mut c_void,
        y: *mut u8,
        uv: *mut u8,
        width: u32,
        height: u32,
    ) -> bool;

    pub fn vp_hardware_encoders() -> u32;
    pub fn vp_encoder_create(
        codec: i32,
        width: u32,
        height: u32,
        fps: u32,
        bitrate_kbps: u32,
        prioritize_speed: bool,
        error: *mut c_char,
        error_len: usize,
    ) -> *mut c_void;
    pub fn vp_encoder_set_bitrate(encoder: *mut c_void, bitrate_kbps: u32);
    pub fn vp_encoder_encode(
        encoder: *mut c_void,
        frame: *mut c_void,
        pts_us: i64,
        force_keyframe: bool,
        data: *mut *const u8,
        len: *mut usize,
        keyframe: *mut bool,
    ) -> i32;
    pub fn vp_encoder_is_hardware(encoder: *mut c_void) -> bool;
    pub fn vp_encoder_destroy(encoder: *mut c_void);

    pub fn vp_cursor_location(x: *mut f64, y: *mut f64) -> bool;
    pub fn vp_cursor_image(
        scale: f64,
        rgba: *mut u8,
        capacity: usize,
        info: *mut vp_cursor_info_t,
    ) -> i32;

    pub fn vp_double_click_interval() -> f64;
    pub fn vp_mouse_move(x: f64, y: f64, held_button: i32, flags: u64);
    pub fn vp_mouse_button(x: f64, y: f64, button: i32, down: bool, click_count: i32, flags: u64);
    pub fn vp_scroll(vertical: i32, horizontal: i32, lines: bool, flags: u64);
    pub fn vp_key(keycode: u16, down: bool, flags: u64, modifier: bool);
    pub fn vp_text(units: *const u16, count: usize);
    pub fn vp_media_key(key: i32, down: bool);
    pub fn vp_tablet_proximity(entering: bool, eraser: bool);
    #[allow(clippy::too_many_arguments)]
    pub fn vp_tablet_point(
        x: f64,
        y: f64,
        phase: i32,
        button: i32,
        pressure: f64,
        tilt_x: f64,
        tilt_y: f64,
        rotation: f64,
        flags: u64,
    );
    pub fn vp_magnify(phase: i32, magnification: f64, x: f64, y: f64);

    pub fn vp_frontmost_app(
        bundle_id: *mut c_char,
        bundle_len: usize,
        name: *mut c_char,
        name_len: usize,
    ) -> bool;
    pub fn vp_computer_name(out: *mut c_char, len: usize) -> bool;
    pub fn vp_cpu_brand(out: *mut c_char, len: usize) -> bool;
    pub fn vp_wake_acquire() -> u32;
    pub fn vp_wake_release(assertion: u32);

    pub fn vp_keychain_get(
        service: *const c_char,
        account: *const c_char,
        out: *mut u8,
        capacity: usize,
        len: *mut usize,
    ) -> i32;
    pub fn vp_keychain_set(
        service: *const c_char,
        account: *const c_char,
        data: *const u8,
        len: usize,
    ) -> i32;
    pub fn vp_keychain_delete(service: *const c_char, account: *const c_char) -> i32;
}

/// NUL-terminated C buffer → owned UTF-8 string (lossy).
pub fn c_buffer_to_string(buffer: &[c_char]) -> String {
    let bytes: Vec<u8> = buffer
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Calls a bridge function that fills a string buffer.
pub fn read_string(len: usize, fill: impl FnOnce(*mut c_char, usize) -> bool) -> Option<String> {
    let mut buffer = vec![0 as c_char; len];
    if !fill(buffer.as_mut_ptr(), buffer.len()) {
        return None;
    }
    let text = c_buffer_to_string(&buffer);
    (!text.is_empty()).then_some(text)
}
