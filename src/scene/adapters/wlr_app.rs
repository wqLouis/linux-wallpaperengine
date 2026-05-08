//! Wayland (wlr-layer-shell) adapter.
//!
//! Renders the wallpaper on a `Layer::Background` surface behind all windows.
//! The WGPU swapchain uses the wallpaper's native resolution; the compositor
//! handles scaling the layer surface to fill the output. Cursor-based parallax
//! is unavailable on wayland (background surfaces never receive pointer focus),
//! so an animated drift from the scene's `cameraparallax*` settings is used.

use std::{ptr::NonNull, time::Duration};

use pollster::block_on;
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
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
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
};
use wayland_client::{
    Connection, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_seat, wl_surface},
};

use crate::scene::renderer::app::{InitAppSurface, WgpuApp};

pub struct Wgpu {
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub app: WgpuApp,
    pub fit_mode: super::FitMode,
    pub wp_resolution: [u32; 2],
}

impl CompositorHandler for Wgpu {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Wgpu {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Wgpu {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {}
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let (new_width, new_height) = configure.new_size;
        if new_width == 0 && new_height == 0 {
            return;
        }

        // Scale wallpaper to fill the output (fit mode), keeping both
        // the layer surface and WGPU surface at the same size so the
        // compositor doesn't get a buffer larger than the surface.
        // The WGPU rendering happens in wallpaper coordinates via the
        // projection matrix, so the swapchain size just needs to match
        // the layer surface size.
        let (use_w, use_h) = match self.fit_mode {
            super::FitMode::Stretch => (new_width, new_height),
            _ => {
                let (wp_w, wp_h) = (self.wp_resolution[0] as f32, self.wp_resolution[1] as f32);
                let scale = match self.fit_mode {
                    super::FitMode::Cover => f32::max(new_width as f32 / wp_w, new_height as f32 / wp_h),
                    super::FitMode::Contain => f32::min(new_width as f32 / wp_w, new_height as f32 / wp_h),
                    _ => unreachable!(),
                };
                ((wp_w * scale).round() as u32, (wp_h * scale).round() as u32)
            }
        };
        layer.set_size(use_w, use_h);
        self.app.resize([use_w, use_h]);
        self.app.render().unwrap();
    }
}

impl SeatHandler for Wgpu {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, _: Capability) {}
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, _: Capability) {}
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

delegate_compositor!(Wgpu);
delegate_output!(Wgpu);
delegate_seat!(Wgpu);
delegate_layer!(Wgpu);
delegate_registry!(Wgpu);

impl ProvidesRegistryState for Wgpu {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

pub fn start(pkg_path: String, fit_mode: super::FitMode, no_effects: bool) {
    let conn = Connection::connect_to_env().unwrap();
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let compositor_state = CompositorState::bind(&globals, &qh).unwrap();
    let layer_shell = LayerShell::bind(&globals, &qh).unwrap();
    let surface = compositor_state.create_surface(&qh);

    let layer = layer_shell.create_layer_surface(
        &qh, surface, Layer::Background, Some("linux wallpaper engine"), None,
    );
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_exclusive_zone(-1);
    layer.set_anchor(Anchor::all());
    layer.set_size(0, 0);
    layer.commit();

    let raw_display_handle = RawDisplayHandle::Wayland(
        WaylandDisplayHandle::new(NonNull::new(conn.backend().display_ptr() as *mut _).unwrap()),
    );
    let raw_window_handle = RawWindowHandle::Wayland(
        WaylandWindowHandle::new(NonNull::new(layer.wl_surface().id().as_ptr() as *mut _).unwrap()),
    );

    let mut app = block_on(WgpuApp::new(
        pkg_path,
        InitAppSurface::Raw((raw_display_handle, raw_window_handle)),
        [256, 256],
        no_effects,
    ));
    app.load();
    let wp_res = app.resolution.expect("Unknown resolution");

    let mut wgpu = Wgpu {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        app,
        fit_mode,
        wp_resolution: wp_res,
    };

    // Event-driven render loop: dispatch wayland events, render each frame
    let frame_duration = Duration::from_millis(16);
    loop {
        event_queue.dispatch_pending(&mut wgpu).unwrap();
        wgpu.app.render();
        std::thread::sleep(frame_duration);
    }
}
