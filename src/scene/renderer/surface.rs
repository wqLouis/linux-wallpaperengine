use std::{fmt::Debug, sync::Arc};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wgpu::*;

/// Where the scene is rendered to.
#[derive(Debug)]
pub struct AppSurface {
    pub target: SurfaceTarget,
    pub config: SurfaceConfiguration,
}

/// The concrete render target of an [AppSurface].
///
/// * [SurfaceTarget::Swapchain] — a real window swapchain (winit window or
///   wlr layer surface).  The rendered frame is presented by the compositor.
/// * [SurfaceTarget::Offscreen] — an off-screen texture.  Used by the GNOME
///   backend, where the frame is not presented by the compositor but copied
///   back to the CPU and handed over to a GNOME Shell extension.
#[derive(Debug)]
pub enum SurfaceTarget {
    Swapchain(Surface<'static>),
    Offscreen(OffscreenTarget),
}

/// An off-screen render target (texture + view).
#[derive(Debug)]
pub struct OffscreenTarget {
    pub texture: Texture,
    pub view: TextureView,
    pub format: TextureFormat,
    pub usage: TextureUsages,
}

pub enum InitAppSurface {
    Raw((RawDisplayHandle, RawWindowHandle)),
    Winit(Arc<winit::window::Window>),
    /// Render into an off-screen texture instead of a window swapchain.
    Offscreen {
        width: u32,
        height: u32,
    },
}

impl AppSurface {
    /// Creates the render target of the app.
    ///
    /// The surface of a window is created by the graphics driver: when it
    /// refuses, the failure is reported and the caller decides what to do.
    /// Nothing here panics.
    pub fn new(
        surface: InitAppSurface,
        instance: &Instance,
        adapter: &Adapter,
        device: &Device,
        size: [u32; 2],
    ) -> Result<Self, wgpu::CreateSurfaceError> {
        match surface {
            InitAppSurface::Offscreen { width, height } => {
                Ok(Self::new_offscreen(device, width.max(1), height.max(1)))
            }
            InitAppSurface::Raw((raw_display_handle, raw_window_handle)) => {
                let wgpu_surface: Surface<'_> = unsafe {
                    instance.create_surface_unsafe(SurfaceTargetUnsafe::RawHandle {
                        raw_display_handle,
                        raw_window_handle,
                    })
                }?;

                let cap = wgpu_surface.get_capabilities(adapter);

                // Pick the most power-friendly present mode the adapter actually
                // supports. FifoRelaxed lets the GPU skip frames when the scene
                // is idle; Fifo strictly vsyncs. Mailbox/Immediate are last-resort
                // fallbacks — Mailbox replaces queued frames (spins the GPU when
                // the app renders as fast as possible), Immediate has no vsync.
                let present_mode = pick_present_mode(&cap.present_modes);

                Ok(Self {
                    target: SurfaceTarget::Swapchain(wgpu_surface),
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
                })
            }
            InitAppSurface::Winit(window) => {
                let wgpu_surface = instance.create_surface(window)?;
                let cap = wgpu_surface.get_capabilities(adapter);
                let present_mode = pick_present_mode(&cap.present_modes);

                Ok(Self {
                    target: SurfaceTarget::Swapchain(wgpu_surface),
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
                })
            }
        }
    }

    /// Create an off-screen render target.
    ///
    /// The texture is sRGB encoded, so the bytes read back from it can be
    /// handed to a compositor (or a GNOME Shell extension) as-is.
    fn new_offscreen(device: &Device, width: u32, height: u32) -> Self {
        let format = TextureFormat::Rgba8UnormSrgb;
        let usage = TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC;
        let (texture, view) = create_offscreen_texture(device, width, height, format, usage);

        Self {
            target: SurfaceTarget::Offscreen(OffscreenTarget {
                texture,
                view,
                format,
                usage,
            }),
            config: SurfaceConfiguration {
                usage,
                format,
                width,
                height,
                present_mode: PresentMode::Fifo,
                alpha_mode: CompositeAlphaMode::Opaque,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
        }
    }

    /// Resize the render target.
    ///
    /// Swapchain targets are re-configured, off-screen targets get a fresh
    /// texture of the new size.
    pub fn resize(&mut self, device: &Device, size: [u32; 2]) {
        let width = size[0].max(1);
        let height = size[1].max(1);

        self.config.width = width;
        self.config.height = height;

        match &mut self.target {
            SurfaceTarget::Swapchain(surface) => {
                surface.configure(device, &self.config);
            }
            SurfaceTarget::Offscreen(target) => {
                let (texture, view) =
                    create_offscreen_texture(device, width, height, target.format, target.usage);
                target.texture = texture;
                target.view = view;
            }
        }
    }

    /// The off-screen target, if this surface renders off-screen.
    pub fn offscreen(&self) -> Option<&OffscreenTarget> {
        match &self.target {
            SurfaceTarget::Offscreen(target) => Some(target),
            SurfaceTarget::Swapchain(_) => None,
        }
    }
}

/// Create a texture + view usable as a render attachment.
fn create_offscreen_texture(
    device: &Device,
    width: u32,
    height: u32,
    format: TextureFormat,
    usage: TextureUsages,
) -> (Texture, TextureView) {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("offscreen target"),
        size: Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });

    let view = texture.create_view(&TextureViewDescriptor::default());

    (texture, view)
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
