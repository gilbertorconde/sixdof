//! Sets `hid_route` where the crate reads the device over USB HID: always on
//! Windows and macOS, on Linux with the `hid` feature.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(hid_route)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let feature = std::env::var_os("CARGO_FEATURE_HID").is_some();
    if os == "windows" || os == "macos" || (os == "linux" && feature) {
        println!("cargo::rustc-cfg=hid_route");
    }
}
