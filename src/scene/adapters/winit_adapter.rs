use std::sync::{Arc, Mutex};

use pollster::block_on;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{self, EventLoop},
    window::{Fullscreen, Window},
};

use crate::scene::renderer::render::WgpuApp;

#[derive(Default)]
struct WinitApp {
    app: Arc<Mutex<Option<WgpuApp>>>,
    window: Option<Arc<Window>>,

    pkg_path: String,
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
            Arc::clone(&window),
            [size.width, size.height],
        ));

        wgpu_app.load();

        self.app.lock().unwrap().replace(wgpu_app);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let mut app = self.app.lock().unwrap();

        match event {
            WindowEvent::RedrawRequested => {
                let app = app.as_mut().unwrap();
                self.window.as_ref().unwrap().pre_present_notify();

                match app.render() {
                    Ok(_) => {}
                    Err(e) => eprintln!("{:?}", e),
                }
            }
            WindowEvent::Resized(physical_size) => {
                let app = app.as_mut().unwrap();
                app.resize([physical_size.width, physical_size.height]);
                self.window.as_ref().unwrap().request_redraw();
            }
            _ => {}
        }
    }
}

pub fn start(pkg_path: String) {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(event_loop::ControlFlow::Wait);
    let mut app = WinitApp {
        pkg_path,
        ..Default::default()
    };

    event_loop.run_app(&mut app).unwrap();
}
