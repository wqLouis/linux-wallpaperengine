use std::{collections::BTreeMap, rc::Rc};

use wgpu::*;

use crate::scene::{loader::object_loader::TextureObject, renderer::bindgroups::TextureBindGroups};

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

        let draw_objects = texture_objects
            .into_iter()
            .map(|texture_object| {
                DrawObject::new(
                    device,
                    queue,
                    texture_object,
                    &texture_sampler,
                    &texture_bindgroups,
                )
            })
            .collect::<Vec<DrawObject>>();

        let mut render_pipelines = BTreeMap::<String, Rc<RenderPipeline>>::new();

        render_pipelines.insert("FALLBACK".to_string(), Rc::new(fallback_pipeline));

        Self {
            queue: draw_objects,
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
    ) -> Self {
        let index_len: u32 = 6;

        let pipelines = texture_object
            .effects
            .iter()
            .map(|effect| effect.file.clone())
            .collect::<Vec<String>>();

        let bindgroup =
            texture_bindgroup.get_bindgroup(device, queue, &texture_object, texture_sampler);

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
