//! The iPad web app (apps/ipad/dist) is embedded at compile time (`server::WebAssets`). Cargo
//! does not see those files as inputs, so an installer rebuilt after an iPad-only change kept
//! serving the previous app. Declaring the folder makes any change to it recompile this crate.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../../apps/ipad/dist");
}
