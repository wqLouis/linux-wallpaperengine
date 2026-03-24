use std::rc::Rc;

use wgpu::*;

use crate::scene::loader::object_loader::TextureObject;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
pub struct Vertex {
    pos: [f32; 3],
    uv: [f32; 2],
}

pub struct DrawObject {
    pub texture_object: TextureObject,
    pub index_len: u32,
    pub bindgroup: BindGroup,
    pub pipeline: Rc<RenderPipeline>,
}

impl DrawObject {
    pub fn new(
        device: &Device,
        texture_object: TextureObject,
        render_pipeline: Rc<RenderPipeline>,
    ) {
        let index_len: u32 = 6;
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
