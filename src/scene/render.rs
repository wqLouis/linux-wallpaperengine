use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use bytemuck::bytes_of;
use depkg::pkg_parser::tex_parser::Tex;
use pollster::block_on;
use wgpu::{wgt::DeviceDescriptor, *};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{self, ActiveEventLoop, EventLoop},
    window::Window,
};

use crate::scene::{Root, camera::CameraUniform, render_tex::create_tex_bind_group};

struct WgpuApp {
    window: Arc<Window>,
    surface: Surface<'static>,
    device: Device,
    queue: Queue,
    config: SurfaceConfiguration,
    size: PhysicalSize<u32>,
    size_changed: bool,

    render_pipeline: RenderPipeline,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    projection_buffer: Buffer,
    bind_groups: Vec<BindGroup>,
    bind_group_layout: BindGroupLayout,
    index_len: u32,
    vertex_len: u32,

    root: crate::scene::Root,
    objects: Vec<crate::scene::Object>,
    texs: HashMap<String, Tex>,
    jsons: HashMap<String, String>,
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
}

impl WgpuApp {
    async fn new(
        window: Arc<Window>,
        general: crate::scene::General,
        objects: Vec<crate::scene::Object>,
        texs: HashMap<String, Tex>,
        jsons: HashMap<String, String>,
        root: Root,
    ) -> Self {
        const MAX_RECT: u64 = 512;
        const MAX_VERTICES: u64 = MAX_RECT * 4;
        const MAX_INDICES: u64 = MAX_RECT * 6;

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
                required_features: Features::empty(),
                required_limits: Limits::defaults(),
                experimental_features: ExperimentalFeatures::disabled(),
                memory_hints: MemoryHints::Performance,
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
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: &[
                VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: VertexFormat::Float32x3,
                },
                VertexAttribute {
                    offset: std::mem::size_of::<[u32; 3]>() as u64,
                    shader_location: 1,
                    format: VertexFormat::Float32x2,
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
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
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
                    blend: Some(BlendState::REPLACE),
                    write_mask: ColorWrites::all(),
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            window,
            surface,
            device,
            queue,
            config,
            size,
            size_changed: false,
            render_pipeline,
            bind_groups: Vec::new(),
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
        }
    }

    pub fn draw_rect(&mut self, pos: [f32; 2], w: f32, h: f32, z: f32) {
        let rect = [
            Vertex {
                position: [pos[0], pos[1], z],
                uv: [0.0, 0.0],
            },
            Vertex {
                position: [pos[0] + w, pos[1], z],
                uv: [1.0, 0.0],
            },
            Vertex {
                position: [pos[0], pos[1] + h, z],
                uv: [0.0, 1.0],
            },
            Vertex {
                position: [pos[0] + w, pos[1] + h, z],
                uv: [1.0, 1.0],
            },
        ];

        let indices: [u16; 6] = [0, 2, 3, 3, 1, 0].map(|f| f + self.index_len as u16);

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
        // clear the buffer
        self.index_len = 0;
        self.vertex_len = 0;

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
            self.render_main();
            if self.index_len > 0 && self.bind_groups.len() > 0 {
                // if there is something then add to render pass
                render_pass.set_bind_group(0, &self.bind_groups[0], &[]);
                render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                render_pass.set_index_buffer(self.index_buffer.slice(..), IndexFormat::Uint16);
                render_pass.draw_indexed(0..self.index_len, 0, 0..1);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        output.present();

        Ok(())
    }

    fn render_main(&mut self) {
        // Put all the render stuff here
        self.bind_groups.clear();

        self.bind_groups.push(create_tex_bind_group(
            &self.device,
            &self.queue,
            &self.bind_group_layout,
            self.texs.get("materials/画师-雨野拓展.tex").unwrap(),
            &self.root,
            &self.projection_buffer,
            &self.window.inner_size().cast::<f32>(),
        ));
        Self::draw_rect(self, [0.0, 0.0], 1920.0, 1080.0, -1.0);
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
