use std::sync::Arc;

use wgpu::*;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::EventLoop,
    window::{Window, WindowAttributes},
};

pub struct State {
    surface: Surface<'static>,
    device: Device,
    queue: Queue,
    config: SurfaceConfiguration,
    is_surface_configured: bool,
    window: Arc<Window>,
}

impl State {
    async fn new(window: Arc<Window>) -> Option<State> {
        let size = window.inner_size();

        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::PRIMARY,
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok()?;

        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: None,
                required_features: Features::empty(),
                experimental_features: ExperimentalFeatures::disabled(),
                required_limits: Limits::defaults(),
                memory_hints: Default::default(),
                trace: Trace::Off,
            })
            .await
            .ok()?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: PresentMode::Immediate,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        Some(State {
            surface: surface,
            device: device,
            queue: queue,
            config: config,
            window: window,
            is_surface_configured: false,
        })
    }
}

#[derive(Default)]
struct App {
    window: Option<Window>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window_attr = WindowAttributes::default().with_inner_size(PhysicalSize::new(800, 800));

        self.window = Some(event_loop.create_window(window_attr).unwrap());
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                println!("Window closed");
                event_loop.exit();
            }
            _ => {}
        }
    }
}

pub fn create_window() {
    let eventloop = EventLoop::new().unwrap();

    eventloop.set_control_flow(winit::event_loop::ControlFlow::Wait);

    let mut app = App::default();
    eventloop.run_app(&mut app).unwrap();
}
