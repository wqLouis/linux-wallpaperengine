use std::{fmt::Debug, io::Cursor, path::Path};

use crate::{
    MAX_INDEX, MAX_TEXTURE,
    scene::{
        loader::{
            object_loader::{ObjectMap, PlaybackMode},
            scene_loader::Scene,
        },
        renderer::{
            bindgroup::BindGroups,
            buffer::Buffers,
            draw::{DrawQueue, Vertex},
            projection::Projection,
        },
    },
};

use super::*;
use rodio::Source;
use wgpu::*;

pub struct WgpuApp {
    surface: AppSurface,

    buffers: buffer::Buffers,

    bindgroups: bindgroup::BindGroups,

    scene_path: String,

    device: Device,
    queue: Queue,
    pipeline: RenderPipeline,

    custom_pipelines: Vec<RenderPipeline>,

    audio_stream: rodio::OutputStream,
}

#[derive(Debug)]
struct AppSurface {
    surface: Surface<'static>,
    config: SurfaceConfiguration,
}

impl WgpuApp {
    pub async fn new(
        scene_path: String,
        surface: impl Into<SurfaceTarget<'static>>,
        size: [u32; 2],
    ) -> Self {
        // init basic 2d scene rendering
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
        let buffers = Buffers::new(&device);
        let bindgroups = BindGroups::new(&device);

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: None,
            source: ShaderSource::Wgsl(include_str!("./shader/image.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&bindgroups.texture_layout, &bindgroups.projection_layout],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Vertex::create_buffer_layout()],
            },
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: Some(Face::Back),
                unclipped_depth: false,
                polygon_mode: PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: surface.config.format,
                    blend: Some(BlendState {
                        color: BlendComponent {
                            src_factor: BlendFactor::SrcAlpha,
                            dst_factor: BlendFactor::OneMinusSrcAlpha,
                            operation: BlendOperation::Add,
                        },
                        alpha: BlendComponent::OVER,
                    }),
                    write_mask: ColorWrites::all(),
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let audio_stream = rodio::OutputStreamBuilder::open_default_stream().unwrap();

        Self {
            surface,
            buffers,
            bindgroups,
            scene_path,
            device,
            queue,
            pipeline,
            custom_pipelines: Vec::new(),
            audio_stream: audio_stream,
        }
    }

    pub fn load(&mut self) {
        let mut scene = Scene::new(self.scene_path.clone());

        let mut draw_queue = DrawQueue::new();
        let object_map = ObjectMap::new(&scene.root.objects);

        for tex in object_map.texture {
            draw_queue.push(tex, &scene.jsons, &scene.textures);
        }

        self.bindgroups
            .create_texture_bindgroup(&mut draw_queue, &self.device, &self.queue);
        self.bindgroups.create_projection_bindgroup(
            &self.buffers,
            &self.device,
            &self.queue,
            &Projection::new(&scene.root).create_camera_uniform(),
        );

        let audio_stream = &self.audio_stream;
        let audio_mixer = audio_stream.mixer();
        let audio_sink = rodio::Sink::connect_new(audio_mixer);

        for audio in object_map.audio {
            for sound in audio.sounds {
                let Some(raw) = scene.desc.remove(&sound) else {
                    continue;
                };

                let cursor = Cursor::new(raw);
                let sound_pathbuf = Path::new(&sound).to_path_buf();
                let hint = sound_pathbuf.extension().unwrap().to_str().unwrap();
                let Some(source) = rodio::decoder::Decoder::builder()
                    .with_data(cursor)
                    .with_hint(hint)
                    .build()
                    .ok()
                else {
                    println!("failed to build audio: {:?} with hint: {:?}", sound, hint);
                    continue;
                };

                match audio.playback_mode {
                    PlaybackMode::Loop => {
                        audio_mixer.add(source.repeat_infinite());
                    }
                    PlaybackMode::Others(_) => {}
                }
            }
        }

        std::thread::spawn(move || {
            audio_sink.play();
            audio_sink.set_volume(1.0);
            audio_sink.sleep_until_end();
        });

        draw_queue.submit_draw_queue(&mut self.buffers, &self.queue);
    }

    pub fn render(&mut self) -> Result<(), SurfaceError> {
        let output = self.surface.surface.get_current_texture()?;
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
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: StoreOp::Store,
                    },
                })],
                ..Default::default()
            });

            render_pass.set_pipeline(&self.pipeline);
            if self.buffers.index_len > 0 {
                render_pass.set_vertex_buffer(0, self.buffers.vertex.slice(..));
                render_pass.set_index_buffer(self.buffers.index.slice(..), IndexFormat::Uint16);
                render_pass.set_bind_group(0, &self.bindgroups.texture, &[]);
                render_pass.set_bind_group(1, &self.bindgroups.projection, &[]);
                render_pass.draw_indexed(0..MAX_INDEX, 0, 0..1);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        output.present();

        Ok(())
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
        surface: impl Into<SurfaceTarget<'static>>,
        instance: &Instance,
        adapter: &Adapter,
        size: [u32; 2],
    ) -> Self {
        let surface = instance.create_surface(surface).unwrap();

        let cap = surface.get_capabilities(adapter);

        Self {
            surface: surface,
            config: SurfaceConfiguration {
                usage: TextureUsages::RENDER_ATTACHMENT,
                format: cap.formats[0],
                width: size[0],
                height: size[1],
                present_mode: PresentMode::AutoNoVsync,
                alpha_mode: cap.alpha_modes[0],
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
        }
    }
}
