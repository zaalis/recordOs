//! Foreground application detection, computer name and sleep prevention.

use super::ffi;

/// The application in front, identified by its lower-case bundle identifier
/// (e.g. `org.blenderfoundation.blender`).
pub fn foreground_app() -> Option<String> {
    let mut bundle = vec![0 as std::ffi::c_char; 256];
    let mut name = vec![0 as std::ffi::c_char; 256];
    let found = unsafe {
        ffi::vp_frontmost_app(
            bundle.as_mut_ptr(),
            bundle.len(),
            name.as_mut_ptr(),
            name.len(),
        )
    };
    if !found {
        return None;
    }
    let bundle = ffi::c_buffer_to_string(&bundle).to_lowercase();
    (!bundle.is_empty()).then_some(bundle)
}

/// Built-in profile id for a bundle identifier (the iPad can still override with its own mapping).
/// Versioned bundles (`com.autodesk.Maya.2025`, `com.adobe.Photoshop` …) match by prefix.
pub fn profile_for(bundle_id: &str) -> Option<&'static str> {
    const PROFILES: &[(&str, &str)] = &[
        ("org.blenderfoundation.blender", "blender"),
        ("com.autodesk.maya", "maya"),
        ("com.pixologic.zbrush", "zbrush"),
        ("com.maxon.zbrush", "zbrush"),
        ("com.sidefx.houdini", "houdini"),
        ("com.adobe.photoshop", "photoshop"),
    ];
    let id = bundle_id.to_ascii_lowercase();
    PROFILES
        .iter()
        .find(|(prefix, _)| id.starts_with(prefix))
        .map(|(_, profile)| *profile)
}

/// "Bryan's MacBook Pro", as set in System Settings → General → About.
pub fn computer_name() -> Option<String> {
    ffi::read_string(256, |out, len| unsafe { ffi::vp_computer_name(out, len) })
}

/// Keeps the display and the Mac awake while held (IOKit power assertion).
pub struct WakeLock(u32);

impl WakeLock {
    pub fn acquire() -> WakeLock {
        let assertion = unsafe { ffi::vp_wake_acquire() };
        if assertion == 0 {
            tracing::warn!("could not prevent sleep during the session");
        }
        WakeLock(assertion)
    }
}

impl Drop for WakeLock {
    fn drop(&mut self) {
        unsafe { ffi::vp_wake_release(self.0) }
    }
}

#[cfg(test)]
mod tests {
    use super::profile_for;

    #[test]
    fn maps_supported_creative_applications() {
        assert_eq!(
            profile_for("org.blenderfoundation.blender"),
            Some("blender")
        );
        assert_eq!(profile_for("com.autodesk.Maya.2025"), Some("maya"));
        assert_eq!(profile_for("com.maxon.zbrush"), Some("zbrush"));
        assert_eq!(profile_for("com.sidefx.houdini20.5"), Some("houdini"));
        assert_eq!(profile_for("com.adobe.Photoshop"), Some("photoshop"));
        assert_eq!(profile_for("com.apple.textedit"), None);
    }
}
