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
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use log;
use pollster::block_on;
use zbus::blocking::{Connection, Proxy};

use crate::engine::runtime::EngineStatus;
use crate::scene::adapters::AdapterConfig;
use crate::scene::error::{EngineError, EngineProblem, RenderError};
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
///
/// Every failure is reported: the adapter hands the error that caused it
/// back to the caller.
pub fn start(config: AdapterConfig) -> Result<(), EngineError> {
    let connection = Connection::session().map_err(EngineError::Bus)?;

    if !wait_for_extension(&connection, EXTENSION_TIMEOUT) {
        return Err(EngineError::Failed(
            "the GNOME Shell extension is not available (install it with: gnome-extension/install.sh, then log out and back in)",
        ));
    }

    let proxy = Proxy::new(
        &connection,
        EXTENSION_NAME,
        EXTENSION_PATH,
        EXTENSION_INTERFACE,
    )
    .map_err(EngineError::Bus)?;

    // Ask the extension for the geometry of the output.
    let (width, height, scale) = proxy
        .call::<_, _, (u32, u32, u32)>("GetOutput", &())
        .map_err(EngineError::Bus)?;
    let output = OutputInfo {
        logical: [width.max(1), height.max(1)],
        scale: scale.max(1),
    };

    log::info!(
        "gnome: output {}x{} (scale {})",
        output.logical[0],
        output.logical[1],
        output.scale
    );

    let physical = output.physical();

    let mut app = block_on(WgpuApp::new(
        config.source.clone(),
        InitAppSurface::Offscreen {
            width: physical[0],
            height: physical[1],
        },
        physical,
        config.no_effects,
        config.no_mdl,
        config.assets_path.clone(),
        config.show_progress,
    ))?;
    // The rate at which the wallpaper is rendered: the videos are played
    // back at that rate.
    app.target_fps = config.target_fps;
    app.load()?;

    // The wallpaper is up: the host is told about it, together with the
    // files that could not be used.
    report(&config.status, &mut app, true);

    // The fit mode is applied to the camera projection.
    apply_fit(&mut app, config.fit_mode, physical);

    let shared = Arc::new(SharedState::new());
    *lock(&shared.output) = Some(output);

    // Listen for the output changes and the pointer position.
    spawn_listener(shared.clone());

    let mut writer = FrameWriter::new(&std::process::id().to_string()).map_err(EngineError::Io)?;

    let mut frame = vec![0u8; frame_size(physical)];
    let mut current_output = output;
    let mut last_frame: Option<Instant> = None;
    let mut frame_count: u64 = 0;
    let static_mode = config.target_fps == Some(0);
    // In static mode only a new frame is rendered when something changed.
    let mut dirty = true;
    // The frames that could not be drawn.
    let mut failures: u64 = 0;
    // The render failure that was reported already.
    let mut reported_failure: Option<RenderError> = None;

    loop {
        if !shared.alive.load(Ordering::Relaxed) {
            log::warn!("the GNOME Shell extension disappeared, stopping");
            break;
        }

        // ── Output changes ───────────────────────────────────────────
        let new_output = *lock(&shared.output);
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
            apply_fit(&mut app, config.fit_mode, physical);
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
            let cursor = *lock(&shared.cursor);
            app.user_params.cursor_position = cursor;
        }

        // ── Render ────────────────────────────────────────────────────
        log::trace!("frame {}: rendering...", frame_count);
        if let Err(error) = app.render() {
            failures = failures.wrapping_add(1);
            log::warn!(
                "frame {}: the frame could not be drawn: {}",
                frame_count,
                error
            );

            // The same failure is reported once: the user does not have
            // to be told about every single frame that was skipped.
            if reported_failure != Some(error) {
                reported_failure = Some(error);
                app.problems.push(EngineProblem::Render(error));
                report(&config.status, &mut app, false);
            }
        }
        frame_count = frame_count.wrapping_add(1);

        // ── Read the frame back and hand it to the extension ──────────
        if !app.read_offscreen_frame(&mut frame) {
            log::warn!("frame {}: failed to read the frame back", frame_count);
        }

        let path = match writer.write(&frame) {
            Ok(path) => path.to_string_lossy().to_string(),
            Err(e) => return Err(EngineError::Io(e)),
        };

        let size = current_output.physical();
        if let Err(e) = proxy.call_method("PushFrame", &(path, size[0], size[1])) {
            log::error!("cannot push the frame to the extension: {}", e);
            return Err(EngineError::Bus(e));
        }

        if static_mode {
            continue;
        }

        // ── FPS limiting ──────────────────────────────────────────────
        if let Some(target_fps) = config.target_fps
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

    Ok(())
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

/// Locks the mutex, even when it is poisoned.
///
/// A poisoned mutex means that a thread died while it held the lock: the
/// data it guards is still there, and unwrapping the poison would take
/// the whole process down (the release profile aborts on a panic).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
                    let mut current = lock(&shared.output);
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
                    *lock(&shared.cursor) = [x as f32, y as f32];
                }
                _ => {}
            }
        }

        shared.alive.store(false, Ordering::Relaxed);
    });
}
