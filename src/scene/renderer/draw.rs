use std::{collections::BTreeMap, path::Path, rc::Rc};

use crate::scene::loader::{model::Model, object_loader::TextureObject};

use super::buffer::Buffers;
use bytemuck::bytes_of;
use depkg::pkg_parser::tex_parser::Tex;
use glam::{Mat2, Vec2, Vec3};
use wgpu::*;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
pub struct Vertex {
    pos: [f32; 3],
    uv: [f32; 2],
    texture_index: u32,
}

pub struct DrawTextureObject {
    pub texture: Rc<Tex>,
    origin: Vec3,
    angles: Vec3,
    scale: Vec3,
    size: Vec2,
}

pub struct DrawQueue {
    pub queue: Vec<DrawTextureObject>,
}

impl DrawQueue {
    pub fn new() -> Self {
        Self { queue: Vec::new() }
    }

    pub fn push(
        &mut self,
        texture_object: TextureObject,
        jsons: &BTreeMap<String, String>,
        texs: &BTreeMap<String, Rc<Tex>>,
    ) -> Option<()> {
        let draw_obj = DrawTextureObject::from_texture_object(texture_object, jsons, texs)?;

        if draw_obj.texture.dimension[0] * draw_obj.texture.dimension[1] * 4
            != draw_obj.texture.payload.len() as u32
        {
            return Some(());
        }

        self.queue.push(draw_obj);
        Some(())
    }

    pub fn submit_draw_queue(self, buffers: &mut Buffers, queue: &Queue) {
        for (index, draw_obj) in self.queue.into_iter().enumerate() {
            draw_obj.draw(buffers, queue, index as u32);
        }
    }
}

impl DrawTextureObject {
    fn from_texture_object(
        texture_object: TextureObject,
        jsons: &BTreeMap<String, String>,
        texs: &BTreeMap<String, Rc<Tex>>,
    ) -> Option<Self> {
        let model = serde_json::from_str::<Model>(jsons.get(&texture_object.model)?).ok()?;

        let mut material = Path::new(&model.material).to_path_buf();
        material.set_extension("tex");

        Some(Self {
            texture: Rc::clone(texs.get(material.to_str().unwrap())?),
            origin: texture_object.origin,
            angles: texture_object.angles,
            scale: texture_object.scale,
            size: texture_object.size,
        })
    }

    fn draw(mut self, buffers: &mut Buffers, queue: &Queue, texture_index: u32) {
        // consume itself and write the data into buffers

        let scale = Vec2 {
            x: self.scale.x,
            y: self.scale.y,
        };

        self.size *= scale;
        self.origin.z -= 1.0;

        let rotation_mat = Mat2::from_angle(self.angles.z.to_radians());
        let rotated = vec![
            Vec2::new(-self.size.x / 2.0, self.size.y / 2.0),
            Vec2::new(self.size.x / 2.0, self.size.y / 2.0),
            Vec2::new(self.size.x / 2.0, -self.size.y / 2.0),
            Vec2::new(-self.size.x / 2.0, -self.size.y / 2.0),
        ]
        .iter()
        .map(|vertex| (rotation_mat * vertex) + Vec2::new(self.origin.x, self.origin.y))
        .collect::<Vec<Vec2>>();

        let rect = [
            Vertex {
                pos: [rotated[0].x, rotated[0].y, self.origin.z],
                uv: [0.0, 0.0],
                texture_index,
            },
            Vertex {
                pos: [rotated[1].x, rotated[1].y, self.origin.z],
                uv: [1.0, 0.0],
                texture_index,
            },
            Vertex {
                pos: [rotated[2].x, rotated[2].y, self.origin.z],
                uv: [1.0, 1.0],
                texture_index,
            },
            Vertex {
                pos: [rotated[3].x, rotated[3].y, self.origin.z],
                uv: [0.0, 1.0],
                texture_index,
            },
        ];

        let indices: [u16; 6] = [0, 2, 1, 0, 3, 2].map(|f| f + buffers.vertex_len as u16);

        queue.write_buffer(
            &buffers.vertex,
            std::mem::size_of::<Vertex>() as u64 * buffers.vertex_len as u64,
            bytes_of(&rect),
        );

        queue.write_buffer(
            &buffers.index,
            std::mem::size_of::<[u16; 6]>() as u64 * buffers.index_len as u64,
            bytes_of(&indices),
        );

        buffers.index_len += indices.len() as u32;
        buffers.vertex_len += rect.len() as u32;
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
                VertexAttribute {
                    offset: std::mem::size_of::<[f32; 5]>() as BufferAddress,
                    shader_location: 2,
                    format: VertexFormat::Uint32,
                },
            ],
        }
    }
}
