//! Asset loading: parses the .pkg scene, uploads textures and geometry,
//! creates render pipelines, and builds the draw queue.

use std::{io::Cursor, path::Path};

use crate::scene::{
    loader::{
        object_loader::{AudioObject, ObjectMap, PlaybackMode, TextureObject},
        scene_loader::Scene,
    },
    renderer::{
        app::WgpuApp, draw::DrawQueue, post_process::PostProcess, projection::Projection,
        vertex::Vertex,
    },
};

use wgpu::*;

use rodio::{OutputStream, Source};

impl WgpuApp {
    /// load assets
    pub fn load(&mut self) {
        let mut scene = Scene::new(self.scene_path.clone());

        // Enable lazy-loading fallback to Wallpaper Engine assets directory.
        if let Some(ref assets_path) = self.assets_path {
            log::info!(
                "Using Wallpaper Engine assets path: {}",
                assets_path
            );
            scene.set_assets_path(std::path::PathBuf::from(assets_path));
        }

        let size = [
            scene.root.general.orthogonalprojection.width as u32,
            scene.root.general.orthogonalprojection.height as u32,
        ];

        let post_process = PostProcess::new(
            &self.device,
            &self.queue,
            size,
            self.has_clear_texture,
        );

        self.clear_color = scene.root.general.clearcolor.parse().unwrap_or_default();

        let pipeline = create_pipeline(&self, &post_process.layout);
        let objects = ObjectMap::with_clear_color(
            &scene.root.objects.clone(),
            &scene,
            self.clear_color,
            self.no_mdl,
        );

        // Pre-compute total geometry so we can allocate GPU buffers once.
        let (total_verts, total_indices) = count_geometry(&objects.texture);
        log::info!(
            "allocating GPU buffers: {} verts, {} indices ({} bytes)",
            total_verts,
            total_indices,
            total_verts as usize * std::mem::size_of::<Vertex>()
                + total_indices as usize * std::mem::size_of::<u32>()
        );
        self.buffers = Some(crate::scene::renderer::buffer::Buffers::new(
            &self.device,
            total_indices as u64,
            total_verts as u64,
        ));

        let draw_queue = DrawQueue::new(
            &self.device,
            &self.queue,
            self.buffers.as_mut().unwrap(),
            &scene,
            objects.texture,
            pipeline,
            &post_process,
            &self.projection_bindgroup.projection_layout,
            self.no_effects,
            self.has_immediates,
            self.has_partially_bound,
            self.has_subgroup,
        );

        load_audios(&self.audio_stream, objects.audio, &scene);

        self.draw_queue = Some(draw_queue);

        let camera_uniform = Projection::new(&scene.root).create_camera_uniform();
        self.projection_bindgroup.create_projection_bindgroup(
            self.buffers.as_ref().unwrap(),
            &self.device,
            &self.queue,
            &camera_uniform,
        );
        self.projection_matrix = camera_uniform.projection;

        self.resolution = Some(size);

        self.post_process = Some(post_process);
    }
}

fn load_audios(audio_stream: &OutputStream, audios: Vec<AudioObject>, scene: &Scene) {
    let audio_mixer = audio_stream.mixer();
    let audio_sink = rodio::Sink::connect_new(audio_mixer);

    for audio in audios {
        for sound in audio.sounds {
            let Some(raw) = scene.misc.remove(&sound) else {
                continue;
            };

            let cursor = Cursor::new(raw);
            let sound_pathbuf = Path::new(&sound).to_path_buf();
            let hint = sound_pathbuf
                .extension()
                .and_then(|e| e.to_str());

            let mut builder = rodio::decoder::Decoder::builder().with_data(cursor);
            if let Some(ext) = hint {
                builder = builder.with_hint(ext);
            }
            // When no hint is provided, rodio auto-detects the format from
            // magic bytes — no need to hardcode a fallback.
            let Some(source) = builder.build().ok() else {
                println!("failed to build audio: {:?}", sound);
                continue;
            };

            match audio.playback_mode {
                PlaybackMode::Loop => {
                    audio_mixer.add(source.repeat_infinite());
                }
                PlaybackMode::Others => {}
            }
        }
    }

    std::thread::spawn(move || {
        audio_sink.play();
        audio_sink.set_volume(1.0);
        audio_sink.sleep_until_end();
    });
}

/// Count total vertices and indices needed for all texture objects.
fn count_geometry(objects: &[TextureObject]) -> (u32, u32) {
    let mut total_verts: u32 = 0;
    let mut total_indices: u32 = 0;
    for obj in objects {
        if let Some(ref mesh) = obj.mesh {
            total_verts += mesh.vertices.len() as u32;
            total_indices += mesh.indices.len() as u32;
        } else {
            total_verts += 4; // 4 corners
            total_indices += 6; // 2 triangles
        }
    }
    // Ensure at least a minimal capacity
    (total_verts.max(4), total_indices.max(6))
}

/// Create default rendering pipeline
fn create_pipeline(app: &WgpuApp, bindgroup_layout: &BindGroupLayout) -> RenderPipeline {
    let shader = app.device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Wgsl(include_str!("./shader/image.wgsl").into()),
    });

    let pipeline_layout = app
        .device
        .create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[
                bindgroup_layout,
                &app.projection_bindgroup.projection_layout,
            ],
            immediate_size: 0,
        });

    let pipeline = app
        .device
        .create_render_pipeline(&RenderPipelineDescriptor {
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
                    format: app.surface.config.format,
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

    pipeline
}
