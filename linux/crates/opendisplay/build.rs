//! Embed the application manifest (Common Controls v6, per-monitor DPI) with
//! the MSVC linker itself, so no Windows SDK resource compiler is needed.
fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "windows" && env == "msvc" {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{dir}/opendisplay.manifest");
    }
    println!("cargo:rerun-if-changed=opendisplay.manifest");
    println!("cargo:rerun-if-changed=build.rs");
}
