//! D3D11 device creation and display (output) enumeration.

use windows::core::{Interface, PCWSTR};
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{HMODULE, RECT};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput1, DXGI_ADAPTER_FLAG_SOFTWARE,
};
use windows::Win32::Graphics::Gdi::{EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS};

pub struct Gpu {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub adapter: IDXGIAdapter1,
    pub luid: (u32, i32),
    pub name: String,
    pub vendor_id: u32,
}

// COM pointers are only used from the owning pipeline thread; the device is multithread-protected.
unsafe impl Send for Gpu {}

impl Gpu {
    pub fn for_adapter(adapter: &IDXGIAdapter1) -> anyhow::Result<Gpu> {
        unsafe {
            let desc = adapter.GetDesc1()?;
            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(
                adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
            let device = device.ok_or_else(|| anyhow::anyhow!("no D3D11 device"))?;
            let context = context.ok_or_else(|| anyhow::anyhow!("no D3D11 context"))?;
            let multithread: ID3D11Multithread = device.cast()?;
            let _ = multithread.SetMultithreadProtected(true);
            Ok(Gpu {
                device,
                context,
                adapter: adapter.clone(),
                luid: (desc.AdapterLuid.LowPart, desc.AdapterLuid.HighPart),
                name: wide_to_string(&desc.Description),
                vendor_id: desc.VendorId,
            })
        }
    }
}

#[derive(Clone)]
pub struct Output {
    pub id: String,
    pub name: String,
    pub device_name: String,
    pub adapter: IDXGIAdapter1,
    pub output: IDXGIOutput1,
    pub rect: RECT,
    pub refresh_hz: u32,
    pub primary: bool,
}

unsafe impl Send for Output {}

impl Output {
    pub fn width(&self) -> u32 {
        (self.rect.right - self.rect.left).max(0) as u32
    }
    pub fn height(&self) -> u32 {
        (self.rect.bottom - self.rect.top).max(0) as u32
    }
}

pub fn wide_to_string(wide: &[u16]) -> String {
    let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..end])
}

/// GDI device name (\\.\DISPLAY1) → monitor friendly name ("DELL U2720Q").
fn friendly_names() -> std::collections::HashMap<String, String> {
    let mut names = std::collections::HashMap::new();
    unsafe {
        let mut path_count = 0u32;
        let mut mode_count = 0u32;
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
            .is_err()
        {
            return names;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
        .is_err()
        {
            return names;
        }
        for path in paths.iter().take(path_count as usize) {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            source.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            source.header.adapterId = path.sourceInfo.adapterId;
            source.header.id = path.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut source.header) != 0 {
                continue;
            }
            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            target.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            target.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            target.header.adapterId = path.targetInfo.adapterId;
            target.header.id = path.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut target.header) != 0 {
                continue;
            }
            let friendly = wide_to_string(&target.monitorFriendlyDeviceName);
            if !friendly.is_empty() {
                names.insert(wide_to_string(&source.viewGdiDeviceName), friendly);
            }
        }
    }
    names
}

fn refresh_rate(device_name: &str) -> u32 {
    let wide: Vec<u16> = device_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut mode = DEVMODEW {
        dmSize: std::mem::size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    unsafe {
        if EnumDisplaySettingsW(PCWSTR(wide.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode).as_bool()
            && mode.dmDisplayFrequency > 1
        {
            mode.dmDisplayFrequency
        } else {
            60
        }
    }
}

/// Every display attached to the desktop, on hardware adapters, primary first.
pub fn outputs() -> Vec<Output> {
    let mut list = vec![];
    let names = friendly_names();
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return list;
        };
        let mut adapter_index = 0;
        while let Ok(adapter) = factory.EnumAdapters1(adapter_index) {
            adapter_index += 1;
            let Ok(desc) = adapter.GetDesc1() else {
                continue;
            };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let mut output_index = 0;
            while let Ok(output) = adapter.EnumOutputs(output_index) {
                output_index += 1;
                let Ok(out_desc) = output.GetDesc() else {
                    continue;
                };
                if !out_desc.AttachedToDesktop.as_bool() {
                    continue;
                }
                let Ok(output1) = output.cast::<IDXGIOutput1>() else {
                    continue;
                };
                let device_name = wide_to_string(&out_desc.DeviceName);
                let rect = out_desc.DesktopCoordinates;
                let primary = rect.left == 0 && rect.top == 0;
                let index = list.len() + 1;
                list.push(Output {
                    id: device_name.trim_start_matches("\\\\.\\").to_string(),
                    name: names
                        .get(&device_name)
                        .cloned()
                        .unwrap_or_else(|| format!("Display {index}")),
                    refresh_hz: refresh_rate(&device_name),
                    device_name,
                    adapter: adapter.clone(),
                    output: output1,
                    rect,
                    primary,
                });
            }
        }
    }
    list.sort_by_key(|o| !o.primary);
    list
}

/// Bounding rectangle of the whole virtual desktop.
pub fn virtual_desktop() -> RECT {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    unsafe {
        let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        RECT {
            left,
            top,
            right: left + GetSystemMetrics(SM_CXVIRTUALSCREEN),
            bottom: top + GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}
