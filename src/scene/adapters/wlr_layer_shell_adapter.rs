use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use smithay_client_toolkit::{
    compositor::CompositorState,
    output::OutputState,
    registry::RegistryState,
    seat::SeatState,
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell},
    },
};
use std::{ptr::NonNull, time::Duration};
use wayland_client::{Connection, Proxy, globals::registry_queue_init};

use super::wlr_app::{FitMode, Wgpu};
use crate::scene::renderer::app::{InitAppSurface, WgpuApp};

pub fn start(pkg_path: String, resolution: Option<[u32; 2]>, fit_mode: FitMode) {
    let conn = Connection::connect_to_env().unwrap();
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let compositor_state = CompositorState::bind(&globals, &qh).unwrap();
    let layer_shell = LayerShell::bind(&globals, &qh).unwrap();
    let surface = compositor_state.create_surface(&qh);

    let layer = layer_shell.create_layer_surface(
        &qh,
        surface,
        Layer::Background,
        Some("linux wallpaper engine"),
        None,
    );

    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_exclusive_zone(-1);
    layer.set_anchor(Anchor::all());
    layer.set_size(0, 0);
    layer.commit();

    let raw_display_handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
        NonNull::new(conn.backend().display_ptr() as *mut _).unwrap(),
    ));
    let raw_window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
        NonNull::new(layer.wl_surface().id().as_ptr() as *mut _).unwrap(),
    ));

    let mut app = pollster::block_on(WgpuApp::new(
        pkg_path,
        InitAppSurface::Raw((raw_display_handle, raw_window_handle)),
        [256, 256],
    ));

    app.load();

    let wp_res = resolution.unwrap_or(app.resolution.expect("Unknown resolution"));

    let mut wgpu = Wgpu {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        app,
        fit_mode,
        wp_resolution: wp_res,
    };

    let frame_duration = Duration::from_millis(16);
    loop {
        event_queue.dispatch_pending(&mut wgpu).unwrap();
        wgpu.app.render();
        std::thread::sleep(frame_duration);
    }
}
