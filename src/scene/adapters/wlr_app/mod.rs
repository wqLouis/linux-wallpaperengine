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

use std::ptr::NonNull;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

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
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_seat, wl_surface},
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{self, WpFractionalScaleV1},
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};

use crate::engine::runtime::{EngineCommand, EngineStatus};
use crate::engine::source::WallpaperSource;
use crate::scene::adapters::{AdapterConfig, RenderFailures};
use crate::scene::error::{EngineError, EngineProblem, WindowError};
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

    pub target_fps: Option<u32>,
    /// Track last frame time for FPS limiting.
    last_frame: Option<Instant>,
    /// Set to true when reconfigure changes the swapchain (so we re-render once
    /// in static-image mode) and initially true for the first frame.
    needs_render: bool,
    /// The failures of the adapter that were reported already.
    ///
    /// The same failure is reported once: the display server can send the
    /// same event on every frame, and the channel of the host must not be
    /// flooded with it.
    reported_failures: Vec<&'static str>,
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

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

// ---- Fractional-scale dispatch --------------------------------------------------

impl Dispatch<WpFractionalScaleManagerV1, FractionalScaleData, WlrState> for WlrState {
    fn event(
        state: &mut WlrState,
        _: &WpFractionalScaleManagerV1,
        _: <WpFractionalScaleManagerV1 as Proxy>::Event,
        _: &FractionalScaleData,
        _: &Connection,
        _: &QueueHandle<WlrState>,
    ) {
        // The manager has no event: the display server does not send one.
        // A protocol error must not take the process down, so it is
        // reported and the wallpaper carries on.
        state.report_failure("the display server sent the fractional scale manager an event");
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
        state: &mut WlrState,
        _: &WpViewporter,
        _: <WpViewporter as Proxy>::Event,
        _: &FractionalScaleData,
        _: &Connection,
        _: &QueueHandle<WlrState>,
    ) {
        // The viewporter has no event: the display server does not send
        // one.  A protocol error must not take the process down, so it is
        // reported and the wallpaper carries on.
        state.report_failure("the display server sent the viewporter an event");
    }
}

impl Dispatch<WpViewport, FractionalScaleData, WlrState> for WlrState {
    fn event(
        state: &mut WlrState,
        _: &WpViewport,
        _: <WpViewport as Proxy>::Event,
        _: &FractionalScaleData,
        _: &Connection,
        _: &QueueHandle<WlrState>,
    ) {
        // The viewport has no event: the display server does not send
        // one.  A protocol error must not take the process down, so it is
        // reported and the wallpaper carries on.
        state.report_failure("the display server sent the viewport an event");
    }
}

// ---------------------------------------------------------------------------
// Core logic
// ---------------------------------------------------------------------------

impl WlrState {
    /// Reports a failure of the adapter.
    ///
    /// The failure is logged and told to the host, once: the display
    /// server can send the same event on every frame, and the channel of
    /// the host must not be flooded with it.  The wallpaper carries on:
    /// a failure of the protocol is not a fatal error.
    fn report_failure(&mut self, what: &'static str) {
        if self.reported_failures.contains(&what) {
            return;
        }
        self.reported_failures.push(what);

        log::error!("the adapter could not carry out what was asked of it: {what}");
        self.app.problems.push(EngineProblem::Adapter(what));
    }

    /// Recompute the layer-surface size and WGPU swapchain dimensions
    /// based on the current logical size, fit mode, wallpaper resolution,
    /// and fractional scale.
    fn reconfigure(&mut self) {
        let Some((log_w, log_h)) = self.last_logical else {
            return;
        };

        // Snapshot scale state before any fallback computation so we can
        // detect whether it changed and always apply when it does.
        let old_scale_received = self.scale.scale_received;
        let old_scale_num = self.scale.scale_num;

        // If the compositor hasn't sent a preferred_scale yet, try to compute
        // one from the output info as a fallback.
        if !self.scale.scale_received {
            let outputs: Vec<wl_output::WlOutput> = self.output_state.outputs().collect();
            for output in &outputs {
                if self
                    .scale
                    .compute_from_output(&self.output_state, output, self.last_logical)
                {
                    break;
                }
            }
        }

        let scale_changed = self.scale.scale_received != old_scale_received
            || self.scale.scale_num != old_scale_num;

        // Skip if nothing has changed since the last successful reconfigure.
        if !scale_changed
            && self.scale.scale_num == self.scale.last_applied_scale
            && self.last_applied_logical == self.last_logical
        {
            return;
        }

        let (wp_w, wp_h) = (self.wp_resolution[0] as f32, self.wp_resolution[1] as f32);

        // Compute layer-surface size from fit mode + logical size.
        let fit_mode = self.fit_mode;
        let (layer_w, layer_h) = match fit_mode {
            super::FitMode::Stretch => (log_w, log_h),
            _ => {
                let s = match fit_mode {
                    super::FitMode::Cover => f32::max(log_w as f32 / wp_w, log_h as f32 / wp_h),
                    super::FitMode::Contain => f32::min(log_w as f32 / wp_w, log_h as f32 / wp_h),
                    // A fit mode the adapter does not know: the surface
                    // keeps the size of the wallpaper instead of taking
                    // the process down.
                    _ => {
                        self.report_failure("the fit mode of the wallpaper is not known");
                        1.0
                    }
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

        let Some(ref layer) = self.last_layer else {
            return;
        };
        layer.set_size(layer_w, layer_h);
        let _ = layer.set_buffer_scale(1);

        self.app.resize([phys_w, phys_h]);

        self.needs_render = true;

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
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
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
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
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
///
/// The wallpaper is rendered until the display server closes the
/// connection.  Every failure is reported: the adapter hands the error
/// that caused it back to the caller.
pub fn start(config: AdapterConfig) -> Result<(), EngineError> {
    run(config, None)
}

/// Runs the wallpaper, taking the commands of the host into account.
///
/// The commands are polled every iteration of the loop, so the host is
/// never left waiting for more than a frame — and for more than a few
/// milliseconds in the static mode, where the loop does not draw a frame
/// every tick.
pub fn run(
    config: AdapterConfig,
    commands: Option<Receiver<EngineCommand>>,
) -> Result<(), EngineError> {
    let conn = Connection::connect_to_env()
        .map_err(|error| EngineError::Window(WindowError::Connect(error)))?;
    let (globals, mut event_queue) = registry_queue_init(&conn)
        .map_err(|error| EngineError::Window(WindowError::Globals(error)))?;
    let qh = event_queue.handle();

    // --- Bind protocols ---

    let compositor_state = CompositorState::bind(&globals, &qh)
        .map_err(|error| EngineError::Window(WindowError::Bind(error)))?;
    let layer_shell = LayerShell::bind(&globals, &qh)
        .map_err(|error| EngineError::Window(WindowError::Bind(error)))?;
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
    let viewporter: Option<WpViewporter> = globals.bind(&qh, 1..=1, FractionalScaleData).ok();
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

    let raw_display_handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
        NonNull::new(conn.backend().display_ptr() as *mut _)
            .ok_or(EngineError::Failed("the Wayland display handle is null"))?,
    ));
    let raw_window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
        NonNull::new(layer.wl_surface().id().as_ptr() as *mut _)
            .ok_or(EngineError::Failed("the Wayland surface handle is null"))?,
    ));

    let mut app = block_on(WgpuApp::new(
        config.source.clone(),
        InitAppSurface::Raw((raw_display_handle, raw_window_handle)),
        [256, 256],
        config.no_effects,
        config.no_mdl,
        config.assets_path.clone(),
        config.show_progress,
    ))?;
    // The rate at which the wallpaper is rendered: the videos are played
    // back at that rate.
    app.target_fps = config.target_fps;
    app.load()?;
    let wp_res = app
        .resolution
        .ok_or(EngineError::Failed("the wallpaper has no resolution"))?;

    let mut state = WlrState {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        app,
        fit_mode: config.fit_mode,
        wp_resolution: wp_res,
        scale: ScaleState::new(frac_mgr, frac_scale, viewporter, viewport),
        last_logical: None,
        last_layer: None,
        last_applied_logical: None,
        target_fps: config.target_fps,
        last_frame: None,
        needs_render: true,
        reported_failures: Vec::new(),
    };

    let mut control = Control::new(config.status, commands);
    let mut frame_count: u64 = 0;

    // The wallpaper is up: the host is told about it, together with the
    // files that could not be used.
    control.report(&mut state, true);

    loop {
        // The commands of the host are polled every iteration: the host
        // never waits for more than a frame.
        if !control.poll(&mut state)? {
            break;
        }

        // ── Static image mode (target_fps == 0) ──────────────────────
        // Only render when the compositor asks for a reconfigure; the
        // events are dispatched all the same, so the host stays in
        // control.
        if state.target_fps == Some(0) {
            // The static mode does not spin: it waits a little and
            // dispatches the events that arrived.  The wait is short
            // (50 ms), so the commands of the host are noticed well
            // within the 200 ms a host expects.
            std::thread::sleep(Duration::from_millis(50));
            dispatch(&mut event_queue, &mut state)?;

            // The problems of the wallpaper — a file that could not be
            // used, a failure of the protocol — are reported to the host.
            control.report(&mut state, false);

            if state.needs_render && !control.paused {
                state.needs_render = false;
                state.last_frame = Some(Instant::now());
                control.render(&mut state, &mut frame_count)?;
            }
            continue;
        }

        // ── Normal / animated mode ───────────────────────────────────
        dispatch(&mut event_queue, &mut state)?;

        // The problems of the wallpaper — a file that could not be used,
        // a failure of the protocol — are reported to the host.
        control.report(&mut state, false);

        if control.paused {
            // The wallpaper is paused: the events are still dispatched,
            // but nothing is drawn.  The loop waits a little, so it does
            // not spin while it waits for the host.
            if !control.wait(&mut state, Duration::from_millis(50))? {
                break;
            }
            continue;
        }

        // --- FPS limiting ---
        if let Some(target_fps) = state.target_fps
            && target_fps > 0
            && let Some(last) = state.last_frame
        {
            let min_delta = Duration::from_secs_f64(1.0 / target_fps as f64);
            if let Some(remaining) = min_delta.checked_sub(last.elapsed()) {
                // The wait is done in small steps and the commands of the
                // host are polled in between: a wallpaper that is
                // rendered at a low rate is still in control within a few
                // milliseconds.
                if !control.wait(&mut state, remaining)? {
                    break;
                }
            }
        }

        // Record frame start so the next throttle check measures
        // from here, not from after render (which would cause the
        // render time to eat into the frame budget).
        state.last_frame = Some(Instant::now());

        control.render(&mut state, &mut frame_count)?;

        // The problems the renderer ran into — an effect that could not
        // be drawn — are reported to the host.
        control.report(&mut state, false);
    }

    Ok(())
}

/// Dispatches the events that arrived from the display server.
fn dispatch(
    event_queue: &mut EventQueue<WlrState>,
    state: &mut WlrState,
) -> Result<(), EngineError> {
    event_queue
        .dispatch_pending(state)
        .map(|_| ())
        .map_err(|error| EngineError::Window(WindowError::Dispatch(error)))
}

/// The control of the render loop: the commands of the host and what the
/// wallpaper has to tell it.
struct Control {
    /// The commands of the host.
    commands: Option<Receiver<EngineCommand>>,
    /// Where the status of the wallpaper is sent.
    status: Option<Sender<EngineStatus>>,
    /// Whether the wallpaper is paused.
    paused: bool,
    /// The frames that could not be drawn.
    ///
    /// They are counted and reported to the host: a frame that cannot be
    /// drawn is a failure the user must know about.
    failures: RenderFailures,
}

impl Control {
    /// Takes over the commands of the host and the channel of the status.
    fn new(
        status: Option<Sender<EngineStatus>>,
        commands: Option<Receiver<EngineCommand>>,
    ) -> Self {
        Self {
            commands,
            status: status.clone(),
            paused: false,
            failures: RenderFailures::new(status),
        }
    }

    /// Handles the commands of the host.
    ///
    /// Returns `false` when the wallpaper must stop: the host asked it
    /// to, or the host is gone (its end of the channel was dropped).
    fn poll(&mut self, state: &mut WlrState) -> Result<bool, EngineError> {
        loop {
            // The command is taken out of the channel first: the borrow
            // of the channel must not live longer than the command.
            let command = match &self.commands {
                Some(commands) => match commands.try_recv() {
                    Ok(command) => command,
                    Err(TryRecvError::Empty) => return Ok(true),
                    Err(TryRecvError::Disconnected) => {
                        // The host is gone: the wallpaper stops with it.
                        log::info!("the host is gone: the wallpaper stops");
                        return Ok(false);
                    }
                },
                None => return Ok(true),
            };

            if !self.handle(command, state)? {
                return Ok(false);
            }
        }
    }

    /// Handles one command of the host.
    ///
    /// Returns `false` when the wallpaper must stop.
    fn handle(
        &mut self,
        command: EngineCommand,
        state: &mut WlrState,
    ) -> Result<bool, EngineError> {
        match command {
            EngineCommand::Stop => Ok(false),
            EngineCommand::Pause => {
                self.paused = true;
                Ok(true)
            }
            EngineCommand::Resume => {
                self.paused = false;
                Ok(true)
            }
            EngineCommand::SetFps(fps) => {
                // The rate of the loop and the rate the videos are played
                // back at are the same.
                state.target_fps = fps;
                state.app.target_fps = fps;
                Ok(true)
            }
            EngineCommand::Switch(source) => {
                self.switch(state, source)?;
                Ok(true)
            }
        }
    }

    /// Loads another wallpaper.
    ///
    /// The device, the surface and the audio stream stay: only the scene
    /// is replaced.  A wallpaper that cannot be loaded is reported to the
    /// host; the wallpaper that was running keeps running.
    fn switch(&mut self, state: &mut WlrState, source: WallpaperSource) -> Result<(), EngineError> {
        log::info!("switching to '{}'", source.path().display());
        state.app.source = source;

        match state.app.load() {
            Ok(()) => {
                // The new wallpaper may have another resolution: the
                // layer surface is resized to fit it.
                if let Some(resolution) = state.app.resolution {
                    state.wp_resolution = resolution;
                    state.last_applied_logical = None;
                    state.reconfigure();
                }
                self.report(state, true);
            }
            Err(error) => {
                log::error!("the wallpaper could not be loaded: {}", error.trace());
                if let Some(status) = &self.status {
                    let _ = status.send(EngineStatus::Failed(error));
                }
            }
        }

        Ok(())
    }

    /// Waits for the next frame.
    ///
    /// The wait is done in small steps and the commands of the host are
    /// polled in between, so a wallpaper that is rendered at a low rate
    /// is still in control within a few milliseconds.
    ///
    /// Returns `false` when the wallpaper must stop.
    fn wait(&mut self, state: &mut WlrState, duration: Duration) -> Result<bool, EngineError> {
        let deadline = Instant::now() + duration;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Ok(true);
            }
            std::thread::sleep((deadline - now).min(Duration::from_millis(50)));
            if !self.poll(state)? {
                return Ok(false);
            }
        }
    }

    /// Draws a frame of the wallpaper.
    ///
    /// A frame that cannot be drawn is counted and reported: the user is
    /// told that something is wrong instead of watching a frozen
    /// wallpaper.
    fn render(&mut self, state: &mut WlrState, frame_count: &mut u64) -> Result<(), EngineError> {
        log::trace!("frame {}: calling render...", frame_count);

        if let Err(error) = state.app.render() {
            // The frame that could not be drawn is counted and reported:
            // the same failure is reported once, the user does not have
            // to be told about every single frame that was skipped.
            self.failures.record(error, state.app.title.as_deref());
        }

        *frame_count = frame_count.wrapping_add(1);
        Ok(())
    }

    /// Sends the status of the wallpaper to the host.
    ///
    /// The problems that appeared since the last status are sent with it:
    /// a problem is reported once, never twice.  The host adds them up
    /// and shows them to the user.
    ///
    /// `force` sends the status even when there is nothing new to say:
    /// the first status tells the host that the wallpaper is up.
    fn report(&mut self, state: &mut WlrState, force: bool) {
        let problems = std::mem::take(&mut state.app.problems);
        if problems.is_empty() && !force {
            return;
        }

        let Some(status) = &self.status else {
            return;
        };

        let _ = status.send(EngineStatus::Running {
            title: state.app.title.clone(),
            problems,
        });
    }
}
