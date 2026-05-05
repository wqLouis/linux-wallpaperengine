use smithay_client_toolkit::{
    compositor::CompositorHandler,
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_seat,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{Capability, SeatHandler, SeatState},
    shell::wlr_layer::{LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
};
use wayland_client::{
    Connection, QueueHandle,
    protocol::{wl_output, wl_seat, wl_surface},
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
