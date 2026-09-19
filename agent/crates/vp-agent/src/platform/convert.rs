//! GPU color conversion and scaling (BGRA → NV12, BT.709 limited range) with the D3D11 video
//! processor. Output textures rotate through a small pool because asynchronous hardware encoders
//! may still hold the previous surface.

use super::gpu::Gpu;
use windows::core::Interface;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoContext1, ID3D11VideoDevice,
    ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView,
    ID3D11VideoProcessorOutputView, D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_OPTIMAL_SPEED, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
    DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};

const POOL: usize = 4;

pub struct Nv12Converter {
    video_context: ID3D11VideoContext,
    processor: ID3D11VideoProcessor,
    enumerator: ID3D11VideoProcessorEnumerator,
    video_device: ID3D11VideoDevice,
    input: Option<(usize, ID3D11VideoProcessorInputView)>,
    outputs: Vec<(ID3D11Texture2D, ID3D11VideoProcessorOutputView)>,
    next: usize,
    staging: Option<ID3D11Texture2D>,
    context: windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    pub out_width: u32,
    pub out_height: u32,
    pub in_width: u32,
    pub in_height: u32,
}

unsafe impl Send for Nv12Converter {}

impl Nv12Converter {
    pub fn new(
        gpu: &Gpu,
        in_width: u32,
        in_height: u32,
        out_width: u32,
        out_height: u32,
        fps: u32,
    ) -> windows::core::Result<Nv12Converter> {
        unsafe {
            let video_device: ID3D11VideoDevice = gpu.device.cast()?;
            let video_context: ID3D11VideoContext = gpu.context.cast()?;
            let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: DXGI_RATIONAL {
                    Numerator: fps,
                    Denominator: 1,
                },
                InputWidth: in_width,
                InputHeight: in_height,
                OutputFrameRate: DXGI_RATIONAL {
                    Numerator: fps,
                    Denominator: 1,
                },
                OutputWidth: out_width,
                OutputHeight: out_height,
                Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
            };
            let enumerator = video_device.CreateVideoProcessorEnumerator(&content)?;
            let processor = video_device.CreateVideoProcessor(&enumerator, 0)?;

            video_context.VideoProcessorSetStreamFrameFormat(
                &processor,
                0,
                D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            );
            video_context.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);
            let source = RECT {
                left: 0,
                top: 0,
                right: in_width as i32,
                bottom: in_height as i32,
            };
            let dest = RECT {
                left: 0,
                top: 0,
                right: out_width as i32,
                bottom: out_height as i32,
            };
            video_context.VideoProcessorSetStreamSourceRect(&processor, 0, true, Some(&source));
            video_context.VideoProcessorSetStreamDestRect(&processor, 0, true, Some(&dest));
            video_context.VideoProcessorSetOutputTargetRect(&processor, true, Some(&dest));
            if let Ok(context1) = video_context.cast::<ID3D11VideoContext1>() {
                context1.VideoProcessorSetStreamColorSpace1(
                    &processor,
                    0,
                    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
                );
                context1.VideoProcessorSetOutputColorSpace1(
                    &processor,
                    DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
                );
            }

            let mut outputs = Vec::with_capacity(POOL);
            for _ in 0..POOL {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: out_width,
                    Height: out_height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_NV12,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };
                let mut texture = None;
                gpu.device
                    .CreateTexture2D(&desc, None, Some(&mut texture))?;
                let texture = texture.expect("texture");
                let view_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                    ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                    Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                        Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                    },
                };
                let mut view = None;
                video_device.CreateVideoProcessorOutputView(
                    &texture,
                    &enumerator,
                    &view_desc,
                    Some(&mut view),
                )?;
                outputs.push((texture, view.expect("output view")));
            }

            Ok(Nv12Converter {
                video_context,
                processor,
                enumerator,
                video_device,
                input: None,
                outputs,
                next: 0,
                staging: None,
                context: gpu.context.clone(),
                out_width,
                out_height,
                in_width,
                in_height,
            })
        }
    }

    /// Converts the BGRA source into the next pooled NV12 texture and returns it.
    pub fn convert(&mut self, source: &ID3D11Texture2D) -> windows::core::Result<ID3D11Texture2D> {
        unsafe {
            let key = source.as_raw() as usize;
            if self.input.as_ref().map(|(k, _)| *k) != Some(key) {
                let desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                    FourCC: 0,
                    ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                    Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                        Texture2D: D3D11_TEX2D_VPIV {
                            MipSlice: 0,
                            ArraySlice: 0,
                        },
                    },
                };
                let mut view = None;
                self.video_device.CreateVideoProcessorInputView(
                    source,
                    &self.enumerator,
                    &desc,
                    Some(&mut view),
                )?;
                self.input = Some((key, view.expect("input view")));
            }
            let (_, input_view) = self.input.as_ref().unwrap();
            let (texture, output_view) = &self.outputs[self.next];
            self.next = (self.next + 1) % self.outputs.len();

            let stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                pInputSurface: std::mem::ManuallyDrop::new(Some(input_view.clone())),
                ..Default::default()
            };
            let streams = [stream];
            let result =
                self.video_context
                    .VideoProcessorBlt(&self.processor, output_view, 0, &streams);
            // Release the reference held by the stream descriptor.
            let [mut stream] = streams;
            std::mem::ManuallyDrop::drop(&mut stream.pInputSurface);
            result?;
            Ok(texture.clone())
        }
    }

    /// Copies an NV12 texture to CPU memory (software encoder path). Returns (Y, UV) planes.
    pub fn read_nv12(
        &mut self,
        gpu: &Gpu,
        texture: &ID3D11Texture2D,
        y_plane: &mut Vec<u8>,
        uv_plane: &mut Vec<u8>,
    ) -> windows::core::Result<()> {
        unsafe {
            if self.staging.is_none() {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: self.out_width,
                    Height: self.out_height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_NV12,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                };
                let mut staging = None;
                gpu.device
                    .CreateTexture2D(&desc, None, Some(&mut staging))?;
                self.staging = staging;
            }
            let staging = self.staging.as_ref().unwrap();
            self.context.CopyResource(staging, texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let w = self.out_width as usize;
            let h = self.out_height as usize;
            let pitch = mapped.RowPitch as usize;
            let base = mapped.pData as *const u8;
            y_plane.resize(w * h, 0);
            uv_plane.resize(w * (h / 2), 0);
            for row in 0..h {
                std::ptr::copy_nonoverlapping(
                    base.add(row * pitch),
                    y_plane.as_mut_ptr().add(row * w),
                    w,
                );
            }
            for row in 0..h / 2 {
                std::ptr::copy_nonoverlapping(
                    base.add((h + row) * pitch),
                    uv_plane.as_mut_ptr().add(row * w),
                    w,
                );
            }
            self.context.Unmap(staging, 0);
            Ok(())
        }
    }
}
