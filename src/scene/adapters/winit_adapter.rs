//! Winit-based adapter: creates an always-on-bottom window.
//!
//! This adapter works on both X11 and Wayland (via XWayland or native
//! wayland winit). Cursor tracking works because winit windows receive
//! pointer events even when stacked behind other windows.

use std::{cell::RefCell, rc::Rc, sync::Arc, time::Instant};

use log;
use pollster::block_on;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::EventLoop,
    window::{CursorIcon, Fullscreen, Window},
};

use crate::engine::runtime::EngineStatus;
use crate::scene::adapters::{AdapterConfig, RenderFailures};
use crate::scene::error::{EngineError, WindowError};
use crate::scene::renderer::app::WgpuApp;

struct WinitApp {
    app: Rc<RefCell<Option<WgpuApp>>>,
    window: Option<Arc<Window>>,

    config: AdapterConfig,
    last_frame: Option<Instant>,
    /// The frames that could not be drawn.
    ///
    /// The failures are counted and reported to the host: a frame that
    /// cannot be drawn is a failure the user must know about.
    failures: RenderFailures,
    /// The failure that happened inside the event loop.
    ///
    /// The event loop has no way to hand an error back, so the failure is
    /// kept here and reported when the loop ends.
    error: Option<EngineError>,
}

impl ApplicationHandler for WinitApp {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.app.borrow().is_some() {
            return;
        }

        let window_attributes = Window::default_attributes()
            .with_decorations(false)
            .with_fullscreen(Some(Fullscreen::Borderless(None)))
            .with_window_level(winit::window::WindowLevel::AlwaysOnBottom)
            .with_transparent(true)
            // Pin the mouse cursor to the standard pointer.
            //
            // `CursorIcon::Default` is the "normal pointer" variant: the
            // regular arrow the desktop uses for everything else.  It is
            // deliberately *not* one of the special shapes (`Text` = I-beam,
            // `Pointer` = hand, `Crosshair`, ...).  Without this the shape
            // of a fullscreen, transparent, always-on-bottom window is
            // whatever the compositor/window manager decides to use for it,
            // which on some setups is a text cursor or another odd shape.
            // The cursor stays visible - only its shape is pinned.
            .with_cursor(CursorIcon::Default)
            .with_title("Linux wallpaper engine");

        let window = match event_loop.create_window(window_attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.fail(EngineError::Window(WindowError::Create(error)));
                event_loop.exit();
                return;
            }
        };

        // Re-assert the standard pointer on the created window: the
        // attribute above only describes the *initial* cursor, and some
        // compositors apply it lazily (on the first pointer enter).
        window.set_cursor(CursorIcon::Default);

        let size = window.inner_size();

        let mut wgpu_app = match block_on(WgpuApp::new(
            self.config.source.clone(),
            crate::scene::renderer::app::InitAppSurface::Winit(Arc::clone(&window)),
            [size.width, size.height],
            self.config.no_effects,
            self.config.no_mdl,
            self.config.assets_path.clone(),
            self.config.show_progress,
        )) {
            Ok(app) => app,
            Err(error) => {
                self.fail(error);
                event_loop.exit();
                return;
            }
        };

        // The rate at which the wallpaper is rendered: the videos are
        // played back at that rate.
        wgpu_app.target_fps = self.config.target_fps;

        if let Err(error) = wgpu_app.load() {
            self.fail(error);
            event_loop.exit();
            return;
        }

        // The wallpaper is up: the host is told about it, together with
        // the files that could not be used.
        report(&self.config.status, &mut wgpu_app, true);

        self.app.borrow_mut().replace(wgpu_app);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let mut app = self.app.borrow_mut();

        match event {
            WindowEvent::RedrawRequested => {
                // Throttle to target_fps if set.
                if let Some(target_fps) = self.config.target_fps
                    && target_fps > 0
                    && let Some(last) = self.last_frame
                {
                    let min_delta = std::time::Duration::from_secs_f64(1.0 / target_fps as f64);
                    if let Some(remaining) = min_delta.checked_sub(last.elapsed()) {
                        std::thread::sleep(remaining);
                    }
                }

                self.last_frame = Some(Instant::now());

                // The window may be gone already: there is nothing to
                // draw into then.
                let Some(window) = self.window.as_ref() else {
                    return;
                };

                let Some(app) = app.as_mut() else {
                    return;
                };

                window.pre_present_notify();

                log::trace!("RedrawRequested: calling render...");
                if let Err(error) = app.render() {
                    // The frame that could not be drawn is counted and
                    // reported: the same failure is reported once, the
                    // user does not have to be told about every single
                    // frame that was skipped.
                    self.failures.record(error, app.title.as_deref());
                }

                // The problems of the wallpaper — the files that could
                // not be used, the effects that could not be drawn — are
                // reported to the host.  They are taken out of the list,
                // so a problem is reported once.
                report(&self.config.status, app, false);

                // Don't request the next redraw when target_fps is 0
                // (static image mode).
                if self.config.target_fps != Some(0) {
                    window.request_redraw();
                    log::trace!("requested next redraw");
                }
            }
            WindowEvent::Resized(physical_size) => {
                let Some(app) = app.as_mut() else {
                    return;
                };

                app.resize([physical_size.width, physical_size.height]);
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved {
                device_id: _,
                position,
                ..
            } => {
                if let Some(app) = app.as_mut() {
                    let size = self.window.as_ref().map(|w| w.inner_size());
                    if let Some(size) = size {
                        let nx = position.x as f32 / size.width as f32;
                        let ny = position.y as f32 / size.height as f32;
                        app.user_params = crate::scene::renderer::app::UserParams {
                            cursor_position: [nx, ny],
                        };
                    }
                }
            }
            _ => {}
        }
    }
}

impl WinitApp {
    /// Remembers the failure that happened inside the event loop.
    fn fail(&mut self, error: EngineError) {
        log::error!("{}", error.trace());
        if self.error.is_none() {
            self.error = Some(error);
        }
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

/// Start the wallpaper engine with the winit backend.
///
/// The window is created on the main thread (the windowing system wants
/// it there) and the wallpaper is rendered until the window is closed.
pub fn start(config: AdapterConfig) -> Result<(), EngineError> {
    let event_loop =
        EventLoop::new().map_err(|error| EngineError::Window(WindowError::EventLoop(error)))?;
    // The failures of the frames are reported to the host.
    let failures = RenderFailures::new(config.status.clone());
    let mut app = WinitApp {
        config,
        last_frame: None,
        failures,
        error: None,
        app: Rc::new(RefCell::new(None)),
        window: None,
    };

    let result = event_loop
        .run_app(&mut app)
        .map_err(|error| EngineError::Window(WindowError::EventLoop(error)));

    // The failure that happened inside the event loop (the window could
    // not be created, the wallpaper could not be loaded) is reported: the
    // event loop itself has no way to hand an error back.
    match app.error {
        Some(error) => Err(error),
        None => result,
    }
}
