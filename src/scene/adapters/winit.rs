use std::sync::{Arc, Mutex};

use pollster::block_on;
use winit::{application::ApplicationHandler, event::WindowEvent, window::Window};

use crate::scene::renderer::render::WgpuApp;

struct WinitApp {
    app: Arc<Mutex<Option<WgpuApp>>>,
    window: Arc<Window>,

    pkg_path: String,
}

impl ApplicationHandler for WinitApp {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.app.as_ref().lock().unwrap().is_some() {
            return;
        }

        let window_attributes = Window::default_attributes().with_title("Linux wallpaper engine");
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        let window_pointer = Arc::clone(&window);
        let size = window.inner_size();

        let wgpu_app = block_on(WgpuApp::new(
            self.pkg_path.clone(),
            window,
            [size.width, size.height],
        ));

        self.app.lock().unwrap().replace(wgpu_app);
        self.window = window_pointer;
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
                self.window.pre_present_notify();

                match app.render() {
                    Ok(_) => {}
                    Err(e) => eprintln!("{:?}", e),
                }
            }
            WindowEvent::Resized(physical_size) => {
                let app = app.as_mut().unwrap();
            }
            _ => {}
        }
    }
}
