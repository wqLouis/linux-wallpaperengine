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

mod scale;

use std::{ptr::NonNull, time::Duration};

use log;
use pollster::block_on;
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use scale::{FractionalScaleData, ScaleState};
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
    viewporter::client::{wp_viewporter::WpViewporter, wp_viewport::WpViewport},
};

use crate::scene::renderer::app::{InitAppSurface, WgpuApp};

/// Main state for the wlr-layer-shell adapter.
///
/// Owns the Wayland protocol state, the WGPU application, and the
/// fractional-scale + viewporter helpers.
pub struct WlrState {
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub app: WgpuApp,
    pub fit_mode: super::FitMode,
    pub wp_resolution: [u32; 2],

    /// Fractional-scale and viewporter management.
    pub scale: ScaleState,

    // Last configure state, so we can re-apply when scale arrives.
    last_logical: Option<(u32, u32)>,
    last_layer: Option<LayerSurface>,
    /// Track last applied logical size to skip redundant reconfigures.
    last_applied_logical: Option<(u32, u32)>,
}

// ---------------------------------------------------------------------------
// Wayland protocol trait implementations
// ---------------------------------------------------------------------------

impl CompositorHandler for WlrState {
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

impl OutputHandler for WlrState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        if !self.scale.scale_received && self.last_logical.is_some() {
            self.reconfigure();
        }
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        if !self.scale.scale_received {
            self.reconfigure();
        }
    }

    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_output::WlOutput,
    ) {
    }
}

// ---- Fractional-scale dispatch --------------------------------------------------

impl Dispatch<WpFractionalScaleManagerV1, FractionalScaleData, WlrState> for WlrState {
    fn event(
        _: &mut WlrState,
        _: &WpFractionalScaleManagerV1,
        _: <WpFractionalScaleManagerV1 as Proxy>::Event,
        _: &FractionalScaleData,
        _: &Connection,
        _: &QueueHandle<WlrState>,
    ) {
        unreachable!()
    }
}

impl Dispatch<WpFractionalScaleV1, FractionalScaleData, WlrState> for WlrState {
    fn event(
        state: &mut WlrState,
        _: &WpFractionalScaleV1,
        event: <WpFractionalScaleV1 as Proxy>::Event,
        _: &FractionalScaleData,
        _: &Connection,
        _: &QueueHandle<WlrState>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            state.scale.handle_preferred_scale(scale);
            state.reconfigure();
        }
    }
}

impl Dispatch<WpViewporter, FractionalScaleData, WlrState> for WlrState {
    fn event(
        _: &mut WlrState,
        _: &WpViewporter,
        _: <WpViewporter as Proxy>::Event,
        _: &FractionalScaleData,
        _: &Connection,
        _: &QueueHandle<WlrState>,
    ) {
        unreachable!()
    }
}

impl Dispatch<WpViewport, FractionalScaleData, WlrState> for WlrState {
    fn event(
        _: &mut WlrState,
        _: &WpViewport,
        _: <WpViewport as Proxy>::Event,
        _: &FractionalScaleData,
        _: &Connection,
        _: &QueueHandle<WlrState>,
    ) {
        unreachable!()
    }
}

// ---------------------------------------------------------------------------
// Core logic
// ---------------------------------------------------------------------------

impl WlrState {
    /// Recompute the layer-surface size and WGPU swapchain dimensions
    /// based on the current logical size, fit mode, wallpaper resolution,
    /// and fractional scale.
    fn reconfigure(&mut self) {
        let Some((log_w, log_h)) = self.last_logical else {
            return;
        };

        // If the compositor hasn't sent a preferred_scale yet, try to compute
        // one from the output info as a fallback.  This is done before the
        // early-return check so that when output events arrive later
        // (triggering a redundant reconfigure()), the scale can still be
        // picked up even if nothing else changed.
        if !self.scale.scale_received {
            let outputs: Vec<wl_output::WlOutput> = self.output_state.outputs().collect();
            for output in &outputs {
                if self.scale.compute_from_output(&self.output_state, output, self.last_logical) {
                    break;
                }
            }
        }

        // Skip if nothing has changed since the last successful reconfigure.
        if self.scale.scale_num == self.scale.last_applied_scale
            && self.last_applied_logical == self.last_logical
        {
            return;
        }

        let Some(ref layer) = self.last_layer else {
            return;
        };
        let (wp_w, wp_h) = (self.wp_resolution[0] as f32, self.wp_resolution[1] as f32);

        // Compute layer-surface size from fit mode + logical size.
        let (layer_w, layer_h) = match self.fit_mode {
            super::FitMode::Stretch => (log_w, log_h),
            _ => {
                let s = match self.fit_mode {
                    super::FitMode::Cover => {
                        f32::max(log_w as f32 / wp_w, log_h as f32 / wp_h)
                    }
                    super::FitMode::Contain => {
                        f32::min(log_w as f32 / wp_w, log_h as f32 / wp_h)
                    }
                    _ => unreachable!(),
                };
                ((wp_w * s).round() as u32, (wp_h * s).round() as u32)
            }
        };

        // Scale to physical pixels.
        let f = self.scale.scale_num as f64 / 120.0;
        let phys_w = (layer_w as f64 * f).round() as u32;
        let phys_h = (layer_h as f64 * f).round() as u32;

        // Apply viewport destination (sub-surface crop).
        if let Some(ref vp) = self.scale.viewport {
            vp.set_destination(layer_w as i32, layer_h as i32);
        }

        layer.set_size(layer_w, layer_h);
        let _ = layer.set_buffer_scale(1);

        self.app.resize([phys_w, phys_h]);
        if self.app.draw_queue.is_some() {
            self.app.render();
        }

        self.scale.last_applied_scale = self.scale.scale_num;
        self.last_applied_logical = self.last_logical;
    }
}

// ---------------------------------------------------------------------------
// Layer-shell handler
// ---------------------------------------------------------------------------

impl LayerShellHandler for WlrState {
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
        if w == 0 && h == 0 {
            return;
        }

        self.last_logical = Some((w, h));
        self.last_layer = Some(layer.clone());
        self.reconfigure();
    }
}

// ---------------------------------------------------------------------------
// Seat handler (cursor events — unused on wayland for background surfaces)
// ---------------------------------------------------------------------------

impl SeatHandler for WlrState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
    ) {
    }
    fn new_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _: Capability,
    ) {
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _: Capability,
    ) {
    }
    fn remove_seat(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
    ) {
    }
}

// ---------------------------------------------------------------------------
// Delegate macros — generate Dispatch impls for core Wayland objects
// ---------------------------------------------------------------------------

delegate_compositor!(WlrState);
delegate_output!(WlrState);
delegate_seat!(WlrState);
delegate_layer!(WlrState);
delegate_registry!(WlrState);

impl ProvidesRegistryState for WlrState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Start the wallpaper engine using the wlr-layer-shell Wayland protocol.
///
/// Creates a `Layer::Background` surface, binds the required Wayland
/// globals (compositor, layer-shell, fractional-scale, viewporter), and
/// enters the render loop.
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
        log::info!("wp_fractional_scale_manager_v1 bound");
    } else {
        log::info!("wp_fractional_scale_manager_v1 not available");
    }

    let frac_scale: Option<WpFractionalScaleV1> =
        frac_mgr.as_ref().map(|m: &WpFractionalScaleManagerV1| {
            let fs = m.get_fractional_scale(&surface, &qh, FractionalScaleData);
            log::info!("wp_fractional_scale_v1 created");
            fs
        });

    // wp_viewporter
    let viewporter: Option<WpViewporter> =
        globals.bind(&qh, 1..=1, FractionalScaleData).ok();
    let viewport: Option<WpViewport> = viewporter.as_ref().map(|v: &WpViewporter| {
        let vp = v.get_viewport(&surface, &qh, FractionalScaleData);
        log::info!("wp_viewporter bound, viewport created");
        vp
    });

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

    let raw_display_handle = RawDisplayHandle::Wayland(
        WaylandDisplayHandle::new(NonNull::new(conn.backend().display_ptr() as *mut _).unwrap()),
    );
    let raw_window_handle = RawWindowHandle::Wayland(
        WaylandWindowHandle::new(
            NonNull::new(layer.wl_surface().id().as_ptr() as *mut _).unwrap(),
        ),
    );

    let mut app = block_on(WgpuApp::new(
        pkg_path,
        InitAppSurface::Raw((raw_display_handle, raw_window_handle)),
        [256, 256],
        no_effects,
    ));
    app.load();
    let wp_res = app.resolution.expect("Unknown resolution");

    let mut state = WlrState {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        app,
        fit_mode,
        wp_resolution: wp_res,
        scale: ScaleState::new(frac_mgr, frac_scale, viewporter, viewport),
        last_logical: None,
        last_layer: None,
        last_applied_logical: None,
    };

    let frame_duration = Duration::from_millis(16);
    let mut frame_count: u64 = 0;
    loop {
        log::trace!("frame {}: dispatching events...", frame_count);
        event_queue.dispatch_pending(&mut state).unwrap();
        log::trace!("frame {}: calling render...", frame_count);
        let render_result = state.app.render();
        if render_result.is_none() {
            log::warn!("frame {}: render returned None", frame_count);
        }
        std::thread::sleep(frame_duration);
        frame_count = frame_count.wrapping_add(1);
    }
}
