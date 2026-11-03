//! Build script.
//!
//! On macOS the AppKit/Metal part of the `mac_app` adapter is written in
//! Swift (see `src/scene/adapters/mac_app/native/MacDesktop.swift`).  It is
//! compiled into a static library and linked into the binary.
//!
//! On every other platform this script does nothing.

use std::path::PathBuf;
use std::process::Command;

/// The Swift source file of the macOS adapter.
const SWIFT_SOURCE: &str = "src/scene/adapters/mac_app/native/MacDesktop.swift";

/// The name of the static library (without the `lib` prefix and suffix).
const LIBRARY_NAME: &str = "lwe_mac_desktop";

fn main() {
    println!("cargo:rerun-if-changed={SWIFT_SOURCE}");
    println!("cargo:rerun-if-env-changed=SWIFT");

    // The build script runs on the host, the target is what matters.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "macos" {
        return;
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let source = PathBuf::from(SWIFT_SOURCE);
    let library = out_dir.join(format!("lib{LIBRARY_NAME}.a"));

    let swiftc = std::env::var("SWIFT").unwrap_or_else(|_| "swiftc".to_string());

    println!("compiling {} with {swiftc}", source.display());

    let status = Command::new(&swiftc)
        .arg("-O")
        .arg("-whole-module-optimization")
        .arg("-emit-library")
        .arg("-static")
        .arg("-o")
        .arg(&library)
        .arg(&source)
        .arg("-framework")
        .arg("AppKit")
        .arg("-framework")
        .arg("Metal")
        .arg("-framework")
        .arg("QuartzCore")
        .status()
        .unwrap_or_else(|e| panic!("failed to run {swiftc}: {e}"));

    assert!(status.success(), "{swiftc} failed with {status}");

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static={LIBRARY_NAME}");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=Metal");
    println!("cargo:rustc-link-lib=framework=QuartzCore");
}
