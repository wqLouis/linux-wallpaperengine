use smithay_client_toolkit::{
    compositor::CompositorHandler,
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::wlr_layer::{LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
};
use wayland_client::{
    Connection, QueueHandle,
    protocol::{wl_output, wl_pointer, wl_seat, wl_surface},
};

use crate::scene::renderer::app::WgpuApp;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    /// Scale wallpaper to fill entire output, cropping if aspect ratios differ
    Cover,
    /// Scale wallpaper to fit within output, letterboxing if aspect ratios differ
    Contain,
    /// Stretch wallpaper to exactly match output (ignores aspect ratio)
    Stretch,
}

pub struct Wgpu {
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub app: WgpuApp,
    pub fit_mode: FitMode,
    pub wp_resolution: [u32; 2],
    pub pointer: Option<wl_pointer::WlPointer>,
    pub surface: wl_surface::WlSurface,
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

        // Ignore initial (0, 0) configure from some compositors
        if new_width == 0 && new_height == 0 {
            return;
        }

        let (layer_w, layer_h) = match self.fit_mode {
            FitMode::Stretch => (new_width, new_height),
            _ => {
                let (wp_w, wp_h) = (self.wp_resolution[0] as f32, self.wp_resolution[1] as f32);
                let scale = match self.fit_mode {
                    FitMode::Cover => f32::max(new_width as f32 / wp_w, new_height as f32 / wp_h),
                    FitMode::Contain => f32::min(new_width as f32 / wp_w, new_height as f32 / wp_h),
                    _ => unreachable!(),
                };
                ((wp_w * scale).round() as u32, (wp_h * scale).round() as u32)
            }
        };

        layer.set_size(layer_w, layer_h);
        self.app.resize([layer_w, layer_h]);
        self.app.render().unwrap();
    }
}

impl SeatHandler for Wgpu {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            let pointer = self
                .seat_state
                .get_pointer(qh, &seat)
                .expect("Failed to create pointer");
            self.pointer = Some(pointer);
        }
    }
    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_some() {
            self.pointer.take().unwrap().release();
        }
    }
    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}
}

impl PointerHandler for Wgpu {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            // Only process events for our surface
            if &event.surface != &self.surface {
                continue;
            }
            match event.kind {
                PointerEventKind::Motion { .. } => {
                    let (sx, sy) = event.position;
                    // Normalize cursor position to [0, 1] range, (0,0) = top-left, as expected by g_ParallaxPosition
                    let nx = sx as f32 / self.wp_resolution[0] as f32;
                    let ny = sy as f32 / self.wp_resolution[1] as f32;
                    self.app.user_params =
                        crate::scene::renderer::app::UserParams {
                            cursor_position: [nx, ny],
                            cursor_pixel: [sx as u32, sy as u32],
                        };
                }
                _ => {}
            }
        }
    }
}

delegate_compositor!(Wgpu);
delegate_output!(Wgpu);
delegate_seat!(Wgpu);
delegate_pointer!(Wgpu);
delegate_layer!(Wgpu);
delegate_registry!(Wgpu);

impl ProvidesRegistryState for Wgpu {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}
