//! GNOME backend.
//!
//! GNOME Shell does not implement the wlr-layer-shell protocol, so a
//! wallpaper cannot be rendered into a compositor surface.  This backend
//! renders the scene into an off-screen texture instead and streams the
//! frames to a GNOME Shell extension (see the `gnome-extension/` folder),
//! which displays them as the desktop background.
//!
//! ## Protocol
//!
//! The extension owns the session bus name
//! `com.github.wqlouis.linux_wallpaper_engine` and exports the interface
//! `com.github.wqlouis.linux_wallpaper_engine` on the object
//! `/com/github/wqlouis/linux_wallpaper_engine`:
//!
//! | member         | direction | description                          |
//! |----------------|-----------|--------------------------------------|
//! | `GetOutput`    | app → ext | logical size + scale of the output    |
//! | `PushFrame`    | app → ext | path of a shared RGBA frame          |
//! | `SetVisible`   | app → ext | show / hide the background actor     |
//! | `OutputChanged`| ext → app | the output geometry changed          |
//! | `CursorMoved`  | ext → app | pointer position (normalised)        |
//!
//! The frames are exchanged through files in `/dev/shm`; only the path is
//! sent over D-Bus, which keeps the bus traffic small even for 4K frames.

mod frame;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use log;
use pollster::block_on;
use zbus::blocking::{Connection, Proxy};

use crate::scene::renderer::app::{InitAppSurface, WgpuApp};

use super::{FitMode, fit_scale};
use frame::FrameWriter;

/// The bus name owned by the GNOME Shell extension.
const EXTENSION_NAME: &str = "com.github.wqlouis.linux_wallpaper_engine";
/// The object path exported by the extension.
const EXTENSION_PATH: &str = "/com/github/wqlouis/linux_wallpaper_engine";
/// The interface exported by the extension.
const EXTENSION_INTERFACE: &str = "com.github.wqlouis.linux_wallpaper_engine";

/// How long to wait for the extension to appear on the bus.
const EXTENSION_TIMEOUT: Duration = Duration::from_secs(30);

/// The geometry of the output the wallpaper is rendered for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct OutputInfo {
    /// Logical size of the primary monitor.
    logical: [u32; 2],
    /// Scale factor of the primary monitor.
    scale: u32,
}

impl OutputInfo {
    /// The physical (device pixel) size of the primary monitor.
    fn physical(&self) -> [u32; 2] {
        [
            self.logical[0].saturating_mul(self.scale).max(1),
            self.logical[1].saturating_mul(self.scale).max(1),
        ]
    }
}

/// State shared between the render loop and the D-Bus listener thread.
#[derive(Default)]
struct SharedState {
    /// The output geometry reported by the extension.
    output: Mutex<Option<OutputInfo>>,
    /// The pointer position, normalised to [0, 1].
    cursor: Mutex<[f32; 2]>,
    /// Set to false when the extension went away.
    alive: AtomicBool,
}

impl SharedState {
    fn new() -> Self {
        Self {
            output: Mutex::new(None),
            cursor: Mutex::new([0.5, 0.5]),
            alive: AtomicBool::new(true),
        }
    }
}

/// Start the wallpaper engine with the GNOME backend.
///
/// Connects to the session bus, waits for the GNOME Shell extension, renders
/// the scene off-screen and pushes the frames to the extension.
pub fn start(
    pkg_path: String,
    fit_mode: FitMode,
    no_effects: bool,
    no_mdl: bool,
    assets_path: Option<String>,
    target_fps: Option<u32>,
    show_progress: bool,
) {
    let connection = match Connection::session() {
        Ok(connection) => connection,
        Err(e) => {
            eprintln!("error: cannot connect to the session bus: {}", e);
            return;
        }
    };

    if !wait_for_extension(&connection, EXTENSION_TIMEOUT) {
        eprintln!(
            "error: the GNOME Shell extension '{}' is not available.",
            EXTENSION_NAME
        );
        eprintln!("       install it with: gnome-extension/install.sh");
        eprintln!("       then log out and back in (Wayland) to activate it");
        return;
    }

    let proxy = match Proxy::new(
        &connection,
        EXTENSION_NAME,
        EXTENSION_PATH,
        EXTENSION_INTERFACE,
    ) {
        Ok(proxy) => proxy,
        Err(e) => {
            eprintln!("error: cannot reach the GNOME Shell extension: {}", e);
            return;
        }
    };

    // Ask the extension for the geometry of the output.
    let output = match proxy.call::<_, _, (u32, u32, u32)>("GetOutput", &()) {
        Ok((width, height, scale)) => OutputInfo {
            logical: [width.max(1), height.max(1)],
            scale: scale.max(1),
        },
        Err(e) => {
            eprintln!("error: cannot query the output geometry: {}", e);
            return;
        }
    };

    log::info!(
        "gnome: output {}x{} (scale {})",
        output.logical[0],
        output.logical[1],
        output.scale
    );

    let physical = output.physical();

    let mut app = block_on(WgpuApp::new(
        pkg_path,
        InitAppSurface::Offscreen {
            width: physical[0],
            height: physical[1],
        },
        physical,
        no_effects,
        no_mdl,
        assets_path,
        show_progress,
    ));
    app.load();

    // The fit mode is applied to the camera projection.
    apply_fit(&mut app, fit_mode, physical);

    let shared = Arc::new(SharedState::new());
    *shared.output.lock().unwrap() = Some(output);

    // Listen for the output changes and the pointer position.
    spawn_listener(shared.clone());

    let mut writer = match FrameWriter::new(&std::process::id().to_string()) {
        Ok(writer) => writer,
        Err(e) => {
            eprintln!("error: cannot create the shared frame files: {}", e);
            return;
        }
    };

    let mut frame = vec![0u8; frame_size(physical)];
    let mut current_output = output;
    let mut last_frame: Option<Instant> = None;
    let mut frame_count: u64 = 0;
    let static_mode = target_fps == Some(0);
    // In static mode only a new frame is rendered when something changed.
    let mut dirty = true;

    loop {
        if !shared.alive.load(Ordering::Relaxed) {
            log::warn!("the GNOME Shell extension disappeared, stopping");
            break;
        }

        // ── Output changes ───────────────────────────────────────────
        let new_output = *shared.output.lock().unwrap();
        if let Some(new_output) = new_output
            && new_output != current_output
        {
            let physical = new_output.physical();
            log::info!(
                "gnome: output changed to {}x{} (scale {})",
                new_output.logical[0],
                new_output.logical[1],
                new_output.scale
            );
            current_output = new_output;
            app.resize(physical);
            apply_fit(&mut app, fit_mode, physical);
            frame = vec![0u8; frame_size(physical)];
            dirty = true;
        }

        if static_mode && !dirty {
            // Nothing changed: wait for the next event.
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        dirty = false;

        // ── Pointer position (depth parallax) ─────────────────────────
        {
            let cursor = *shared.cursor.lock().unwrap();
            app.user_params.cursor_position = cursor;
        }

        // ── Render ────────────────────────────────────────────────────
        log::trace!("frame {}: rendering...", frame_count);
        if app.render().is_none() {
            log::warn!("frame {}: render returned None", frame_count);
        }
        frame_count = frame_count.wrapping_add(1);

        // ── Read the frame back and hand it to the extension ──────────
        if !app.read_offscreen_frame(&mut frame) {
            log::warn!("frame {}: failed to read the frame back", frame_count);
        }

        let path = match writer.write(&frame) {
            Ok(path) => path.to_string_lossy().to_string(),
            Err(e) => {
                eprintln!("error: cannot write the shared frame: {}", e);
                break;
            }
        };

        let size = current_output.physical();
        if let Err(e) = proxy.call_method("PushFrame", &(path, size[0], size[1])) {
            log::error!("cannot push the frame to the extension: {}", e);
            eprintln!("error: the GNOME Shell extension is not responding: {}", e);
            break;
        }

        if static_mode {
            continue;
        }

        // ── FPS limiting ──────────────────────────────────────────────
        if let Some(target_fps) = target_fps
            && target_fps > 0
            && let Some(last) = last_frame
        {
            let min_delta = Duration::from_secs_f64(1.0 / target_fps as f64);
            if let Some(remaining) = min_delta.checked_sub(last.elapsed()) {
                std::thread::sleep(remaining);
            }
        }
        last_frame = Some(Instant::now());
    }

    // Hide the background again when the app stops.
    let _ = proxy.call_method("SetVisible", &(false,));
}

/// The size (in bytes) of an RGBA frame.
fn frame_size(size: [u32; 2]) -> usize {
    size[0] as usize * size[1] as usize * 4
}

/// Apply the fit mode to the camera projection.
fn apply_fit(app: &mut WgpuApp, fit_mode: FitMode, target: [u32; 2]) {
    let Some(scene) = app.resolution else {
        return;
    };

    let (sx, sy) = fit_scale(fit_mode, scene, target);
    log::info!("fit mode {:?}: ndc scale {}x{}", fit_mode, sx, sy);
    app.set_view_scale(sx, sy);
}

/// Wait until the GNOME Shell extension owns its bus name.
fn wait_for_extension(connection: &Connection, timeout: Duration) -> bool {
    let Ok(proxy) = Proxy::new(
        connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    ) else {
        return false;
    };

    let deadline = Instant::now() + timeout;
    loop {
        let has_owner: bool = proxy
            .call("NameHasOwner", &(EXTENSION_NAME,))
            .unwrap_or(false);
        if has_owner {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Spawn the thread that listens for the extension's signals.
fn spawn_listener(shared: Arc<SharedState>) {
    std::thread::spawn(move || {
        let Ok(connection) = Connection::session() else {
            shared.alive.store(false, Ordering::Relaxed);
            return;
        };
        let Ok(proxy) = Proxy::new(
            &connection,
            EXTENSION_NAME,
            EXTENSION_PATH,
            EXTENSION_INTERFACE,
        ) else {
            shared.alive.store(false, Ordering::Relaxed);
            return;
        };

        let Ok(signals) = proxy.receive_all_signals() else {
            shared.alive.store(false, Ordering::Relaxed);
            return;
        };

        for message in signals {
            let Some(member) = message.header().member().map(|m| m.to_string()) else {
                continue;
            };

            match member.as_str() {
                "OutputChanged" => {
                    let Ok((width, height, scale)) =
                        message.body().deserialize::<(u32, u32, u32)>()
                    else {
                        continue;
                    };
                    let output = OutputInfo {
                        logical: [width.max(1), height.max(1)],
                        scale: scale.max(1),
                    };
                    let mut current = shared.output.lock().unwrap();
                    if current.as_ref() != Some(&output) {
                        log::info!(
                            "gnome: the extension reports a {}x{} output (scale {})",
                            output.logical[0],
                            output.logical[1],
                            output.scale
                        );
                        *current = Some(output);
                    }
                }
                "CursorMoved" => {
                    let Ok((x, y)) = message.body().deserialize::<(f64, f64)>() else {
                        continue;
                    };
                    *shared.cursor.lock().unwrap() = [x as f32, y as f32];
                }
                _ => {}
            }
        }

        shared.alive.store(false, Ordering::Relaxed);
    });
}
