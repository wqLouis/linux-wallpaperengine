use std::{io::Cursor, path::Path};

use crate::scene::{
    loader::{
        object_loader::{AudioObject, ObjectMap, PlaybackMode},
        scene_loader::Scene,
    },
    renderer::{
        app::WgpuApp,
        draw::{DrawQueue, Vertex},
        post_process::PostProcess,
        projection::Projection,
    },
};

use wgpu::*;

use rodio::{OutputStream, Source};

impl WgpuApp {
    /// load assets
    pub fn load(&mut self) {
        let mut scene = Scene::new(self.scene_path.clone());
        let size = [
            scene.root.general.orthogonalprojection.width as u32,
            scene.root.general.orthogonalprojection.height as u32,
        ];

        let post_process = PostProcess::new(&self.device, size);

        self.clear_color = scene.root.general.clearcolor.parse().unwrap_or_default();

        let pipeline = create_pipeline(&self, &post_process.layout);
        let objects = ObjectMap::new(&scene.root.objects.clone(), &scene);
        let draw_queue = DrawQueue::new(
            &self.device,
            &self.queue,
            &mut self.buffers,
            &scene,
            objects.texture,
            pipeline,
            &post_process,
        );

        load_audios(&self.audio_stream, objects.audio, &mut scene);

        self.draw_queue = Some(draw_queue);

        self.projection_bindgroup.create_projection_bindgroup(
            &self.buffers,
            &self.device,
            &self.queue,
            &Projection::new(&scene.root).create_camera_uniform(),
        );

        self.resolution = Some(size);

        self.post_process = Some(post_process);
    }
}

fn load_audios(audio_stream: &OutputStream, audios: Vec<AudioObject>, scene: &mut Scene) {
    let audio_mixer = audio_stream.mixer();
    let audio_sink = rodio::Sink::connect_new(audio_mixer);

    for audio in audios {
        for sound in audio.sounds {
            let Some(raw) = scene.misc.remove(&sound) else {
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
