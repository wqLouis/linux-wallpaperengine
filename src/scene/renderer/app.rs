use std::time::Instant;

use glam::Vec3;
use wgpu::*;

use crate::{MAX_INDEX, MAX_TEXTURE, MAX_VERTEX};

use super::{
    buffer::Buffers,
    draw::{DrawObject, DrawQueue},
    intermediate_pass,
    post_process::PostProcess,
    projection::ProjectionBindGroups,
    surface::AppSurface,
};

pub use super::surface::InitAppSurface;

pub struct WgpuApp {
    pub surface: AppSurface,
    pub buffers: Buffers,
    pub projection_bindgroup: ProjectionBindGroups,
    pub scene_path: String,
    pub clear_color: Vec3,
    pub device: Device,
    pub queue: Queue,
    pub audio_stream: rodio::OutputStream,
    pub draw_queue: Option<DrawQueue>,
    pub post_process: Option<PostProcess>,
    pub resolution: Option<[u32; 2]>,
    pub start_time: Instant,
    pub projection_matrix: [[f32; 4]; 4],
}

impl WgpuApp {
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
            surface, buffers, projection_bindgroup, scene_path,
            clear_color: Vec3::ZERO, device, queue, audio_stream,
            draw_queue: None, resolution: None, post_process: None,
            start_time: Instant::now(), projection_matrix: [[1.0; 4]; 4],
        }
    }

    pub fn render(&mut self) -> Option<()> {
        let elapsed = self.start_time.elapsed().as_secs_f32();

        let draw_queue = self.draw_queue.as_ref()?;
        let post_process = self.post_process.as_ref()?;
        let screen_res = [self.surface.config.width, self.surface.config.height];

        write_effect_uniforms(&self.queue, draw_queue.queue.as_ref(), elapsed, &self.projection_matrix, screen_res);

        let has_multi = draw_queue.queue.iter().any(|o| o.intermediates.is_some());
        if has_multi {
            intermediate_pass::render_intermediate_passes(
                &self.device, &self.queue, &self.buffers,
                &self.projection_bindgroup, &self.projection_matrix,
                draw_queue, post_process, elapsed, screen_res,
            );
        }

        render_final_pass(
            &self.device, &self.queue, &self.surface,
            &self.buffers, &self.projection_bindgroup,
            draw_queue, post_process, self.clear_color,
        )
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        self.surface.config.width = size[0];
        self.surface.config.height = size[1];
        self.surface.surface.configure(&self.device, &self.surface.config);
    }
}

fn render_final_pass(
    device: &Device,
    queue: &Queue,
    surface: &AppSurface,
    buffers: &Buffers,
    projection_bindgroup: &ProjectionBindGroups,
    draw_queue: &DrawQueue,
    post_process: &PostProcess,
    clear_color: Vec3,
) -> Option<()> {
    let output = surface.surface.get_current_texture().unwrap();
    let view = output.texture.create_view(&TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());

    {
        let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color {
                        r: (clear_color.x / 255.0) as f64,
                        g: (clear_color.y / 255.0) as f64,
                        b: (clear_color.z / 255.0) as f64,
                        a: 1.0,
                    }),
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });

        render_pass.set_vertex_buffer(0, buffers.vertex.slice(..));
        render_pass.set_index_buffer(buffers.index.slice(..), IndexFormat::Uint32);
        render_pass.set_bind_group(1, projection_bindgroup.projection.as_ref(), &[]);

        for draw_object in draw_queue.queue.iter() {
            render_pass.set_pipeline(&draw_queue.image_pipeline);
            if let Some(ref pp) = draw_object.intermediates {
                let final_bg = pp.make_bindgroup(device, &post_process.layout, &post_process.sampler);
                render_pass.set_bind_group(0, &final_bg, &[]);
            } else {
                render_pass.set_bind_group(0, &draw_object.bindgroup, &[]);
            }
            render_pass.draw_indexed(
                draw_object.index_range[0]..draw_object.index_range[1],
                0, 0..1,
            );
        }
    }

    queue.submit(Some(encoder.finish()));
    output.present();
    Some(())
}

pub fn write_effect_uniforms(
    queue: &Queue,
    objects: &[DrawObject],
    elapsed: f32,
    projection: &[[f32; 4]; 4],
    screen_res: [u32; 2],
) {
    use crate::scene::renderer::post_processor::effect_param::SystemUniforms;

    for draw_object in objects {
        for effect_bg in &draw_object.effect_bindgroups {
            if let Some(ref buf) = effect_bg.uniform_buffer {
                let buf_size = effect_bg.uniform_layout.total_size() as usize;
                let mut staging = vec![0u8; buf_size];
                let sys = SystemUniforms {
                    screen_resolution: screen_res,
                    tex_resolutions: effect_bg.tex_resolutions.clone(),
                };
                effect_bg.uniform_layout.populate_effect_params(
                    &mut staging,
                    &effect_bg.constants,
                    &effect_bg.material_keys,
                    elapsed, projection, &sys,
                );
                queue.write_buffer(buf, 0, &staging);
            }
        }
    }
}
