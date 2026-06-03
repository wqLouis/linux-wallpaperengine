use std::{fmt::Debug, sync::Arc};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wgpu::*;

#[derive(Debug)]
pub struct AppSurface {
    pub surface: Surface<'static>,
    pub config: SurfaceConfiguration,
}

pub enum InitAppSurface {
    Raw((RawDisplayHandle, RawWindowHandle)),
    Winit(Arc<winit::window::Window>),
}

impl AppSurface {
    pub fn new(
        surface: InitAppSurface,
        instance: &Instance,
        adapter: &Adapter,
        size: [u32; 2],
    ) -> Self {
        let wgpu_surface: Surface<'_> = match surface {
            InitAppSurface::Raw((raw_display_handle, raw_window_handle)) => unsafe {
                instance
                    .create_surface_unsafe(SurfaceTargetUnsafe::RawHandle {
                        raw_display_handle,
                        raw_window_handle,
                    })
                    .unwrap()
            },
            InitAppSurface::Winit(window) => instance.create_surface(window).unwrap(),
        };

        let cap = wgpu_surface.get_capabilities(adapter);

        // Pick the most power-friendly present mode the adapter actually
        // supports. FifoRelaxed lets the GPU skip frames when the scene
        // is idle; Fifo strictly vsyncs. Mailbox/Immediate are last-resort
        // fallbacks — Mailbox replaces queued frames (spins the GPU when
        // the app renders as fast as possible), Immediate has no vsync.
        let present_mode = pick_present_mode(&cap.present_modes);

        Self {
            surface: wgpu_surface,
            config: SurfaceConfiguration {
                usage: TextureUsages::RENDER_ATTACHMENT,
                format: cap.formats[0],
                width: size[0],
                height: size[1],
                present_mode,
                alpha_mode: CompositeAlphaMode::Auto,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
        }
    }
}

/// Choose the present mode that minimises GPU work for an idle wallpaper.
/// Preference order: FifoRelaxed > Fifo > Mailbox > Immediate > first supported.
fn pick_present_mode(supported: &[PresentMode]) -> PresentMode {
    const PREFERRED: &[PresentMode] = &[
        PresentMode::FifoRelaxed,
        PresentMode::Fifo,
        PresentMode::Mailbox,
        PresentMode::Immediate,
    ];
    for mode in PREFERRED {
        if supported.contains(mode) {
            return *mode;
        }
    }
    supported.first().copied().unwrap_or(PresentMode::Fifo)
}
