//! Compiles the Objective-C bridge (native/vp_macos.m) and links the Apple frameworks it uses.

fn main() {
    println!("cargo:rerun-if-changed=native/vp_macos.m");
    println!("cargo:rerun-if-changed=native/vp_macos.h");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "macos" {
        panic!(
            "Vision-Pro for macOS builds only for macOS targets (aarch64-apple-darwin, x86_64-apple-darwin)."
        );
    }

    // ScreenCaptureKit's configuration surface used here requires macOS 13 Ventura.
    let deployment = std::env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| "13.0".into());
    cc::Build::new()
        .file("native/vp_macos.m")
        .include("native")
        .flag("-fobjc-arc")
        .flag("-fmodules")
        .flag(format!("-mmacosx-version-min={deployment}"))
        .flag_if_supported("-Wno-deprecated-declarations")
        .warnings(true)
        .compile("vp_macos");

    for framework in [
        "Foundation",
        "AppKit",
        "ApplicationServices",
        "CoreFoundation",
        "CoreGraphics",
        "CoreMedia",
        "CoreVideo",
        "IOKit",
        "ScreenCaptureKit",
        "Security",
        "SystemConfiguration",
        "VideoToolbox",
    ] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
