//! Winit-based adapter: creates an always-on-bottom window.
//!
//! This adapter works on both X11 and Wayland (via XWayland or native
//! wayland winit). Cursor tracking works because winit windows receive
//! pointer events even when stacked behind other windows.

use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use log;
use pollster::block_on;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::EventLoop,
    window::{Fullscreen, Window},
};

use crate::scene::renderer::app::WgpuApp;

struct WinitApp {
    app: Arc<Mutex<Option<WgpuApp>>>,
    window: Option<Arc<Window>>,

    pkg_path: String,
    no_effects: bool,
    no_mdl: bool,
    assets_path: Option<String>,
    target_fps: Option<u32>,
    last_frame: Option<Instant>,
}

impl ApplicationHandler for WinitApp {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.app.as_ref().lock().unwrap().is_some() {
            return;
        }

        let window_attributes = Window::default_attributes()
            .with_decorations(false)
            .with_fullscreen(Some(Fullscreen::Borderless(None)))
            .with_window_level(winit::window::WindowLevel::AlwaysOnBottom)
            .with_transparent(true)
            .with_title("Linux wallpaper engine");

        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());

        let size = window.inner_size();

        let mut wgpu_app = block_on(WgpuApp::new(
            self.pkg_path.clone(),
            crate::scene::renderer::app::InitAppSurface::Winit(Arc::clone(&window)),
            [size.width, size.height],
            self.no_effects,
            self.no_mdl,
            self.assets_path.clone(),
        ));

        wgpu_app.load();

        self.app.lock().unwrap().replace(wgpu_app);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let mut app = self.app.lock().unwrap();

        match event {
            WindowEvent::RedrawRequested => {
                // Throttle to target_fps if set.
                if let Some(target_fps) = self.target_fps {
                    if target_fps > 0 {
                        if let Some(last) = self.last_frame {
                            let min_delta =
                                std::time::Duration::from_secs_f64(1.0 / target_fps as f64);
                            if let Some(remaining) = min_delta.checked_sub(last.elapsed()) {
                                std::thread::sleep(remaining);
                            }
                        }
                    }
                }

                self.last_frame = Some(Instant::now());

                let app = app.as_mut().unwrap();
                self.window.as_ref().unwrap().pre_present_notify();

                log::trace!("RedrawRequested: calling render...");
                let render_result = app.render();
                if render_result.is_none() {
                    log::warn!("render returned None");
                }
                // Don't request the next redraw when target_fps is 0
                // (static image mode).
                if self.target_fps != Some(0) {
                    self.window.as_ref().unwrap().request_redraw();
                    log::trace!("requested next redraw");
                }
            }
            WindowEvent::Resized(physical_size) => {
                let app = app.as_mut().unwrap();
                app.resize([physical_size.width, physical_size.height]);
                self.window.as_ref().unwrap().request_redraw();
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
                        app.user_params =
                            crate::scene::renderer::app::UserParams {
                                cursor_position: [nx, ny],
                            };
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn start(
    pkg_path: String,
    no_effects: bool,
    no_mdl: bool,
    assets_path: Option<String>,
    target_fps: Option<u32>,
) {
    let event_loop = EventLoop::new().unwrap();
    let mut app = WinitApp {
        pkg_path,
        no_effects,
        no_mdl,
        assets_path,
        target_fps,
        last_frame: None,
        app: Arc::new(Mutex::new(None)),
        window: None,
    };

    event_loop.run_app(&mut app).unwrap();
}
