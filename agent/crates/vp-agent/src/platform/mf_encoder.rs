//! Hardware video encoding through Media Foundation (NVIDIA NVENC, Intel Quick Sync, AMD AMF all
//! expose MFTs). Frames are submitted as D3D11 textures — no CPU copy — with low-latency CBR,
//! no B-frames and on-demand IDR frames.

use super::gpu::Gpu;
use crate::config::CodecId;
use crate::media::codec::EncoderCapability;
use std::sync::Once;
use windows::core::{Interface, GUID, PWSTR};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};
use windows::Win32::System::Variant::VARIANT;

static MF_STARTUP: Once = Once::new();

pub fn ensure_media_foundation() {
    MF_STARTUP.call_once(|| unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if let Err(error) = MFStartup(MF_VERSION, MFSTARTUP_LITE) {
            tracing::error!(%error, "Media Foundation startup failed");
        }
    });
}

pub fn com_init_thread() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn subtype(codec: CodecId) -> GUID {
    match codec {
        CodecId::H264 => MFVideoFormat_H264,
        CodecId::Hevc => MFVideoFormat_HEVC,
        CodecId::Av1 => MFVideoFormat_AV1,
    }
}

struct Activates {
    ptr: *mut Option<IMFActivate>,
    count: u32,
}

impl Activates {
    fn find(codec: CodecId) -> Activates {
        ensure_media_foundation();
        let input = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_NV12,
        };
        let output = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: subtype(codec),
        };
        let mut ptr = std::ptr::null_mut();
        let mut count = 0u32;
        unsafe {
            if MFTEnumEx(
                MFT_CATEGORY_VIDEO_ENCODER,
                MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
                Some(&input),
                Some(&output),
                &mut ptr,
                &mut count,
            )
            .is_err()
            {
                return Activates {
                    ptr: std::ptr::null_mut(),
                    count: 0,
                };
            }
        }
        Activates { ptr, count }
    }

    fn iter(&self) -> impl Iterator<Item = &IMFActivate> {
        (0..self.count as usize).filter_map(move |i| unsafe { (*self.ptr.add(i)).as_ref() })
    }
}

impl Drop for Activates {
    fn drop(&mut self) {
        unsafe {
            for i in 0..self.count as usize {
                std::ptr::drop_in_place(self.ptr.add(i));
            }
            if !self.ptr.is_null() {
                CoTaskMemFree(Some(self.ptr as *const _));
            }
        }
    }
}

fn friendly_name(activate: &IMFActivate) -> String {
    unsafe {
        let mut value = PWSTR::null();
        let mut length = 0u32;
        if activate
            .GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut value, &mut length)
            .is_ok()
            && !value.is_null()
        {
            let name = value.to_string().unwrap_or_default();
            CoTaskMemFree(Some(value.0 as *const _));
            return name;
        }
    }
    "Hardware encoder".into()
}

fn adapter_luid(activate: &IMFActivate) -> Option<(u32, i32)> {
    unsafe {
        let mut blob = [0u8; 8];
        let mut size = 0u32;
        activate
            .GetBlob(&MFT_ENUM_ADAPTER_LUID, &mut blob, Some(&mut size))
            .ok()?;
        if size != 8 {
            return None;
        }
        Some((
            u32::from_le_bytes(blob[0..4].try_into().unwrap()),
            i32::from_le_bytes(blob[4..8].try_into().unwrap()),
        ))
    }
}

/// Hardware encoders available on this PC (optionally restricted to one GPU).
pub fn probe(luid: Option<(u32, i32)>) -> Vec<EncoderCapability> {
    let mut list = vec![];
    for codec in [CodecId::H264, CodecId::Hevc, CodecId::Av1] {
        let activates = Activates::find(codec);
        let name = activates
            .iter()
            .find(|a| luid.is_none() || adapter_luid(a).is_none() || adapter_luid(a) == luid)
            .map(friendly_name);
        if let Some(name) = name {
            list.push(EncoderCapability {
                codec,
                hardware: true,
                name,
            });
        }
    }
    list
}

pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub keyframe: bool,
    pub timestamp_100ns: i64,
}

pub struct MfEncoder {
    transform: IMFTransform,
    events: Option<IMFMediaEventGenerator>,
    codec_api: Option<ICodecAPI>,
    _manager: IMFDXGIDeviceManager,
    input_id: u32,
    output_id: u32,
    provides_samples: bool,
    output_buffer_size: u32,
    pending_inputs: u32,
    /// Inputs submitted whose encoded output has not been collected yet.
    in_flight: u32,
    /// The last frame was not submitted because the asynchronous encoder was not ready.
    input_dropped: bool,
    last_input: Option<std::time::Instant>,
    pub name: String,
    pub codec: CodecId,
    pub width: u32,
    pub height: u32,
    fps: u32,
    bitrate_kbps: u32,
    sequence_header: Vec<u8>,
}

unsafe impl Send for MfEncoder {}

fn set_codec_value(api: &ICodecAPI, key: &GUID, value: VARIANT) -> bool {
    unsafe { api.SetValue(key, &value).is_ok() }
}

/// Media Foundation expresses this setting in bits.  A quarter-second VBV is
/// long enough for CBR hardware encoders to remain stable, while preventing a
/// whole second of already obsolete desktop video from sitting in the encoder.
fn low_latency_buffer_size(kbps: u32) -> u32 {
    kbps.saturating_mul(250).max(250_000)
}

#[cfg(test)]
mod tests {
    use super::low_latency_buffer_size;

    #[test]
    fn vbv_window_is_short_and_has_a_safe_floor() {
        assert_eq!(low_latency_buffer_size(2_000), 500_000);
        assert_eq!(low_latency_buffer_size(200_000), 50_000_000);
        assert_eq!(low_latency_buffer_size(100), 250_000);
    }
}

impl MfEncoder {
    pub fn new(
        gpu: &Gpu,
        codec: CodecId,
        width: u32,
        height: u32,
        fps: u32,
        bitrate_kbps: u32,
        low_latency_quality: u32,
    ) -> anyhow::Result<MfEncoder> {
        ensure_media_foundation();
        let activates = Activates::find(codec);
        let candidates: Vec<&IMFActivate> = activates
            .iter()
            .filter(|a| adapter_luid(a).is_none_or(|l| l == gpu.luid))
            .collect();
        anyhow::ensure!(
            !candidates.is_empty(),
            "no {} hardware encoder on {}",
            codec.display(),
            gpu.name
        );
        let mut last_error = None;
        for activate in candidates {
            match unsafe {
                Self::create(
                    gpu,
                    activate,
                    codec,
                    width,
                    height,
                    fps,
                    bitrate_kbps,
                    low_latency_quality,
                )
            } {
                Ok(encoder) => return Ok(encoder),
                Err(error) => {
                    tracing::warn!(%error, name = %friendly_name(activate), "hardware encoder rejected configuration");
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("no usable encoder")))
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn create(
        gpu: &Gpu,
        activate: &IMFActivate,
        codec: CodecId,
        width: u32,
        height: u32,
        fps: u32,
        bitrate_kbps: u32,
        quality: u32,
    ) -> anyhow::Result<MfEncoder> {
        unsafe {
            let name = friendly_name(activate);
            let transform: IMFTransform = activate.ActivateObject()?;
            let attributes = transform.GetAttributes()?;
            let is_async = attributes.GetUINT32(&MF_TRANSFORM_ASYNC).unwrap_or(0) != 0;
            if is_async {
                attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)?;
            }
            let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);

            let mut reset_token = 0u32;
            let mut manager = None;
            MFCreateDXGIDeviceManager(&mut reset_token, &mut manager)?;
            let manager = manager.ok_or_else(|| anyhow::anyhow!("no DXGI device manager"))?;
            manager.ResetDevice(&gpu.device, reset_token)?;
            transform.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)?;

            let (mut input_id, mut output_id) = (0u32, 0u32);
            {
                let mut inputs = [0u32; 1];
                let mut outputs = [0u32; 1];
                if transform.GetStreamIDs(&mut inputs, &mut outputs).is_ok() {
                    input_id = inputs[0];
                    output_id = outputs[0];
                }
            }

            let frame_size = ((width as u64) << 32) | height as u64;
            let frame_rate = ((fps as u64) << 32) | 1;

            let output_type = MFCreateMediaType()?;
            output_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            output_type.SetGUID(&MF_MT_SUBTYPE, &subtype(codec))?;
            output_type.SetUINT32(&MF_MT_AVG_BITRATE, bitrate_kbps.saturating_mul(1000))?;
            output_type.SetUINT64(&MF_MT_FRAME_SIZE, frame_size)?;
            output_type.SetUINT64(&MF_MT_FRAME_RATE, frame_rate)?;
            output_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1)?;
            output_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            match codec {
                CodecId::H264 => {
                    output_type
                        .SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32)?;
                }
                CodecId::Hevc => {
                    output_type
                        .SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH265VProfile_Main_420_8.0 as u32)?;
                }
                CodecId::Av1 => {}
            }
            let _ =
                output_type.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32);
            let _ = output_type.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32);
            let _ = output_type.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32);
            let _ = output_type.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32);
            transform.SetOutputType(output_id, &output_type, 0)?;

            let mut input_set = false;
            for index in 0..32 {
                let Ok(candidate) = transform.GetInputAvailableType(input_id, index) else {
                    break;
                };
                if candidate.GetGUID(&MF_MT_SUBTYPE).ok() == Some(MFVideoFormat_NV12) {
                    candidate.SetUINT64(&MF_MT_FRAME_SIZE, frame_size)?;
                    candidate.SetUINT64(&MF_MT_FRAME_RATE, frame_rate)?;
                    candidate
                        .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                    let _ = candidate
                        .SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32);
                    transform.SetInputType(input_id, &candidate, 0)?;
                    input_set = true;
                    break;
                }
            }
            anyhow::ensure!(input_set, "encoder does not accept NV12 input");

            let codec_api: Option<ICodecAPI> = transform.cast().ok();
            if let Some(api) = &codec_api {
                set_codec_value(api, &CODECAPI_AVLowLatencyMode, VARIANT::from(true));
                // Prefer the driver's real-time scheduling path.  Not every
                // MFT exposes this control, so a rejected value is harmless.
                set_codec_value(api, &CODECAPI_AVEncCommonRealTime, VARIANT::from(true));
                set_codec_value(
                    api,
                    &CODECAPI_AVEncCommonRateControlMode,
                    VARIANT::from(eAVEncCommonRateControlMode_CBR.0 as u32),
                );
                set_codec_value(
                    api,
                    &CODECAPI_AVEncCommonMeanBitRate,
                    VARIANT::from(bitrate_kbps.saturating_mul(1000)),
                );
                // Bound the encoder's own queue.  A one-second VBV is
                // perceptibly stale for direct interaction on an iPad.
                set_codec_value(
                    api,
                    &CODECAPI_AVEncCommonBufferSize,
                    VARIANT::from(low_latency_buffer_size(bitrate_kbps)),
                );
                set_codec_value(
                    api,
                    &CODECAPI_AVEncMPVDefaultBPictureCount,
                    VARIANT::from(0u32),
                );
                set_codec_value(
                    api,
                    &CODECAPI_AVEncMPVGOPSize,
                    VARIANT::from(fps.saturating_mul(30)),
                );
                set_codec_value(
                    api,
                    &CODECAPI_AVEncCommonQualityVsSpeed,
                    VARIANT::from(quality.min(100)),
                );
            }

            let stream_info = transform.GetOutputStreamInfo(output_id).unwrap_or_default();
            let provides_samples =
                stream_info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;

            transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;

            let sequence_header = transform
                .GetOutputCurrentType(output_id)
                .ok()
                .and_then(|t| {
                    let size = t.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER).ok()?;
                    let mut buf = vec![0u8; size as usize];
                    t.GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut buf, None)
                        .ok()?;
                    Some(buf)
                })
                .unwrap_or_default();

            Ok(MfEncoder {
                events: if is_async {
                    transform.cast().ok()
                } else {
                    None
                },
                transform,
                codec_api,
                _manager: manager,
                input_id,
                output_id,
                provides_samples,
                output_buffer_size: stream_info.cbSize.max(1024 * 1024),
                pending_inputs: 0,
                in_flight: 0,
                input_dropped: false,
                last_input: None,
                name,
                codec,
                width,
                height,
                fps,
                bitrate_kbps,
                sequence_header,
            })
        }
    }

    pub fn set_bitrate(&mut self, kbps: u32) {
        if kbps == self.bitrate_kbps {
            return;
        }
        self.bitrate_kbps = kbps;
        if let Some(api) = &self.codec_api {
            set_codec_value(
                api,
                &CODECAPI_AVEncCommonMeanBitRate,
                VARIANT::from(kbps.saturating_mul(1000)),
            );
            // Keep the encoder VBV window short and aligned with the adaptive
            // bitrate. Leaving either the former 200 Mb/s buffer or a full
            // second of video in place would make interaction feel delayed.
            set_codec_value(
                api,
                &CODECAPI_AVEncCommonBufferSize,
                VARIANT::from(low_latency_buffer_size(kbps)),
            );
        }
    }

    pub fn bitrate_kbps(&self) -> u32 {
        self.bitrate_kbps
    }

    /// True while a recently submitted frame has not come out of the encoder yet. An output that
    /// never arrives (a driver discarding a frame) stops counting after 750 ms.
    pub fn awaiting_output(&self) -> bool {
        self.in_flight > 0
            && self
                .last_input
                .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(750))
    }

    /// True if the last `encode` could not submit its frame (the encoder was still busy).
    pub fn input_dropped(&self) -> bool {
        self.input_dropped
    }

    /// Collects output that completed since the last call, without submitting a frame.
    /// Hardware encoders finish a few milliseconds after `encode` returns; waiting for the next
    /// capture to collect it delayed the last change on screen until the idle refresh (500 ms).
    pub fn poll(&mut self) -> anyhow::Result<Vec<EncodedFrame>> {
        if self.in_flight == 0 {
            return Ok(vec![]);
        }
        let mut frames = vec![];
        unsafe {
            match self.events.clone() {
                Some(events) => self.drain_events(&events, &mut frames)?,
                None => {
                    if let Some(frame) = self.drain_output()? {
                        frames.push(frame);
                    }
                }
            }
        }
        Ok(frames)
    }

    /// Submits one NV12 texture and returns the encoded access units (Annex B) now available,
    /// oldest first. Asynchronous encoders may return the previous frame, or nothing yet.
    pub fn encode(
        &mut self,
        texture: &ID3D11Texture2D,
        timestamp_100ns: i64,
        force_keyframe: bool,
    ) -> anyhow::Result<Vec<EncodedFrame>> {
        unsafe {
            if force_keyframe {
                if let Some(api) = &self.codec_api {
                    set_codec_value(api, &CODECAPI_AVEncVideoForceKeyFrame, VARIANT::from(1u32));
                }
            }
            let buffer = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, texture, 0, false)?;
            if let Ok(buffer2d) = buffer.cast::<IMF2DBuffer>() {
                if let Ok(length) = buffer2d.GetContiguousLength() {
                    let _ = buffer.SetCurrentLength(length);
                }
            }
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(timestamp_100ns)?;
            sample.SetSampleDuration(10_000_000 / self.fps.max(1) as i64)?;

            match self.events.clone() {
                Some(events) => self.encode_async(&events, &sample),
                None => self.encode_sync(&sample),
            }
        }
    }

    unsafe fn encode_sync(&mut self, sample: &IMFSample) -> anyhow::Result<Vec<EncodedFrame>> {
        unsafe {
            self.input_dropped = false;
            self.transform.ProcessInput(self.input_id, sample, 0)?;
            self.in_flight = (self.in_flight + 1).min(8);
            self.last_input = Some(std::time::Instant::now());
            Ok(self.drain_output()?.into_iter().collect())
        }
    }

    unsafe fn encode_async(
        &mut self,
        events: &IMFMediaEventGenerator,
        sample: &IMFSample,
    ) -> anyhow::Result<Vec<EncodedFrame>> {
        unsafe {
            // An asynchronous MFT is a pipeline: many drivers do not expose
            // the output from input N until input N+1 arrives.  Waiting here
            // for that output (the previous implementation waited 250 ms)
            // serializes the pipeline and can reduce a 60 FPS desktop to
            // 2–4 FPS.  Drain events already ready, submit at most one fresh
            // frame, then drain once more without ever sleeping. Output that
            // completes later is collected by `poll`.
            let mut output = vec![];
            self.drain_events(events, &mut output)?;
            if self.pending_inputs > 0 {
                self.transform.ProcessInput(self.input_id, sample, 0)?;
                self.pending_inputs -= 1;
                self.in_flight = (self.in_flight + 1).min(8);
                self.last_input = Some(std::time::Instant::now());
                self.input_dropped = false;
                self.drain_events(events, &mut output)?;
            } else {
                self.input_dropped = true;
            }
            Ok(output)
        }
    }

    /// Consume every Media Foundation event already available.  This is kept
    /// non-blocking so capture continues at the requested frame rate even
    /// when a driver has one frame of internal encoder latency.
    unsafe fn drain_events(
        &mut self,
        events: &IMFMediaEventGenerator,
        output: &mut Vec<EncodedFrame>,
    ) -> anyhow::Result<()> {
        unsafe {
            loop {
                match events.GetEvent(MF_EVENT_FLAG_NO_WAIT) {
                    Ok(event) => self.handle_event(&event, output)?,
                    Err(e) if e.code() == MF_E_NO_EVENTS_AVAILABLE => break,
                    Err(e) => return Err(e.into()),
                }
            }
            Ok(())
        }
    }

    unsafe fn handle_event(
        &mut self,
        event: &IMFMediaEvent,
        output: &mut Vec<EncodedFrame>,
    ) -> anyhow::Result<()> {
        unsafe {
            let kind = event.GetType()?;
            if kind == METransformNeedInput.0 as u32 {
                // Keep one current slot only.  Queueing samples inside the
                // driver adds latency and is unnecessary for screen sharing.
                self.pending_inputs = 1;
            } else if kind == METransformHaveOutput.0 as u32 {
                // Keep every access unit: dropping one breaks the decoder's reference chain
                // until the next keyframe.
                if let Some(frame) = self.drain_output()? {
                    output.push(frame);
                }
            } else if kind == MEError.0 as u32 {
                let status = event.GetStatus().unwrap_or_default();
                anyhow::bail!("encoder error {status:?}");
            }
            Ok(())
        }
    }

    unsafe fn drain_output(&mut self) -> anyhow::Result<Option<EncodedFrame>> {
        unsafe {
            let sample = if self.provides_samples {
                None
            } else {
                let sample = MFCreateSample()?;
                let buffer = MFCreateMemoryBuffer(self.output_buffer_size)?;
                sample.AddBuffer(&buffer)?;
                Some(sample)
            };
            let mut buffers = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: self.output_id,
                pSample: std::mem::ManuallyDrop::new(sample),
                dwStatus: 0,
                pEvents: std::mem::ManuallyDrop::new(None),
            }];
            let mut status = 0u32;
            let result = self.transform.ProcessOutput(0, &mut buffers, &mut status);
            let [mut buffer] = buffers;
            let sample = std::mem::ManuallyDrop::take(&mut buffer.pSample);
            std::mem::ManuallyDrop::drop(&mut buffer.pEvents);
            match result {
                Ok(()) => {}
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(None),
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    let new_type = self.transform.GetOutputAvailableType(self.output_id, 0)?;
                    self.transform.SetOutputType(self.output_id, &new_type, 0)?;
                    return Ok(None);
                }
                Err(e) => return Err(e.into()),
            }
            let Some(sample) = sample else {
                return Ok(None);
            };
            self.in_flight = self.in_flight.saturating_sub(1);
            let keyframe = sample.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) != 0;
            let timestamp = sample.GetSampleTime().unwrap_or(0);
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut ptr = std::ptr::null_mut();
            let mut length = 0u32;
            buffer.Lock(&mut ptr, None, Some(&mut length))?;
            let mut data = Vec::with_capacity(length as usize + self.sequence_header.len());
            let bytes = std::slice::from_raw_parts(ptr, length as usize);
            if keyframe
                && self.codec != CodecId::Av1
                && !self.sequence_header.is_empty()
                && !contains_parameter_sets(bytes, self.codec)
            {
                data.extend_from_slice(&self.sequence_header);
            }
            data.extend_from_slice(bytes);
            buffer.Unlock()?;
            Ok(Some(EncodedFrame {
                data,
                keyframe,
                timestamp_100ns: timestamp,
            }))
        }
    }
}

impl Drop for MfEncoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
        }
    }
}

/// True if the Annex B access unit already carries SPS (H.264 type 7) or VPS/SPS (HEVC 32/33).
fn contains_parameter_sets(data: &[u8], codec: CodecId) -> bool {
    let mut i = 0;
    while i + 3 < data.len() {
        if data[i] == 0
            && data[i + 1] == 0
            && (data[i + 2] == 1 || (data[i + 2] == 0 && i + 4 < data.len() && data[i + 3] == 1))
        {
            let start = if data[i + 2] == 1 { i + 3 } else { i + 4 };
            if start < data.len() {
                let found = match codec {
                    CodecId::H264 => data[start] & 0x1f == 7,
                    CodecId::Hevc => matches!((data[start] >> 1) & 0x3f, 32 | 33),
                    CodecId::Av1 => true,
                };
                if found {
                    return true;
                }
            }
            i = start;
        } else {
            i += 1;
        }
    }
    false
}
