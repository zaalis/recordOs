//! Screen capture with ScreenCaptureKit: frames stay in IOSurface-backed GPU memory, are scaled
//! and converted to NV12 by the window server, and only changed frames are delivered. The pointer
//! is excluded from the video and reported separately (position + shape) so the iPad draws it
//! locally without video latency.

use super::display::Output;
use super::ffi;
use std::ffi::{c_char, c_void};
use std::ptr::NonNull;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct CursorShape {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub hot_x: i32,
    pub hot_y: i32,
    pub rgba: Vec<u8>,
}

/// Pointer position in captured-display pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CursorState {
    pub x: i32,
    pub y: i32,
    pub visible: bool,
    pub shape_id: u32,
}

/// A retained NV12 `CVPixelBuffer`.
pub struct Frame(NonNull<c_void>);

// A CVPixelBuffer is reference counted and immutable once delivered; it may move between threads.
unsafe impl Send for Frame {}

impl Frame {
    pub(crate) fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }

    /// Copies the planes into tightly packed buffers (software encoder path).
    pub fn copy_nv12(
        &self,
        width: u32,
        height: u32,
        y: &mut Vec<u8>,
        uv: &mut Vec<u8>,
    ) -> anyhow::Result<()> {
        y.resize(width as usize * height as usize, 0);
        uv.resize(width as usize * (height as usize / 2), 0);
        let ok = unsafe {
            ffi::vp_frame_copy_nv12(
                self.as_ptr(),
                y.as_mut_ptr(),
                uv.as_mut_ptr(),
                width,
                height,
            )
        };
        anyhow::ensure!(ok, "captured frame is not a readable NV12 buffer");
        Ok(())
    }
}

impl Clone for Frame {
    fn clone(&self) -> Frame {
        let ptr = unsafe { ffi::vp_frame_retain(self.as_ptr()) };
        Frame(NonNull::new(ptr).expect("retained frame"))
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        unsafe { ffi::vp_frame_release(self.as_ptr()) }
    }
}

pub enum Acquire {
    Frame(Frame),
    NoChange,
    Stopped,
}

pub struct ScreenCapture {
    raw: NonNull<c_void>,
    pub width: u32,
    pub height: u32,
}

// The Objective-C capture object synchronises its own state; the handle is owned by one thread.
unsafe impl Send for ScreenCapture {}

impl ScreenCapture {
    /// Starts streaming `output`, scaled to `width` × `height` pixels, at most `fps` frames/s.
    pub fn start(
        output: &Output,
        width: u32,
        height: u32,
        fps: u32,
    ) -> anyhow::Result<ScreenCapture> {
        let mut error = [0 as c_char; 512];
        let raw = unsafe {
            ffi::vp_capture_start(
                output.display_id,
                width,
                height,
                fps,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        match NonNull::new(raw) {
            Some(raw) => Ok(ScreenCapture { raw, width, height }),
            None => {
                let message = ffi::c_buffer_to_string(&error);
                anyhow::bail!(if message.is_empty() {
                    "screen capture could not start".to_string()
                } else {
                    message
                })
            }
        }
    }

    /// Waits up to `timeout_ms` for a new frame.
    pub fn next(&mut self, timeout_ms: u32) -> Acquire {
        let mut frame: *mut c_void = std::ptr::null_mut();
        match unsafe { ffi::vp_capture_next(self.raw.as_ptr(), timeout_ms, &mut frame) } {
            1 => match NonNull::new(frame) {
                Some(frame) => Acquire::Frame(Frame(frame)),
                None => Acquire::NoChange,
            },
            0 => Acquire::NoChange,
            _ => Acquire::Stopped,
        }
    }
}

impl Drop for ScreenCapture {
    fn drop(&mut self) {
        unsafe { ffi::vp_capture_stop(self.raw.as_ptr()) }
    }
}

const SHAPE_POLL: Duration = Duration::from_millis(100);
const MAX_SHAPE_BYTES: usize = 256 * 256 * 4;

/// Follows the system pointer: position every frame, shape (rendered from `NSCursor`) at 10 Hz.
pub struct CursorTracker {
    last_poll: Instant,
    last_hash: u64,
    counter: u32,
    shape_id: u32,
    buffer: Vec<u8>,
}

impl Default for CursorTracker {
    fn default() -> Self {
        CursorTracker {
            last_poll: Instant::now() - SHAPE_POLL,
            last_hash: 0,
            counter: 0,
            shape_id: 0,
            buffer: vec![0u8; MAX_SHAPE_BYTES],
        }
    }
}

impl CursorTracker {
    /// Returns the pointer state relative to `output` (pixels) and a new shape when it changed.
    pub fn poll(&mut self, output: &Output) -> (CursorState, Option<CursorShape>) {
        let mut shape = None;
        if self.last_poll.elapsed() >= SHAPE_POLL {
            self.last_poll = Instant::now();
            shape = self.read_shape(output.scale());
        }
        let (mut x, mut y) = (0.0f64, 0.0f64);
        let located = unsafe { ffi::vp_cursor_location(&mut x, &mut y) };
        let visible = located && output.bounds.contains(x, y);
        let scale = output.scale();
        let state = CursorState {
            x: ((x - output.bounds.x) * scale).round() as i32,
            y: ((y - output.bounds.y) * scale).round() as i32,
            visible,
            shape_id: self.shape_id,
        };
        (state, shape)
    }

    fn read_shape(&mut self, scale: f64) -> Option<CursorShape> {
        let mut info = ffi::vp_cursor_info_t::default();
        let result = unsafe {
            ffi::vp_cursor_image(
                scale,
                self.buffer.as_mut_ptr(),
                self.buffer.len(),
                &mut info,
            )
        };
        if result != 1 || info.hash == self.last_hash || info.width == 0 || info.height == 0 {
            return None;
        }
        self.last_hash = info.hash;
        self.counter = self.counter.wrapping_add(1).max(1);
        self.shape_id = self.counter;
        let bytes = info.width as usize * info.height as usize * 4;
        Some(CursorShape {
            id: self.counter,
            width: info.width,
            height: info.height,
            hot_x: info.hot_x,
            hot_y: info.hot_y,
            rgba: self.buffer[..bytes].to_vec(),
        })
    }
}

pub fn encode_png(shape: &CursorShape) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, shape.width, shape.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&shape.rgba).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{encode_png, CursorShape};

    #[test]
    fn cursor_png_roundtrips_dimensions() {
        let shape = CursorShape {
            id: 1,
            width: 2,
            height: 3,
            hot_x: 0,
            hot_y: 0,
            rgba: vec![255; 2 * 3 * 4],
        };
        let png = encode_png(&shape).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }
}
