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

use crate::engine::runtime::EngineStatus;
use crate::scene::adapters::{AdapterConfig, RenderFailures};
use crate::scene::error::EngineError;
use crate::scene::renderer::app::{InitAppSurface, WgpuApp};

use super::fit_scale;

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

/// Sends the status of the wallpaper to the host.
///
/// The problems that appeared since the last status are sent with it: a
/// problem is reported once, never twice.  `force` sends the status even
/// when there is nothing new to say: the first status tells the host that
/// the wallpaper is up.
fn report(status: &Option<std::sync::mpsc::Sender<EngineStatus>>, app: &mut WgpuApp, force: bool) {
    let problems = std::mem::take(&mut app.problems);
    if problems.is_empty() && !force {
        return;
    }

    let Some(status) = status else {
        return;
    };

    let _ = status.send(EngineStatus::Running {
        title: app.title.clone(),
        problems,
    });
}

/// Start the wallpaper engine with the macOS backend.
///
/// Creates an AppKit window in the desktop layer, renders the scene into its
/// Metal layer and keeps the AppKit event loop going.
///
/// Every failure is reported: the adapter hands the error that caused it
/// back to the caller.
pub fn start(config: AdapterConfig) -> Result<(), EngineError> {
    // The size of the screen in points and its scale factor.
    let (mut width, mut height, mut scale) = (0.0f64, 0.0f64, 1.0f64);
    unsafe { lwe_mac_screen_size(&mut width, &mut height, &mut scale) };

    if width <= 0.0 || height <= 0.0 {
        return Err(EngineError::Failed(
            "cannot determine the size of the screen",
        ));
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
        return Err(EngineError::Failed("cannot create the desktop window"));
    }

    // Closes the window when the adapter stops.
    let _window = DesktopWindow;

    let display_handle = RawDisplayHandle::AppKit(AppKitDisplayHandle::new());
    let window_handle = RawWindowHandle::AppKit(AppKitWindowHandle::new(
        NonNull::new(view).ok_or(EngineError::Failed("the desktop window is null"))?,
    ));

    // The Metal layer draws in device pixels.
    let size = [
        (width * scale).round() as u32,
        (height * scale).round() as u32,
    ];

    let mut app = block_on(WgpuApp::new(
        config.source.clone(),
        InitAppSurface::Raw((display_handle, window_handle)),
        size,
        config.no_effects,
        config.no_mdl,
        config.assets_path.clone(),
        config.show_progress,
    ))?;
    // The rate at which the wallpaper is rendered: the videos are played
    // back at that rate.
    app.target_fps = config.target_fps;
    app.load()?;

    // The fit mode is applied to the camera projection.
    if let Some(scene) = app.resolution {
        let (sx, sy) = fit_scale(config.fit_mode, scene, size);
        log::info!("fit mode {:?}: ndc scale {}x{}", config.fit_mode, sx, sy);
        app.set_view_scale(sx, sy);
    }

    // The wallpaper is up: the host is told about it, together with the
    // files that could not be used.
    report(&config.status, &mut app, true);

    let mut last_frame: Option<Instant> = None;
    let mut frame_count: u64 = 0;
    // The frames that could not be drawn: they are counted and reported
    // to the host.
    let mut failures = RenderFailures::new(config.status.clone());

    loop {
        // ── FPS limiting ──────────────────────────────────────────────
        if let Some(target_fps) = config.target_fps
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
        if let Err(error) = app.render() {
            // The frame that could not be drawn is counted and reported:
            // the same failure is reported once, the user does not have
            // to be told about every single frame that was skipped.
            failures.record(error, app.title.as_deref());
        }
        frame_count = frame_count.wrapping_add(1);

        // The problems of the wallpaper — the files that could not be
        // used, the effects that could not be drawn — are reported to the
        // host.  They are taken out of the list, so a problem is reported
        // once.
        report(&config.status, &mut app, false);

        // The Rust loop is not an AppKit run loop, so the events have to be
        // pumped by hand.
        unsafe { lwe_mac_pump_events() };
    }
}
