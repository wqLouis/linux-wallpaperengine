use crate::{
    MAX_TEXTURE,
    scene::{
        loader::scene_loader::Scene,
        renderer::{bindgroup::BindGroups, buffer::Buffers, draw::Vertex},
    },
};

use super::*;
use wgpu::*;

pub struct WgpuApp {
    surface: AppSurface,

    buffers: buffer::Buffers,

    bindgroups: bindgroup::BindGroups,

    scene: Scene,

    device: Device,
    queue: Queue,
    pipeline: RenderPipeline,

    custom_pipelines: Vec<RenderPipeline>,

    audio_stream: rodio::OutputStream,
}

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

        let scene = Scene::new(scene_path);

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
            scene,
            device,
            queue,
            pipeline,
            custom_pipelines: Vec::new(),
            audio_stream,
        }
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
