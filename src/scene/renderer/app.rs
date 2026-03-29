use std::{fmt::Debug, sync::Arc};

use crate::{
    MAX_INDEX, MAX_TEXTURE, MAX_VERTEX,
    scene::renderer::{
        buffer::Buffers, draw::DrawQueue, post_process::PostProcess,
        projection::ProjectionBindGroups,
    },
};

use super::*;
use glam::Vec3;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wgpu::*;

pub struct WgpuApp {
    pub surface: AppSurface,

    pub buffers: buffer::Buffers,

    pub projection_bindgroup: ProjectionBindGroups,

    pub scene_path: String,
    pub clear_color: Vec3,

    pub device: Device,
    pub queue: Queue,

    pub audio_stream: rodio::OutputStream,

    pub draw_queue: Option<DrawQueue>,
    pub post_process: Option<PostProcess>,
    pub resolution: Option<[u32; 2]>,
}

#[derive(Debug)]
pub struct AppSurface {
    pub surface: Surface<'static>,
    pub config: SurfaceConfiguration,
}

pub enum InitAppSurface {
    Raw((RawDisplayHandle, RawWindowHandle)),
    Winit(Arc<winit::window::Window>),
}

impl WgpuApp {
    /// init basic 2d scene rendering
    pub async fn new(scene_path: String, surface: InitAppSurface, size: [u32; 2]) -> Self {
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::VULKAN | Backends::METAL,
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: None,
                required_features: Features::TEXTURE_BINDING_ARRAY
                    | Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
                required_limits: Limits {
                    max_binding_array_elements_per_shader_stage: MAX_TEXTURE,
                    ..Default::default()
                },
                experimental_features: ExperimentalFeatures::disabled(),
                memory_hints: MemoryHints::MemoryUsage,
                trace: Trace::Off,
            })
            .await
            .unwrap();

        let surface = AppSurface::new(surface, &instance, &adapter, size);
        let buffers = Buffers::new(&device, MAX_INDEX as u64, MAX_VERTEX as u64);
        let projection_bindgroup = ProjectionBindGroups::new(&device);

        let audio_stream = rodio::OutputStreamBuilder::open_default_stream().unwrap();

        Self {
            surface,
            buffers,
            projection_bindgroup,
            scene_path,
            clear_color: Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            device,
            queue,
            audio_stream: audio_stream,
            draw_queue: None,
            resolution: None,
            post_process: None,
        }
    }

    pub fn render(&mut self) -> Option<()> {
        let output = self.surface.surface.get_current_texture().unwrap();
        let view = output
            .texture
            .create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                ..Default::default()
            });

        {
            // render pass
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color {
                            r: (self.clear_color.x / 255.0) as f64,
                            g: (self.clear_color.y / 255.0) as f64,
                            b: (self.clear_color.z / 255.0) as f64,
                            a: 1.0,
                        }),
                        store: StoreOp::Store,
                    },
                })],
                ..Default::default()
            });

            let Some(draw_queue) = &self.draw_queue else {
                return None;
            };

            render_pass.set_pipeline(&draw_queue.image_pipeline);
            render_pass.set_vertex_buffer(0, self.buffers.vertex.slice(..));
            render_pass.set_index_buffer(self.buffers.index.slice(..), IndexFormat::Uint32);
            render_pass.set_bind_group(1, &self.projection_bindgroup.projection, &[]);

            for draw_object in draw_queue.queue.iter() {
                if draw_object.pipelines.len() == 0 {
                    render_pass.set_bind_group(0, &draw_object.bindgroup, &[]);
                    render_pass.draw_indexed(
                        draw_object.index_range[0]..draw_object.index_range[1],
                        0,
                        0..1,
                    );
                }
            }
        }

        self.queue.submit(Some(encoder.finish()));
        output.present();

        Some(())
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        self.surface.config.width = size[0];
        self.surface.config.height = size[1];

        self.surface
            .surface
            .configure(&self.device, &self.surface.config);
    }
}

impl AppSurface {
    fn new(
        surface: InitAppSurface,
        instance: &Instance,
        adapter: &Adapter,
        size: [u32; 2],
    ) -> Self {
        let wgpu_surface: Surface<'_>;

        match surface {
            InitAppSurface::Raw((raw_display_handle, raw_window_handle)) => unsafe {
                wgpu_surface = instance
                    .create_surface_unsafe(SurfaceTargetUnsafe::RawHandle {
                        raw_display_handle,
                        raw_window_handle,
                    })
                    .unwrap();
            },
            InitAppSurface::Winit(window) => {
                wgpu_surface = instance.create_surface(window).unwrap();
            }
        }

        let cap = wgpu_surface.get_capabilities(adapter);

        Self {
            surface: wgpu_surface,
            config: SurfaceConfiguration {
                usage: TextureUsages::RENDER_ATTACHMENT,
                format: cap.formats[0],
                width: size[0],
                height: size[1],
                present_mode: PresentMode::Mailbox,
                alpha_mode: CompositeAlphaMode::Auto,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
        }
    }
}
