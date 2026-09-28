//! Foreground application detection, sleep prevention, DPI awareness and real-time scheduling.

use std::sync::Mutex;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::System::Power::{
    SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcess, ProcessPowerThrottling,
    QueryFullProcessImageNameW, SetProcessInformation, SetThreadInformation, SetThreadPriority,
    ThreadPowerThrottling, PROCESS_NAME_WIN32, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
    PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_LIMITED_INFORMATION,
    THREAD_POWER_THROTTLING_CURRENT_VERSION, THREAD_POWER_THROTTLING_EXECUTION_SPEED,
    THREAD_POWER_THROTTLING_STATE, THREAD_PRIORITY_HIGHEST,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// Physical-pixel coordinates are required for capture and input mapping. Safe to call repeatedly.
pub fn enable_dpi_awareness() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// Lower-case executable name of the foreground window's process, e.g. `blender.exe`.
pub fn foreground_executable() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(process);
        result.ok()?;
        let path = String::from_utf16_lossy(&buffer[..size as usize]);
        path.rsplit(['\\', '/']).next().map(|s| s.to_lowercase())
    }
}

/// Built-in profile id for an executable (the iPad can still override with its own mapping).
pub fn profile_for(exe: &str) -> Option<&'static str> {
    match exe {
        "blender.exe" | "blender-launcher.exe" => Some("blender"),
        "maya.exe" | "mayabatch.exe" => Some("maya"),
        "zbrush.exe" | "zbrushcore.exe" => Some("zbrush"),
        "houdini.exe" | "houdinifx.exe" | "houdiniapprentice.exe" => Some("houdini"),
        "photoshop.exe" => Some("photoshop"),
        _ => None,
    }
}

/// Keeps the display and system awake while held.
pub struct WakeLock;

impl WakeLock {
    pub fn acquire() -> WakeLock {
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED);
        }
        WakeLock
    }
}

impl Drop for WakeLock {
    fn drop(&mut self) {
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

/// Streaming sessions currently holding [`RealtimeScheduling`].
static REALTIME_USERS: Mutex<usize> = Mutex::new(0);

/// Real-time scheduling for the whole process while a session streams.
///
/// Without it, Windows rounds every short wait up to its default 15.6 ms timer tick: the
/// 1 ms encoder output poll, the frame-rate limiter and the WebRTC pacer's timers all wake
/// late, adding latency to every frame. Windows 11 also puts a background process (Vision-Pro
/// usually runs from the tray) under EcoQoS — efficiency cores, reduced clocks — and may ignore
/// its timer requests. The guard opts out of both, and restores the system defaults when the
/// last session ends.
pub struct RealtimeScheduling;

impl RealtimeScheduling {
    pub fn acquire() -> RealtimeScheduling {
        unsafe {
            timeBeginPeriod(1);
        }
        let mut users = REALTIME_USERS.lock().unwrap_or_else(|e| e.into_inner());
        *users += 1;
        if *users == 1 {
            set_process_power_throttling(false);
        }
        RealtimeScheduling
    }
}

impl Drop for RealtimeScheduling {
    fn drop(&mut self) {
        {
            let mut users = REALTIME_USERS.lock().unwrap_or_else(|e| e.into_inner());
            *users = users.saturating_sub(1);
            if *users == 0 {
                set_process_power_throttling(true);
            }
        }
        unsafe {
            timeEndPeriod(1);
        }
    }
}

/// `system_managed == false` opts the process out of EcoQoS and makes Windows honour its timer
/// resolution even while it has no visible window; `true` hands both back to the system.
fn set_process_power_throttling(system_managed: bool) {
    let apply = |control_mask: u32| unsafe {
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: control_mask,
            StateMask: 0,
        };
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            &state as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
    if system_managed {
        let _ = apply(0);
        return;
    }
    let full =
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;
    // Windows 10 does not know the timer-resolution flag: keep the EcoQoS opt-out alone there.
    if let Err(error) = apply(full) {
        if let Err(fallback) = apply(PROCESS_POWER_THROTTLING_EXECUTION_SPEED) {
            tracing::debug!(%error, %fallback, "power throttling opt-out unavailable");
        }
    }
}

/// Raises the calling thread for latency-critical work (capture/encode, input injection) and
/// keeps it off EcoQoS. The thread spends almost all its time waiting, so it never starves
/// other work.
pub fn prioritize_current_thread() {
    unsafe {
        let thread = GetCurrentThread();
        if let Err(error) = SetThreadPriority(thread, THREAD_PRIORITY_HIGHEST) {
            tracing::debug!(%error, "thread priority unchanged");
        }
        let state = THREAD_POWER_THROTTLING_STATE {
            Version: THREAD_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: THREAD_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: 0,
        };
        let _ = SetThreadInformation(
            thread,
            ThreadPowerThrottling,
            &state as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<THREAD_POWER_THROTTLING_STATE>() as u32,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::profile_for;

    #[test]
    fn maps_supported_creative_executables() {
        assert_eq!(profile_for("blender.exe"), Some("blender"));
        assert_eq!(profile_for("maya.exe"), Some("maya"));
        assert_eq!(profile_for("zbrush.exe"), Some("zbrush"));
        assert_eq!(profile_for("houdinifx.exe"), Some("houdini"));
        assert_eq!(profile_for("notepad.exe"), None);
    }
}
