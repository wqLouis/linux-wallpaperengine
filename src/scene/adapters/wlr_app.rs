//! Wayland (wlr-layer-shell) adapter.
//!
//! Renders the wallpaper on a `Layer::Background` surface behind all windows.
//! The WGPU swapchain uses the wallpaper's native resolution; the compositor
//! handles scaling the layer surface to fill the output.
//!
//! ## Depth parallax
//!
//! Wayland's security model does not allow background (or any non-focused)
//! surfaces to receive pointer events.  Depth-parallax effects that rely on
//! cursor position are therefore unavailable in the wlr adapter.  The winit
//! adapter should be used when cursor-parallax is desired.

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
    Connection, Dispatch, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_seat, wl_surface},
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{self, WpFractionalScaleV1},
    },
    viewporter::client::{
        wp_viewporter::WpViewporter,
        wp_viewport::WpViewport,
    },
};

use crate::scene::renderer::app::{InitAppSurface, WgpuApp};

#[derive(Debug)]
struct FractionalScaleData;

pub struct Wgpu {
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub app: WgpuApp,
    pub fit_mode: super::FitMode,
    pub wp_resolution: [u32; 2],

    /// Fractional scale, encoded as numerator/120 (default 120 = 1.0x).
    /// Updated by wp_fractional_scale_v1::preferred_scale, or computed
    /// from output info as fallback.
    scale_num: u32,
    /// True once preferred_scale event was received (locks out fallback).
    scale_received: bool,

    // Last configure state, so we can re-apply when scale arrives.
    last_logical: Option<(u32, u32)>,
    last_layer: Option<LayerSurface>,
    // Track last applied to skip redundant reconfigures.
    last_applied_scale: u32,
    last_applied_logical: Option<(u32, u32)>,

    // Protocol objects that must be kept alive.
    _fractional_scale_mgr: Option<WpFractionalScaleManagerV1>,
    _fractional_scale: Option<WpFractionalScaleV1>,
    _viewporter: Option<WpViewporter>,
    viewport: Option<WpViewport>,
}

impl CompositorHandler for Wgpu {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Wgpu {
    fn output_state(&mut self) -> &mut OutputState { &mut self.output_state }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        if !self.scale_received && self.last_logical.is_some() {
            self.compute_scale_from_output(&output);
        }
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        if !self.scale_received {
            self.compute_scale_from_output(&output);
        }
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

// ---- Protocol dispatch implementations ----

impl Dispatch<WpFractionalScaleManagerV1, FractionalScaleData, Wgpu> for Wgpu {
    fn event(
        _: &mut Wgpu,
        _: &WpFractionalScaleManagerV1,
        _: <WpFractionalScaleManagerV1 as Proxy>::Event,
        _: &FractionalScaleData,
        _: &Connection,
        _: &QueueHandle<Wgpu>,
    ) { unreachable!() }
}

impl Dispatch<WpFractionalScaleV1, FractionalScaleData, Wgpu> for Wgpu {
    fn event(
        state: &mut Wgpu,
        _: &WpFractionalScaleV1,
        event: <WpFractionalScaleV1 as Proxy>::Event,
        _: &FractionalScaleData,
        _: &Connection,
        _: &QueueHandle<Wgpu>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            eprintln!("[wlr] preferred_scale: {} (×{:.2})", scale, scale as f64 / 120.0);
            state.scale_num = scale;
            state.scale_received = true;
            state.reconfigure();
        }
    }
}

impl Dispatch<WpViewporter, FractionalScaleData, Wgpu> for Wgpu {
    fn event(
        _: &mut Wgpu,
        _: &WpViewporter,
        _: <WpViewporter as Proxy>::Event,
        _: &FractionalScaleData,
        _: &Connection,
        _: &QueueHandle<Wgpu>,
    ) { unreachable!() }
}

impl Dispatch<WpViewport, FractionalScaleData, Wgpu> for Wgpu {
    fn event(
        _: &mut Wgpu,
        _: &WpViewport,
        _: <WpViewport as Proxy>::Event,
        _: &FractionalScaleData,
        _: &Connection,
        _: &QueueHandle<Wgpu>,
    ) { unreachable!() }
}

// ---- Core logic ----

impl Wgpu {
    fn compute_scale_from_output(&mut self, output: &wl_output::WlOutput) {
        let Some(info) = self.output_state.info(output) else { return };
        let Some(mode) = info.modes.iter().find(|m| m.current) else { return };

        let (log_w, log_h) = match info.logical_size {
            Some((w, h)) if w > 0 && h > 0 => (w, h),
            _ => match self.last_logical {
                Some((w, h)) => (w as i32, h as i32),
                _ => return,
            },
        };
        if log_w <= 0 || log_h <= 0 { return; }

        let w_scale = mode.dimensions.0 as f64 / log_w as f64;
        let h_scale = mode.dimensions.1 as f64 / log_h as f64;
        let computed = ((w_scale + h_scale) / 2.0 * 120.0).round() as u32;

        if computed > self.scale_num {
            self.scale_num = computed;
            self.scale_received = true;
            self.reconfigure();
        }
    }

    fn reconfigure(&mut self) {
        let Some((log_w, log_h)) = self.last_logical else { return };

        if self.scale_num == self.last_applied_scale
            && self.last_applied_logical == self.last_logical
        {
            return;
        }

        if !self.scale_received {
            let outputs: Vec<wl_output::WlOutput> = self.output_state.outputs().collect();
            for output in &outputs {
                self.compute_scale_from_output(output);
                if self.scale_received { break; }
            }
        }

        let Some(ref layer) = self.last_layer else { return };

        let (wp_w, wp_h) = (self.wp_resolution[0] as f32, self.wp_resolution[1] as f32);

        let (layer_w, layer_h) = match self.fit_mode {
            super::FitMode::Stretch => (log_w, log_h),
            _ => {
                let s = match self.fit_mode {
                    super::FitMode::Cover =>
                        f32::max(log_w as f32 / wp_w, log_h as f32 / wp_h),
                    super::FitMode::Contain =>
                        f32::min(log_w as f32 / wp_w, log_h as f32 / wp_h),
                    _ => unreachable!(),
                };
                ((wp_w * s).round() as u32, (wp_h * s).round() as u32)
            }
        };

        let f = self.scale_num as f64 / 120.0;
        let phys_w = (layer_w as f64 * f).round() as u32;
        let phys_h = (layer_h as f64 * f).round() as u32;

        if let Some(ref vp) = self.viewport {
            vp.set_destination(layer_w as i32, layer_h as i32);
        }

        layer.set_size(layer_w, layer_h);
        let _ = layer.set_buffer_scale(1);

        self.app.resize([phys_w, phys_h]);
        if self.app.draw_queue.is_some() { self.app.render(); }

        self.last_applied_scale = self.scale_num;
        self.last_applied_logical = self.last_logical;
    }
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
        let (w, h) = configure.new_size;
        if w == 0 && h == 0 { return; }

        self.last_logical = Some((w, h));
        self.last_layer = Some(layer.clone());
        self.reconfigure();
    }
}

impl SeatHandler for Wgpu {
    fn seat_state(&mut self) -> &mut SeatState { &mut self.seat_state }
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
    fn registry(&mut self) -> &mut RegistryState { &mut self.registry_state }
    registry_handlers![OutputState, SeatState];
}

pub fn start(pkg_path: String, fit_mode: super::FitMode, no_effects: bool) {
    let conn = Connection::connect_to_env().unwrap();
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    // --- Bind protocols ---

    let compositor_state = CompositorState::bind(&globals, &qh).unwrap();
    let layer_shell = LayerShell::bind(&globals, &qh).unwrap();
    let surface = compositor_state.create_surface(&qh);

    // wp_fractional_scale_manager_v1
    let frac_mgr: Option<WpFractionalScaleManagerV1> =
        globals.bind(&qh, 1..=1, FractionalScaleData).ok();
    if frac_mgr.is_some() {
        eprintln!("[wlr] wp_fractional_scale_manager_v1 bound");
    } else {
        eprintln!("[wlr] wp_fractional_scale_manager_v1 not available");
    }

    let frac_scale: Option<WpFractionalScaleV1> = frac_mgr.as_ref().map(|m: &WpFractionalScaleManagerV1| {
        let fs = m.get_fractional_scale(&surface, &qh, FractionalScaleData);
        eprintln!("[wlr] wp_fractional_scale_v1 created");
        fs
    });

    // wp_viewporter
    let viewporter: Option<WpViewporter> =
        globals.bind(&qh, 1..=1, FractionalScaleData).ok();
    let viewport: Option<WpViewport> = viewporter.as_ref().map(|v: &WpViewporter| {
        let vp = v.get_viewport(&surface, &qh, FractionalScaleData);
        eprintln!("[wlr] wp_viewporter bound, viewport created");
        vp
    });

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
        scale_num: 120,
        scale_received: false,
        last_logical: None,
        last_layer: None,
        _fractional_scale_mgr: frac_mgr,
        _fractional_scale: frac_scale,
        _viewporter: viewporter,
        viewport,
        last_applied_scale: 120,
        last_applied_logical: None,
    };

    let frame_duration = Duration::from_millis(16);
    loop {
        if let Err(e) = event_queue.dispatch_pending(&mut wgpu) {
            eprintln!("[wlr] Wayland dispatch error: {:?}", e);
            break;
        }
        if wgpu.app.render().is_none() {
            std::thread::sleep(Duration::from_millis(100));
        } else {
            std::thread::sleep(frame_duration);
        }
    }
    eprintln!("[wlr] render loop exited, will restart");
}
