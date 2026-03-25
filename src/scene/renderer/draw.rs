use std::{collections::BTreeMap, rc::Rc};

use wgpu::*;

use crate::scene::loader::object_loader::TextureObject;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
pub struct Vertex {
    pos: [f32; 3],
    uv: [f32; 2],
}

struct DrawObject {
    pub texture_object: TextureObject,
    pub index_len: u32,
    pub bindgroup: BindGroup,
    pub pipelines: Vec<String>,
}

pub struct DrawQueue {
    queue: Vec<DrawObject>,
    render_pipelines: BTreeMap<String, Rc<RenderPipeline>>,
}

impl DrawQueue {
    pub fn new(device: &Device, queue: &Queue, texture_objects: Vec<TextureObject>) -> Self {
        let texture_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
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

        let draw_objects = texture_objects
            .into_iter()
            .map(|texture_object| {
                DrawObject::new(
                    device,
                    queue,
                    texture_object,
                    &texture_layout,
                    &texture_sampler,
                )
            })
            .collect::<Vec<DrawObject>>();

        Self {
            queue: draw_objects,
            render_pipelines: BTreeMap::new(),
        }
    }
}

impl DrawObject {
    pub fn new(
        device: &Device,
        queue: &Queue,
        texture_object: TextureObject,
        texture_layout: &BindGroupLayout,
        texture_sampler: &Sampler,
    ) -> Self {
        let index_len: u32 = 6;
        let texture = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: texture_object.texture.dimension[0],
                height: texture_object.texture.dimension[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &texture_object.texture.payload,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(texture_object.texture.dimension[0] * 4),
                rows_per_image: None,
            },
            Extent3d {
                width: texture_object.texture.dimension[0],
                height: texture_object.texture.dimension[1],
                depth_or_array_layers: 1,
            },
        );

        let bindgroup = device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: texture_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(
                        &texture.create_view(&TextureViewDescriptor::default()),
                    ),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(texture_sampler),
                },
            ],
        });

        let pipelines = texture_object
            .effects
            .iter()
            .map(|effect| effect.file.clone())
            .collect::<Vec<String>>();

        Self {
            texture_object,
            index_len,
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
