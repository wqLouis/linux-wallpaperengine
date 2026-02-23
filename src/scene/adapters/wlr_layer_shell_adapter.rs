use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_seat,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{Capability, SeatHandler, SeatState},
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, Layer, LayerShell, LayerShellHandler, LayerSurfaceConfigure},
    },
};
use std::ptr::NonNull;
use wayland_client::{
    Connection, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_seat, wl_surface},
};

// 引入您的 WgpuApp
use crate::scene::renderer::render::WgpuApp;

// ============================================================================
// 1. Wayland Surface Handle Wrapper
// ============================================================================

#[derive(Clone)]
struct WaylandSurfaceHandle {
    surface: NonNull<std::ffi::c_void>,
    display: NonNull<std::ffi::c_void>,
}

impl HasWindowHandle for WaylandSurfaceHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = RawWindowHandle::Wayland(WaylandWindowHandle::new(self.surface));
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for WaylandSurfaceHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let raw = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(self.display));
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}

unsafe impl Send for WaylandSurfaceHandle {}
unsafe impl Sync for WaylandSurfaceHandle {}

// ============================================================================
// 2. Main Entry Point
// ============================================================================

pub fn start(pkg_path: String) {
    let conn = Connection::connect_to_env().unwrap();
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let compositor_state = CompositorState::bind(&globals, &qh).unwrap();
    let layer_shell = LayerShell::bind(&globals, &qh).unwrap();

    let surface = compositor_state.create_surface(&qh);

    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Background, Some(""), None);

    layer.set_anchor(Anchor::all());
    layer.set_size(0, 0);

    let mut app = WlrLayerShellApp {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        wgpu_app: None,
        layer_surface: Some(layer),
        exit: false,
        pkg_path,
    };

    app.layer_surface.as_ref().unwrap().wl_surface().commit();

    println!("Layer shell started. Waiting for configure...");

    loop {
        event_queue.blocking_dispatch(&mut app).unwrap();

        if app.exit {
            println!("exiting example");
            break;
        }
    }
}

// ============================================================================
// 3. Application State
// ============================================================================

struct WlrLayerShellApp {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,

    wgpu_app: Option<WgpuApp>,
    layer_surface: Option<smithay_client_toolkit::shell::wlr_layer::LayerSurface>,

    exit: bool,

    pkg_path: String,
}

// ============================================================================
// 4. Layer Shell Handler
// ============================================================================

impl LayerShellHandler for WlrLayerShellApp {
    fn closed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &smithay_client_toolkit::shell::wlr_layer::LayerSurface,
    ) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        _layer: &smithay_client_toolkit::shell::wlr_layer::LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (width, height) = configure.new_size;

        if self.wgpu_app.is_none() && width > 0 && height > 0 {
            println!("Configured with size: {}x{}", width, height);

            let surface = self.layer_surface.as_ref().unwrap().wl_surface();

            let handle = WaylandSurfaceHandle {
                surface: NonNull::new(surface.id().as_ptr() as *mut _).unwrap(),
                display: NonNull::new(conn.backend().display_ptr() as *mut _).unwrap(),
            };

            let mut wgpu_app = pollster::block_on(WgpuApp::new(
                self.pkg_path.clone(),
                handle.clone(),
                [width, height],
            ));

            wgpu_app.load();

            self.wgpu_app = Some(wgpu_app);

            surface.frame(qh, surface.clone());
            surface.commit();
        } else if let Some(app) = &mut self.wgpu_app {
            app.resize([width, height]);
            let surface = self.layer_surface.as_ref().unwrap().wl_surface();
            surface.frame(qh, surface.clone());
            surface.commit();
        }
    }
}

// ============================================================================
// 5. Compositor Handler
// ============================================================================

impl CompositorHandler for WlrLayerShellApp {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        if let Some(app) = &mut self.wgpu_app {
            match app.render() {
                Ok(_) => {
                    surface.frame(qh, surface.clone());
                    surface.commit();
                }
                Err(e) => eprintln!("Render error: {:?}", e),
            }
        }
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

// ============================================================================
// 6. Other Handlers & Delegates
// ============================================================================

impl OutputHandler for WlrLayerShellApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl SeatHandler for WlrLayerShellApp {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl ProvidesRegistryState for WlrLayerShellApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

// Delegates
delegate_compositor!(WlrLayerShellApp);
delegate_output!(WlrLayerShellApp);
delegate_seat!(WlrLayerShellApp);
delegate_layer!(WlrLayerShellApp);
delegate_registry!(WlrLayerShellApp);
