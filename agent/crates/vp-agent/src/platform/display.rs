//! Display enumeration (Quartz Display Services + NSScreen names).
//!
//! Geometry is kept in both units macOS uses: points for input (Quartz event coordinates are
//! global points with the origin at the top-left of the main display) and backing pixels for
//! capture, so Retina displays are streamed at full sharpness.

use super::ffi;

/// A rectangle in global display points.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn right(&self) -> f64 {
        self.x + self.width
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
}

#[derive(Debug, Clone)]
pub struct Output {
    /// Stable identifier sent to the iPad (the CGDirectDisplayID in decimal).
    pub id: String,
    pub display_id: u32,
    pub name: String,
    /// Position and size in global points.
    pub bounds: Rect,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub refresh_hz: u32,
    pub primary: bool,
    pub builtin: bool,
}

impl Output {
    /// Native capture width in pixels.
    pub fn width(&self) -> u32 {
        self.pixel_width
    }
    /// Native capture height in pixels.
    pub fn height(&self) -> u32 {
        self.pixel_height
    }
    /// Backing scale factor (2.0 on Retina displays).
    pub fn scale(&self) -> f64 {
        if self.bounds.width > 0.0 {
            self.pixel_width as f64 / self.bounds.width
        } else {
            1.0
        }
    }
}

/// Every active display (mirrors excluded), main display first.
pub fn outputs() -> Vec<Output> {
    let mut raw = vec![ffi::vp_display_t::default(); 32];
    let count = unsafe { ffi::vp_displays(raw.as_mut_ptr(), raw.len() as i32) }.clamp(0, 32);
    let mut list: Vec<Output> = raw
        .into_iter()
        .take(count as usize)
        .filter(|d| d.pixel_width > 0 && d.pixel_height > 0 && d.width > 0.0 && d.height > 0.0)
        .map(|d| Output {
            id: d.id.to_string(),
            display_id: d.id,
            name: ffi::c_buffer_to_string(&d.name),
            bounds: Rect {
                x: d.x,
                y: d.y,
                width: d.width,
                height: d.height,
            },
            pixel_width: d.pixel_width,
            pixel_height: d.pixel_height,
            refresh_hz: d.refresh_hz.clamp(24, 240),
            primary: d.primary,
            builtin: d.builtin,
        })
        .collect();
    list.sort_by_key(|o| !o.primary);
    list
}

#[cfg(test)]
mod tests {
    use super::Rect;

    #[test]
    fn rect_contains_is_half_open() {
        let rect = Rect {
            x: -1440.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        };
        assert!(rect.contains(-1440.0, 0.0));
        assert!(rect.contains(-0.5, 899.5));
        assert!(!rect.contains(0.0, 10.0));
        assert!(!rect.contains(-10.0, 900.0));
    }
}
