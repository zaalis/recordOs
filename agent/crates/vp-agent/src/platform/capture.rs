//! Screen capture with the DXGI Desktop Duplication API: frames stay on the GPU, only changed
//! frames are delivered, and the cursor (position + shape) is reported separately so the iPad can
//! draw it locally without video latency.

use super::gpu::{Gpu, Output};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    IDXGIOutput5, IDXGIOutputDuplication, IDXGIResource, DXGI_ERROR_ACCESS_LOST,
    DXGI_ERROR_INVALID_CALL, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
    DXGI_OUTDUPL_POINTER_SHAPE_INFO,
};

const SHAPE_MONOCHROME: u32 = 1;
const SHAPE_COLOR: u32 = 2;
const SHAPE_MASKED_COLOR: u32 = 4;

#[derive(Debug, Clone)]
pub struct CursorShape {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub hot_x: i32,
    pub hot_y: i32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CursorState {
    pub x: i32,
    pub y: i32,
    pub visible: bool,
    pub shape_id: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Acquire {
    Frame,
    NoChange,
    AccessLost,
}

pub struct DesktopDuplicator {
    duplication: IDXGIOutputDuplication,
    context: windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    pub frame: ID3D11Texture2D,
    pub width: u32,
    pub height: u32,
    pub cursor: CursorState,
    pub new_shape: Option<CursorShape>,
    shape_buffer: Vec<u8>,
    shape_counter: u32,
    pub has_frame: bool,
}

unsafe impl Send for DesktopDuplicator {}

impl DesktopDuplicator {
    pub fn new(gpu: &Gpu, output: &Output) -> windows::core::Result<DesktopDuplicator> {
        unsafe {
            let duplication = match output.output.cast::<IDXGIOutput5>() {
                Ok(output5) => {
                    output5.DuplicateOutput1(&gpu.device, 0, &[DXGI_FORMAT_B8G8R8A8_UNORM])?
                }
                Err(_) => output.output.DuplicateOutput(&gpu.device)?,
            };
            let desc = duplication.GetDesc();
            let width = desc.ModeDesc.Width;
            let height = desc.ModeDesc.Height;
            let texture_desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut frame = None;
            gpu.device
                .CreateTexture2D(&texture_desc, None, Some(&mut frame))?;
            Ok(DesktopDuplicator {
                duplication,
                context: gpu.context.clone(),
                frame: frame.expect("texture"),
                width,
                height,
                cursor: CursorState::default(),
                new_shape: None,
                shape_buffer: Vec::new(),
                shape_counter: 0,
                has_frame: false,
            })
        }
    }

    /// Waits up to `timeout_ms` for a desktop update. Cursor state is refreshed as a side effect.
    pub fn acquire(&mut self, timeout_ms: u32) -> windows::core::Result<Acquire> {
        unsafe {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;
            match self
                .duplication
                .AcquireNextFrame(timeout_ms, &mut info, &mut resource)
            {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(Acquire::NoChange),
                Err(e)
                    if e.code() == DXGI_ERROR_ACCESS_LOST
                        || e.code() == DXGI_ERROR_INVALID_CALL =>
                {
                    return Ok(Acquire::AccessLost)
                }
                Err(e) => return Err(e),
            }
            let result = self.process_frame(&info, resource);
            let _ = self.duplication.ReleaseFrame();
            result
        }
    }

    unsafe fn process_frame(
        &mut self,
        info: &DXGI_OUTDUPL_FRAME_INFO,
        resource: Option<IDXGIResource>,
    ) -> windows::core::Result<Acquire> {
        let mut updated = false;
        if info.LastPresentTime != 0 {
            if let Some(resource) = resource {
                let texture: ID3D11Texture2D = resource.cast()?;
                self.context.CopyResource(&self.frame, &texture);
                updated = true;
                self.has_frame = true;
            }
        }
        if info.LastMouseUpdateTime != 0 {
            self.cursor.x = info.PointerPosition.Position.x;
            self.cursor.y = info.PointerPosition.Position.y;
            self.cursor.visible = info.PointerPosition.Visible.as_bool();
        }
        if info.PointerShapeBufferSize > 0 {
            self.shape_buffer
                .resize(info.PointerShapeBufferSize as usize, 0);
            let mut required = 0u32;
            let mut shape_info = DXGI_OUTDUPL_POINTER_SHAPE_INFO::default();
            if self
                .duplication
                .GetFramePointerShape(
                    info.PointerShapeBufferSize,
                    self.shape_buffer.as_mut_ptr().cast(),
                    &mut required,
                    &mut shape_info,
                )
                .is_ok()
            {
                if let Some(rgba) = convert_shape(&shape_info, &self.shape_buffer) {
                    self.shape_counter = self.shape_counter.wrapping_add(1).max(1);
                    let height = if shape_info.Type == SHAPE_MONOCHROME {
                        shape_info.Height / 2
                    } else {
                        shape_info.Height
                    };
                    self.cursor.shape_id = self.shape_counter;
                    self.new_shape = Some(CursorShape {
                        id: self.shape_counter,
                        width: shape_info.Width,
                        height,
                        hot_x: shape_info.HotSpot.x,
                        hot_y: shape_info.HotSpot.y,
                        rgba,
                    });
                }
            }
        }
        Ok(if updated {
            Acquire::Frame
        } else {
            Acquire::NoChange
        })
    }
}

/// Converts DXGI pointer shapes to straight RGBA. XOR/inverting pixels are drawn black with a
/// white halo so text cursors stay visible on any background.
fn convert_shape(info: &DXGI_OUTDUPL_POINTER_SHAPE_INFO, data: &[u8]) -> Option<Vec<u8>> {
    let width = info.Width as usize;
    let pitch = info.Pitch as usize;
    if width == 0 || width > 512 || info.Height == 0 || info.Height > 1024 {
        return None;
    }
    match info.Type {
        SHAPE_COLOR | SHAPE_MASKED_COLOR => {
            let height = info.Height as usize;
            if data.len() < pitch * height || pitch < width * 4 {
                return None;
            }
            let mut rgba = vec![0u8; width * height * 4];
            let mut invert = vec![false; width * height];
            for y in 0..height {
                for x in 0..width {
                    let s = y * pitch + x * 4;
                    let (b, g, r, a) = (data[s], data[s + 1], data[s + 2], data[s + 3]);
                    let d = (y * width + x) * 4;
                    if info.Type == SHAPE_COLOR {
                        rgba[d..d + 4].copy_from_slice(&[r, g, b, a]);
                    } else if a == 0 {
                        rgba[d..d + 4].copy_from_slice(&[r, g, b, 255]);
                    } else if r | g | b != 0 {
                        invert[y * width + x] = true;
                    }
                }
            }
            apply_invert(&mut rgba, &invert, width, height);
            Some(rgba)
        }
        SHAPE_MONOCHROME => {
            let height = (info.Height / 2) as usize;
            if data.len() < pitch * height * 2 || pitch * 8 < width {
                return None;
            }
            let bit = |row: usize, x: usize| (data[row * pitch + x / 8] >> (7 - (x % 8))) & 1;
            let mut rgba = vec![0u8; width * height * 4];
            let mut invert = vec![false; width * height];
            for y in 0..height {
                for x in 0..width {
                    let and = bit(y, x);
                    let xor = bit(y + height, x);
                    let d = (y * width + x) * 4;
                    match (and, xor) {
                        (0, 0) => rgba[d..d + 4].copy_from_slice(&[0, 0, 0, 255]),
                        (0, 1) => rgba[d..d + 4].copy_from_slice(&[255, 255, 255, 255]),
                        (1, 1) => invert[y * width + x] = true,
                        _ => {}
                    }
                }
            }
            apply_invert(&mut rgba, &invert, width, height);
            Some(rgba)
        }
        _ => None,
    }
}

fn apply_invert(rgba: &mut [u8], invert: &[bool], width: usize, height: usize) {
    if !invert.iter().any(|&i| i) {
        return;
    }
    for y in 0..height {
        for x in 0..width {
            if !invert[y * width + x] {
                continue;
            }
            let d = (y * width + x) * 4;
            rgba[d..d + 4].copy_from_slice(&[0, 0, 0, 255]);
            for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                    continue;
                }
                let n = (ny as usize * width + nx as usize) * 4;
                if rgba[n + 3] == 0 {
                    rgba[n..n + 4].copy_from_slice(&[255, 255, 255, 230]);
                }
            }
        }
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
