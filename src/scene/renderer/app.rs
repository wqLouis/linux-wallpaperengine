use std::{
    fmt::Debug,
    rc::Rc,
    sync::Arc,
    time::Instant,
};

use bytemuck::bytes_of;
use crate::{
    MAX_INDEX, MAX_TEXTURE, MAX_VERTEX,
    scene::renderer::{
        buffer,
        draw::{DrawObject, DrawQueue, EffectBindGroup},
        post_process::PostProcess,
        post_processor::{
            effect_param::{populate_effect_params, SystemUniforms},
            pipeline_handler::EffectPipelineData,
            shader_preprocessor::WM_SAMPLER_BINDING,
        },
        projection::ProjectionBindGroups,
    },
};

use super::*;
use buffer::Buffers;
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

    pub start_time: Instant,
    pub projection_matrix: [[f32; 4]; 4],
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
            audio_stream,
            draw_queue: None,
            resolution: None,
            post_process: None,
            start_time: Instant::now(),
            projection_matrix: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn render(&mut self) -> Option<()> {
        let elapsed = self.start_time.elapsed().as_secs_f32();
        let identity: [[f32; 4]; 4] = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];

        let draw_queue = self.draw_queue.as_ref()?;
        let post_process = self.post_process.as_ref()?;

        if elapsed < 3.0 {
            eprintln!("[frame {:.3}s] objs: noeff={} single={} multi={}",
                elapsed,
                draw_queue.queue.iter().filter(|o| o.effect_bindgroups.is_empty()).count(),
                draw_queue.queue.iter().filter(|o| o.effect_bindgroups.len() == 1).count(),
                draw_queue.queue.iter().filter(|o| o.effect_bindgroups.len() >= 2).count(),
            );
        }

        fn write_effect_uniforms(
            queue: &Queue,
            objects: &[DrawObject],
            elapsed: f32,
            projection: &[[f32; 4]; 4],
            screen_res: [u32; 2],
        ) {
            for draw_object in objects {
                for effect_bg in &draw_object.effect_bindgroups {
                    if let Some(ref buf) = effect_bg.uniform_buffer {
                        let buf_size = effect_bg.uniform_layout.total_size() as usize;
                        let mut staging = vec![0u8; buf_size];
                        let sys = SystemUniforms {
                            screen_resolution: screen_res,
                            tex_resolutions: effect_bg.tex_resolutions.clone(),
                        };
                        populate_effect_params(
                            &effect_bg.uniform_layout,
                            &mut staging,
                            &effect_bg.constants,
                            &effect_bg.material_keys,
                            elapsed,
                            projection,
                            &sys,
                        );
                        queue.write_buffer(buf, 0, &staging);
                    }
                }
            }
        }

        let screen_res = self.resolution.unwrap_or([1920, 1080]);
        write_effect_uniforms(&self.queue, draw_queue.queue.as_ref(), elapsed, &self.projection_matrix, screen_res);

        let has_multi = draw_queue.queue.iter().any(|o| o.intermediates.is_some());
        if has_multi {
            self.queue.write_buffer(&self.buffers.projection, 0, bytes_of(&identity));
            write_effect_uniforms(&self.queue, draw_queue.queue.as_ref(), elapsed, &identity, screen_res);

            let mut inter_encoder = self.device.create_command_encoder(&CommandEncoderDescriptor::default());

            for draw_object in draw_queue.queue.iter() {
                let Some(ref pp) = draw_object.intermediates else {
                    continue;
                };

                let n_effects = draw_object.effect_bindgroups.len();

                {
                    let mut pass = inter_encoder.begin_render_pass(&RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &pp.view_a,
                            depth_slice: None,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Clear(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }),
                                store: StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    });
                    pass.set_pipeline(&draw_queue.image_pipeline);
                    pass.set_vertex_buffer(0, pp.ndc_vbuf.slice(..));
                    pass.set_index_buffer(pp.ndc_ibuf.slice(..), IndexFormat::Uint32);
                    pass.set_bind_group(0, &draw_object.bindgroup, &[]);
                    pass.set_bind_group(1, self.projection_bindgroup.projection.as_ref(), &[]);
                    pass.draw_indexed(0..6, 0, 0..1);
                }

                let n_effects = draw_object.effect_bindgroups.len();
                let mut source_view = &pp.view_a;
                let mut target_view = &pp.view_b;

                for (i, effect_bg) in draw_object.effect_bindgroups.iter().enumerate() {
                    let pipedata = draw_queue.render_pipelines.values().find(|d| {
                        Rc::ptr_eq(&d.pipeline, &effect_bg.pipeline)
                    });

                    let is_last = i == n_effects - 1;

                    {
                        let mut pass = inter_encoder.begin_render_pass(&RenderPassDescriptor {
                            label: None,
                            color_attachments: &[Some(RenderPassColorAttachment {
                                view: target_view,
                                depth_slice: None,
                                resolve_target: None,
                                ops: Operations {
                                    load: LoadOp::Clear(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }),
                                    store: StoreOp::Store,
                                },
                            })],
                            ..Default::default()
                        });
                        pass.set_pipeline(&effect_bg.pipeline);
                        pass.set_vertex_buffer(0, pp.ndc_vbuf.slice(..));
                        pass.set_index_buffer(pp.ndc_ibuf.slice(..), IndexFormat::Uint32);
                        pass.set_bind_group(1, self.projection_bindgroup.projection.as_ref(), &[]);

                        let inter_bg = make_effect_intermediate_bindgroup(
                            &self.device,
                            pipedata,
                            effect_bg,
                            source_view,
                            &post_process.sampler,
                        );
                        pass.set_bind_group(0, &inter_bg, &[]);
                        pass.draw_indexed(0..6, 0, 0..1);
                    }

                    if !is_last {
                        std::mem::swap(&mut source_view, &mut target_view);
                    }
                }

                if n_effects % 2 == 1 {
                    let bg = pp.make_bindgroup_for(&self.device, &post_process.layout, &post_process.sampler, target_view);
                    let mut pass = inter_encoder.begin_render_pass(&RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &pp.view_a,
                            depth_slice: None,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Clear(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }),
                                store: StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    });
                    pass.set_pipeline(&draw_queue.image_pipeline);
                    pass.set_vertex_buffer(0, pp.ndc_vbuf.slice(..));
                    pass.set_index_buffer(pp.ndc_ibuf.slice(..), IndexFormat::Uint32);
                    pass.set_bind_group(0, &bg, &[]);
                    pass.set_bind_group(1, self.projection_bindgroup.projection.as_ref(), &[]);
                    pass.draw_indexed(0..6, 0, 0..1);
                }
            }

            self.queue.submit(Some(inter_encoder.finish()));
            self.queue.write_buffer(&self.buffers.projection, 0, bytes_of(&self.projection_matrix));
            write_effect_uniforms(&self.queue, draw_queue.queue.as_ref(), elapsed, &self.projection_matrix, screen_res);
        }

        let output = self.surface.surface.get_current_texture().unwrap();
        let view = output.texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor::default());

        {
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

            render_pass.set_vertex_buffer(0, self.buffers.vertex.slice(..));
            render_pass.set_index_buffer(self.buffers.index.slice(..), IndexFormat::Uint32);
            render_pass.set_bind_group(1, self.projection_bindgroup.projection.as_ref(), &[]);

            for draw_object in draw_queue.queue.iter() {
                if let Some(ref pp) = draw_object.intermediates {
                    let final_bg = pp.make_bindgroup(&self.device, &post_process.layout, &post_process.sampler);
                    render_pass.set_pipeline(&draw_queue.image_pipeline);
                    render_pass.set_bind_group(0, &final_bg, &[]);
                } else {
                    render_pass.set_pipeline(&draw_queue.image_pipeline);
                    render_pass.set_bind_group(0, &draw_object.bindgroup, &[]);
                }
                render_pass.draw_indexed(
                    draw_object.index_range[0]..draw_object.index_range[1],
                    0,
                    0..1,
                );
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

fn make_effect_intermediate_bindgroup(
    device: &Device,
    pipedata: Option<&EffectPipelineData>,
    effect_bg: &EffectBindGroup,
    source_view: &TextureView,
    sampler: &Sampler,
) -> BindGroup {
    let pipedata = pipedata.unwrap();
    let mut entries = Vec::new();

    for i in 0..pipedata.layout.sampler_count() {
        let view: &TextureView = if i == 0 {
            source_view
        } else {
            &effect_bg._blank_view
        };
        entries.push(BindGroupEntry {
            binding: i as u32 * 2,
            resource: BindingResource::TextureView(view),
        });
    }

    entries.push(BindGroupEntry {
        binding: WM_SAMPLER_BINDING,
        resource: BindingResource::Sampler(sampler),
    });

    if let Some(ref buf) = effect_bg.uniform_buffer {
        entries.push(BindGroupEntry {
            binding: pipedata.layout.uniform_binding,
            resource: buf.as_entire_binding(),
        });
    }

    device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &pipedata.bindgroup_layout,
        entries: &entries,
    })
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
