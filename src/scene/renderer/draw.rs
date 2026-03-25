use std::{collections::BTreeMap, rc::Rc};

use wgpu::*;

use crate::scene::{
    loader::object_loader::TextureObject,
    renderer::{app::WgpuApp, bindgroups::TextureBindGroups},
};

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
pub struct Vertex {
    pos: [f32; 3],
    uv: [f32; 2],
}

#[derive(Debug, Clone)]
pub struct DrawObject {
    pub texture_object: TextureObject,
    pub index_range: [u32; 2],
    pub bindgroup: BindGroup,
    pub pipelines: Vec<String>,
}

/// Contains the queue and the rendering pipelines
/// queue: The draw queue
/// render_pipelines: The pipelines for spiecial effects and with the effects file name as key
pub struct DrawQueue {
    pub queue: Rc<Vec<DrawObject>>,
    pub render_pipelines: BTreeMap<String, Rc<RenderPipeline>>,
}

impl DrawQueue {
    pub fn new(
        device: &Device,
        queue: &Queue,
        texture_objects: Vec<TextureObject>,
        fallback_pipeline: RenderPipeline,
    ) -> Self {
        let texture_sampler = device.create_sampler(&SamplerDescriptor {
            label: None,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Nearest,
            mipmap_filter: MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let texture_bindgroups = TextureBindGroups::new(device);

        let mut index_ptr: u32 = 0;
        let draw_objects = texture_objects
            .into_iter()
            .map(|texture_object| {
                DrawObject::new(
                    device,
                    queue,
                    texture_object,
                    &texture_sampler,
                    &texture_bindgroups,
                    &mut index_ptr,
                )
            })
            .collect::<Vec<DrawObject>>();

        let mut render_pipelines = BTreeMap::<String, Rc<RenderPipeline>>::new();

        render_pipelines.insert("FALLBACK".to_string(), Rc::new(fallback_pipeline)); // FALLBACK is the render pipeline for materials that has no custom effects which renders as a simple image

        Self {
            queue: Rc::new(draw_objects),
            render_pipelines,
        }
    }
}

impl DrawObject {
    pub fn new(
        device: &Device,
        queue: &Queue,
        texture_object: TextureObject,
        texture_sampler: &Sampler,
        texture_bindgroup: &TextureBindGroups,
        index_ptr: &mut u32,
    ) -> Self {
        let index_len: u32 = 6;
        let index_start = index_ptr.clone();

        let mut pipelines = texture_object
            .effects
            .iter()
            .map(|effect| effect.file.clone())
            .collect::<Vec<String>>();

        if pipelines.len() == 0 {
            pipelines.push("FALLBACK".to_string());
        }

        let bindgroup =
            texture_bindgroup.get_bindgroup(device, queue, &texture_object, texture_sampler);

        *index_ptr += index_len;

        Self {
            texture_object,
            index_range: [index_start, index_len],
            bindgroup,
            pipelines,
        }
    }
}

impl Vertex {
    pub fn create_buffer_layout<'a>() -> VertexBufferLayout<'a> {
        VertexBufferLayout {
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
            ],
        }
    }
}

impl WgpuApp {
    /// This function process textures with multiple render pipelines
    /// WIP
    pub(super) fn pipelines_process_texture(
        &self,
        pipelines: &Vec<&Rc<RenderPipeline>>,
        draw_object: &DrawObject,
    ) {
        let resolution = self.resolution.unwrap();

        let canvas_desc = TextureDescriptor {
            label: None,
            size: Extent3d {
                width: resolution[0],
                height: resolution[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };

        let canvas = self.device.create_texture(&canvas_desc);
        let mut canvas_view = canvas.create_view(&Default::default());

        let layout = self
            .device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
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
                ],
            });

        let sampler = self.device.create_sampler(&SamplerDescriptor {
            label: None,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Nearest,
            mipmap_filter: MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let render_pass_desc = RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &canvas_view,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    }),
                    store: StoreOp::Store,
                },
                depth_slice: None,
            })],
            ..Default::default()
        };

        let mut bindgroup = &draw_object.bindgroup;

        for pipeline in pipelines {
            let mut encoder = self
                .device
                .create_command_encoder(&CommandEncoderDescriptor::default());

            {
                let mut render_pass = encoder.begin_render_pass(&render_pass_desc);
                render_pass.set_pipeline(pipeline);
                render_pass.set_bind_group(0, bindgroup, &[]); // The intermediate texture
                render_pass.set_bind_group(1, &self.projection_bindgroup.projection, &[]);

                render_pass.draw_indexed(
                    draw_object.index_range[0]..draw_object.index_range[1],
                    0,
                    0..1,
                );
            }

            self.queue.submit(Some(encoder.finish()));
        }
    }
}
