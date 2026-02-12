use std::{
    collections::HashMap,
    num::NonZeroU32,
    path::Path,
    sync::{Arc, Mutex},
};

use bytemuck::bytes_of;
use depkg::pkg_parser::tex_parser::Tex;
use glam::{Mat2, Vec2};
use pollster::block_on;
use serde_json::Value;
use wgpu::{
    wgt::{BufferDescriptor, DeviceDescriptor},
    *,
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{self, ActiveEventLoop, EventLoop},
    window::Window,
};

use crate::scene::{
    Root, Vectors, camera::CameraUniform, renderer::bindgroup::create_tex_bind_group,
};

struct WgpuApp {
    window: Arc<Window>,
    surface: Surface<'static>,
    device: Device,
    queue: Queue,
    config: SurfaceConfiguration,
    size: PhysicalSize<u32>,

    render_pipeline: RenderPipeline,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    projection_buffer: Buffer,
    bind_group_layout: BindGroupLayout,
    projection_bind_group_layout: BindGroupLayout,
    bind_group: Option<BindGroup>,
    projection_bind_group: Option<BindGroup>,
    index_len: u32,
    vertex_len: u32,

    root: crate::scene::Root,
    objects: Vec<crate::scene::Object>,
    texs: HashMap<String, Tex>,
    jsons: HashMap<String, String>,
    render_tex: Vec<Tex>,
}

#[derive(Default)]
struct WgpuAppHandler {
    app: Arc<Mutex<Option<WgpuApp>>>,

    root: crate::scene::Root,
    jsons: HashMap<String, String>,
    texs: HashMap<String, Tex>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    uv: [f32; 2],
    tex_index: u32,
    alpha: f32,
}

const MAX_RECT: u64 = 512;
const MAX_VERTICES: u64 = MAX_RECT * 4;
const MAX_INDICES: u64 = MAX_RECT * 6;

impl WgpuApp {
    async fn new(
        window: Arc<Window>,
        general: crate::scene::General,
        objects: Vec<crate::scene::Object>,
        texs: HashMap<String, Tex>,
        jsons: HashMap<String, String>,
        root: Root,
    ) -> Self {
        let texs_len = texs.len();

        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::VULKAN,
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: None,
                required_features: Features::TEXTURE_BINDING_ARRAY
                    | Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
                required_limits: Limits {
                    max_binding_array_elements_per_shader_stage: MAX_RECT as u32,
                    ..Default::default()
                },
                experimental_features: ExperimentalFeatures::disabled(),
                memory_hints: MemoryHints::MemoryUsage,
                trace: Trace::Off,
            })
            .await
            .unwrap();

        let caps = surface.get_capabilities(&adapter);
        let size = window.inner_size();

        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: caps.formats[0],
            width: size.width,
            height: size.height,
            present_mode: PresentMode::Immediate,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: None,
            source: ShaderSource::Wgsl(include_str!("./shader/image.wgsl").into()),
        });

        surface.configure(&device, &config);

        let (vertex_buffer, index_buffer, projection_buffer) = Self::create_buffer(&device);

        queue.write_buffer(
            &projection_buffer,
            0,
            bytes_of(
                &root
                    .camera
                    .new(&general)
                    .create_projection_matrix(&window.inner_size().cast::<f32>()),
            ),
        );

        let vertex_buffer_layout = VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as BufferAddress,
            step_mode: VertexStepMode::Vertex,
            attributes: &[
                VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: VertexFormat::Float32x3,
                },
                VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as BufferAddress,
                    shader_location: 1,
                    format: VertexFormat::Float32x2,
                },
                VertexAttribute {
                    offset: std::mem::size_of::<[f32; 5]>() as BufferAddress,
                    shader_location: 2,
                    format: VertexFormat::Uint32,
                },
                VertexAttribute {
                    offset: std::mem::size_of::<[f32; 6]>() as BufferAddress,
                    shader_location: 3,
                    format: VertexFormat::Float32,
                },
            ],
        };

        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: Some(NonZeroU32::new(MAX_RECT as u32).unwrap()),
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let projection_bind_group_layout =
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: None,
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Pipeline layout"),
            bind_group_layouts: &[&bind_group_layout, &projection_bind_group_layout],
            immediate_size: 0,
        });

        let render_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Render pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[vertex_buffer_layout],
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
                    format: config.format,
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

        let mut wgpu_app = Self {
            window,
            surface,
            device,
            queue,
            config,
            size,
            render_pipeline,
            index_len: 0,
            vertex_len: 0,
            vertex_buffer,
            index_buffer,
            objects,
            jsons,
            root,
            texs,
            bind_group_layout,
            projection_buffer,
            projection_bind_group_layout,
            render_tex: Vec::with_capacity(texs_len),
            bind_group: None,
            projection_bind_group: None,
        };
        wgpu_app.load();
        wgpu_app
    }

    fn create_buffer(device: &Device) -> (Buffer, Buffer, Buffer) {
        let vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: None,
            size: MAX_VERTICES * std::mem::size_of::<Vertex>() as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&BufferDescriptor {
            label: None,
            size: MAX_INDICES * std::mem::size_of::<u16>() as u64,
            usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let projection_buffer = device.create_buffer(&BufferDescriptor {
            label: None,
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        (vertex_buffer, index_buffer, projection_buffer)
    }

    fn draw_rect(
        &mut self,
        pos: [f32; 2],
        w: f32,
        h: f32,
        z: f32,
        tex_index: u32,
        rad: f32,
        alpha: f32,
    ) {
        let rotation_mat = Mat2::from_angle(rad);
        let rotated = vec![
            Vec2::new(-w / 2.0, h / 2.0),
            Vec2::new(w / 2.0, h / 2.0),
            Vec2::new(w / 2.0, -h / 2.0),
            Vec2::new(-w / 2.0, -h / 2.0),
        ]
        .iter()
        .map(|vertex| (rotation_mat * vertex) + Vec2::new(pos[0] + w / 2.0, pos[1] + h / 2.0))
        .collect::<Vec<Vec2>>();
        let rect = [
            Vertex {
                position: [rotated[0].x, rotated[0].y, z],
                uv: [0.0, 0.0],
                tex_index,
                alpha,
            },
            Vertex {
                position: [rotated[1].x, rotated[1].y, z],
                uv: [1.0, 0.0],
                tex_index,
                alpha,
            },
            Vertex {
                position: [rotated[2].x, rotated[2].y, z],
                uv: [1.0, 1.0],
                tex_index,
                alpha,
            },
            Vertex {
                position: [rotated[3].x, rotated[3].y, z],
                uv: [0.0, 1.0],
                tex_index,
                alpha,
            },
        ];

        let indices: [u16; 6] = [0, 2, 1, 0, 3, 2].map(|f| f + self.vertex_len as u16);

        self.queue.write_buffer(
            &self.vertex_buffer,
            std::mem::size_of::<Vertex>() as u64 * self.vertex_len as u64,
            bytes_of(&rect),
        );

        self.queue.write_buffer(
            &self.index_buffer,
            std::mem::size_of::<[u16; 6]>() as u64 * self.index_len as u64,
            bytes_of(&indices),
        );

        self.vertex_len += rect.len() as u32;
        self.index_len += indices.len() as u32;
    }

    fn render(&mut self) -> Result<(), SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            // Render pass part
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Render pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
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

            render_pass.set_pipeline(&self.render_pipeline);
            if self.index_len < 1 {
                return Ok(());
            }

            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), IndexFormat::Uint16);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.set_bind_group(1, &self.projection_bind_group, &[]);
            render_pass.draw_indexed(0..(MAX_INDICES as u32), 0, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        output.present();

        Ok(())
    }

    fn load(&mut self) {
        // Put all the render stuff here
        struct Draw {
            origin: [f32; 3],
            scale: [f32; 3],
            angles: [f32; 3],
            size: [f32; 2],
            tex_index: u32,
            tex: Tex,
            alpha: f32,
        }

        let mut draw_queue: Vec<Draw> = Vec::with_capacity(self.objects.len());
        let mut tex_index = 0;

        for object in &self.objects {
            if object.image.is_none() {
                continue;
            }

            let visible = match &object.visible {
                Some(val) => val.value,
                None => true,
            };

            if !visible {
                continue;
            }

            let image = Path::new(object.image.as_ref().unwrap_or(&"".to_string())).to_path_buf();
            let origin = match &object.origin {
                Some(val) => val.parse().unwrap(),
                None => continue,
            };

            let scale = &object
                .scale
                .as_ref()
                .unwrap_or(&Vectors::Vectors("1.0 1.0 1.0".to_string()))
                .parse()
                .unwrap_or_default();
            let size = &object
                .size
                .as_ref()
                .unwrap_or(&Vectors::default())
                .parse()
                .unwrap_or_default();
            let angles = &object
                .angles
                .as_ref()
                .unwrap_or(&Vectors::Vectors("0.0 0.0 0.0".to_string()))
                .parse()
                .unwrap_or_default()
                .to_vec();
            let alpha = object.alpha.unwrap_or(1.0) as f32;

            let model_path = image.clone();
            let Some(model) = self.jsons.get(model_path.to_str().unwrap_or_default()) else {
                continue;
            };
            let model = serde_json::from_str::<crate::scene::models::Root>(model);
            let model = match model {
                Ok(val) => val,
                Err(_) => continue,
            };

            let mut tex_path = Path::new(&model.material).to_path_buf();
            tex_path.set_extension("tex");

            let Some(tex) = self.texs.get(tex_path.to_str().unwrap_or_default()) else {
                continue;
            };
            let tex = tex.to_owned();

            if tex.payload.len() != (tex.dimension[0] * tex.dimension[1] * 4) as usize {
                println!("Broken texture: {:?}", tex_path);
                println!(
                    "format: {:?}    dimensions: {:?}",
                    tex.extension, tex.dimension
                );
                println!(
                    "size: {:?}    actual_size: {:?}",
                    (tex.dimension[0] * tex.dimension[0] * 4),
                    tex.payload.len()
                );
                println!();
                continue;
            }

            println!("Loaded texture: {:?}", tex_path);
            println!(
                "format: {:?}    dimensions: {:?}",
                tex.extension, tex.dimension
            );
            println!("origin: {:?}    angles: {:?}", origin, angles);
            println!();

            draw_queue.push(Draw {
                origin: [origin[0] as f32, origin[1] as f32, origin[2] as f32 - 1.0],
                scale: [scale[0] as f32, scale[1] as f32, scale[2] as f32],
                size: [size[0] as f32, size[1] as f32],
                angles: [angles[0] as f32, angles[1] as f32, angles[2] as f32],
                tex_index,
                tex,
                alpha,
            });

            tex_index += 1;
        }

        let (bind_group, projection_bind_group) = create_tex_bind_group(
            &self.device,
            &self.queue,
            &self.bind_group_layout,
            &self.projection_bind_group_layout,
            &draw_queue
                .iter()
                .map(|draw| draw.tex.clone())
                .collect::<Vec<Tex>>(),
            &self.root,
            &self.projection_buffer,
            &self.window.inner_size().cast::<f32>(),
        );

        self.bind_group = Some(bind_group);
        self.projection_bind_group = Some(projection_bind_group);

        for draw in draw_queue {
            let scaled_size = [draw.size[0] * draw.scale[0], draw.size[1] * draw.scale[1]];
            self.draw_rect(
                [
                    draw.origin[0] - (scaled_size[0] / 2.0),
                    draw.origin[1] - (scaled_size[1] / 2.0),
                ],
                scaled_size[0],
                scaled_size[1],
                draw.origin[2],
                draw.tex_index,
                draw.angles[2],
                draw.alpha,
            );
            self.render_tex.push(draw.tex);
        }
    }
}

impl ApplicationHandler for WgpuAppHandler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.app.as_ref().lock().unwrap().is_some() {
            return;
        }

        let window_attributes = Window::default_attributes().with_title("Linux wallpaper engine");
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        let wgpu_app = block_on(WgpuApp::new(
            window,
            self.root.general.to_owned(),
            self.root.objects.to_owned(),
            self.texs.to_owned(),
            self.jsons.to_owned(),
            self.root.to_owned(),
        ));
        self.app.lock().unwrap().replace(wgpu_app);
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let mut app = self.app.lock().unwrap();

        match event {
            WindowEvent::RedrawRequested => {
                let app = app.as_mut().unwrap();
                app.window.pre_present_notify();

                match app.render() {
                    Ok(_) => {}

                    Err(e) => eprintln!("{:?}", e),
                }
            }

            WindowEvent::Resized(physical_size) => {
                let app = app.as_mut().unwrap();
                app.size = physical_size;

                app.config.width = physical_size.width;
                app.config.height = physical_size.height;

                app.surface.configure(&app.device, &app.config);

                app.window.request_redraw();
            }

            _ => {}
        }
    }
}

pub fn start(
    scene: crate::scene::Root,
    jsons: HashMap<String, String>,
    texs: HashMap<String, Tex>,
) {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(event_loop::ControlFlow::Wait);

    let mut app = WgpuAppHandler {
        root: scene,
        jsons: jsons,
        texs: texs,
        ..Default::default()
    };

    event_loop.run_app(&mut app).unwrap();
}
