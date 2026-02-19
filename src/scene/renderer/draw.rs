use std::{collections::BTreeMap, path::Path, sync::Arc};

use super::buffer::Buffers;
use depkg::pkg_parser::tex_parser::Tex;
use glam::{Vec2, Vec3};
use serde_json::{Map, Value, from_value};
use wgpu::*;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
pub struct Vertex {
    pos: [f32; 3],
    uv: [f32; 2],
    texture_index: u32,
}

pub struct DrawTextureObject {
    texture: Arc<Tex>,
    origin: Vec3,
    angles: Vec3,
    scale: Vec3,
    size: Vec2,
    alpha: f32,
}

impl DrawTextureObject {
    pub fn new(
        object: &crate::scene::loader::scene::Object,
        jsons: Arc<BTreeMap<String, String>>,
        textures: Arc<BTreeMap<String, Arc<Tex>>>,
    ) -> Option<Self> {
        let visible =
            from_value::<bool>((&object.visible.clone().unwrap_or(Value::Bool(true))).to_owned());
        let visible_object = from_value::<bool>(
            (&object
                .visible
                .clone()
                .unwrap_or(Value::Bool(true))
                .as_object()
                .unwrap_or(&Map::default())
                .get("value")
                .unwrap_or(&Value::Bool(true))
                .to_owned())
                .to_owned(),
        );

        if !visible.unwrap_or(true) | !visible_object.unwrap_or(true) {
            return None;
        }

        let origin = &object.origin.clone()?.parse()?;
        let angles = &object.angles.clone().unwrap_or_default().parse()?;
        let scale = &object.scale.clone().unwrap_or_default().parse()?;
        let size = &object.size.clone()?.parse()?;
        let alpha = 1.0;
        let image = &object.image.clone()?;
        let mut model = Path::new(jsons.get(image)?).to_path_buf();
        model.set_extension("tex");
        let texture = Arc::clone(textures.get(model.to_str()?)?);

        Some(Self {
            texture,
            origin: Vec3 {
                x: origin[0],
                y: origin[1],
                z: origin[2],
            },
            angles: Vec3 {
                x: angles[0],
                y: angles[1],
                z: angles[2],
            },
            scale: Vec3 {
                x: scale[0],
                y: scale[1],
                z: scale[2],
            },
            size: Vec2 {
                x: size[0],
                y: size[1],
            },
            alpha,
        })
    }

    fn draw(self, buffers: &mut Buffers, queue: &Queue) {
        // consume itself and write the data into buffers
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
                VertexAttribute {
                    offset: std::mem::size_of::<[f32; 6]>() as BufferAddress,
                    shader_location: 3,
                    format: VertexFormat::Float32,
                },
            ],
        }
    }
}
