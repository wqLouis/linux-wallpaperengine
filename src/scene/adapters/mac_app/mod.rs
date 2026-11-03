//! macOS adapter (experimental).
//!
//! macOS has no layer shell either, but the window server does have a
//! "desktop" level: a window placed there is drawn above the desktop picture
//! and below the desktop icons.  The adapter creates such a window with
//! AppKit, backs it with a `CAMetalLayer` and renders the wallpaper into it
//! with wgpu's Metal backend.
//!
//! The AppKit part lives in `native/MacDesktop.swift`; it is compiled by
//! the `build.rs` of the crate and called through a small C ABI.
//!
//! ## Limitations
//!
//! * Only the primary screen is covered.
//! * The window is created once; it is not moved when the screen layout
//!   changes.
//! * The pointer position is not tracked, so the depth parallax effects are
//!   not available.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::time::Instant;

use log;
use pollster::block_on;
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, RawDisplayHandle, RawWindowHandle,
};

use crate::scene::renderer::app::{InitAppSurface, WgpuApp};

use super::{FitMode, fit_scale};

// The Swift side of the adapter (see `native/MacDesktop.swift`).
unsafe extern "C" {
    // The size of the primary screen (in points) and its scale factor.
    fn lwe_mac_screen_size(width: *mut f64, height: *mut f64, scale: *mut f64);

    // Create the desktop window and return the view that hosts the Metal
    // layer.  Must be called from the main thread.
    fn lwe_mac_create_window(width: f64, height: f64) -> *mut c_void;

    // Process the pending AppKit events.
    fn lwe_mac_pump_events();

    // Close the desktop window again.
    fn lwe_mac_destroy_window();
}

/// Closes the desktop window when the adapter stops.
struct DesktopWindow;

impl Drop for DesktopWindow {
    fn drop(&mut self) {
        unsafe { lwe_mac_destroy_window() };
    }
}

/// Start the wallpaper engine with the macOS backend.
///
/// Creates an AppKit window in the desktop layer, renders the scene into its
/// Metal layer and keeps the AppKit event loop going.
pub fn start(
    pkg_path: String,
    fit_mode: FitMode,
    no_effects: bool,
    no_mdl: bool,
    assets_path: Option<String>,
    target_fps: Option<u32>,
    show_progress: bool,
) {
    // The size of the screen in points and its scale factor.
    let (mut width, mut height, mut scale) = (0.0f64, 0.0f64, 1.0f64);
    unsafe { lwe_mac_screen_size(&mut width, &mut height, &mut scale) };

    if width <= 0.0 || height <= 0.0 {
        eprintln!("error: cannot determine the size of the screen");
        return;
    }

    log::info!(
        "mac: screen {}x{} (scale {})",
        width as u32,
        height as u32,
        scale
    );

    // The window has to be created on the main thread.
    let view = unsafe { lwe_mac_create_window(width, height) };
    if view.is_null() {
        eprintln!("error: cannot create the desktop window");
        return;
    }

    // Closes the window when the adapter stops.
    let _window = DesktopWindow;

    let display_handle = RawDisplayHandle::AppKit(AppKitDisplayHandle::new());
    let window_handle = RawWindowHandle::AppKit(AppKitWindowHandle::new(
        NonNull::new(view).expect("the view pointer is not null"),
    ));

    // The Metal layer draws in device pixels.
    let size = [
        (width * scale).round() as u32,
        (height * scale).round() as u32,
    ];

    let mut app = block_on(WgpuApp::new(
        pkg_path,
        InitAppSurface::Raw((display_handle, window_handle)),
        size,
        no_effects,
        no_mdl,
        assets_path,
        show_progress,
    ));
    app.load();

    // The fit mode is applied to the camera projection.
    if let Some(scene) = app.resolution {
        let (sx, sy) = fit_scale(fit_mode, scene, size);
        log::info!("fit mode {:?}: ndc scale {}x{}", fit_mode, sx, sy);
        app.set_view_scale(sx, sy);
    }

    let mut last_frame: Option<Instant> = None;
    let mut frame_count: u64 = 0;

    loop {
        // ── FPS limiting ──────────────────────────────────────────────
        if let Some(target_fps) = target_fps
            && target_fps > 0
            && let Some(last) = last_frame
        {
            let min_delta = std::time::Duration::from_secs_f64(1.0 / target_fps as f64);
            if let Some(remaining) = min_delta.checked_sub(last.elapsed()) {
                std::thread::sleep(remaining);
            }
        }
        last_frame = Some(Instant::now());

        // ── Render ────────────────────────────────────────────────────
        log::trace!("frame {}: rendering...", frame_count);
        if app.render().is_none() {
            log::warn!("frame {}: render returned None", frame_count);
        }
        frame_count = frame_count.wrapping_add(1);

        // The Rust loop is not an AppKit run loop, so the events have to be
        // pumped by hand.
        unsafe { lwe_mac_pump_events() };
    }
}
