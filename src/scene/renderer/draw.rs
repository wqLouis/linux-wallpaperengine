use std::{collections::BTreeMap, rc::Rc};

use bytemuck::bytes_of;
use glam::{Mat2, Vec2, Vec3};
use wgpu::*;

use crate::scene::{
    loader::{object_loader::TextureObject, scene_loader::Scene},
    renderer::{
        app::WgpuApp, buffer::Buffers, post_process::PostProcess,
        post_processor::pipeline_handler::get_or_create_pipeline,
    },
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
    pub pipelines: Vec<Rc<RenderPipeline>>,
}

/// Contains the queue and the rendering pipelines
/// queue: The draw queue
/// render_pipelines: The pipelines for spiecial effects and with the effects file name as key
pub struct DrawQueue {
    pub queue: Rc<Vec<DrawObject>>,
    pub render_pipelines: BTreeMap<String, Rc<RenderPipeline>>,
    pub image_pipeline: RenderPipeline,
}

impl DrawQueue {
    pub fn new(
        device: &Device,
        queue: &Queue,
        buffers: &mut Buffers,
        scene: &Scene,
        texture_objects: Vec<TextureObject>,
        image_pipeline: RenderPipeline,
        post_process: &PostProcess,
    ) -> Self {
        let mut render_pipelines = BTreeMap::<String, Rc<RenderPipeline>>::new();

        let draw_objects = texture_objects
            .into_iter()
            .map(|texture_object| {
                DrawObject::new(
                    device,
                    queue,
                    scene,
                    texture_object,
                    post_process,
                    &mut render_pipelines,
                    buffers,
                )
            })
            .collect::<Vec<DrawObject>>();

        Self {
            queue: Rc::new(draw_objects),
            render_pipelines,
            image_pipeline,
        }
    }
}

impl DrawObject {
    pub fn new(
        device: &Device,
        queue: &Queue,
        scene: &Scene,
        texture_object: TextureObject,
        post_process: &PostProcess,
        pipelines: &mut BTreeMap<String, Rc<RenderPipeline>>,
        buffers: &mut Buffers,
    ) -> Self {
        let index_start = buffers.index_len.clone();

        let pipelines = texture_object
            .effects
            .iter()
            .filter_map(|effect| get_or_create_pipeline(effect.file.clone(), pipelines, scene))
            .collect::<Vec<Rc<RenderPipeline>>>();

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
            layout: &post_process.layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(
                        &texture.create_view(&Default::default()),
                    ),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&post_process.sampler),
                },
            ],
        });

        draw_texture(&texture_object, buffers, queue);

        Self {
            texture_object,
            index_range: [index_start, buffers.index_len],
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

fn draw_rect(buffers: &mut Buffers, queue: &Queue, pos: [Vec3; 4]) {
    let rect = [
        Vertex {
            pos: pos[0].to_array(),
            uv: [0.0, 0.0],
        },
        Vertex {
            pos: pos[1].to_array(),
            uv: [1.0, 0.0],
        },
        Vertex {
            pos: pos[2].to_array(),
            uv: [1.0, 1.0],
        },
        Vertex {
            pos: pos[3].to_array(),
            uv: [0.0, 1.0],
        },
    ];

    let indices: [u32; 6] = [0, 2, 1, 0, 3, 2].map(|f| f + buffers.vertex_len);

    queue.write_buffer(
        &buffers.vertex,
        std::mem::size_of::<Vertex>() as BufferAddress * buffers.vertex_len as BufferAddress,
        bytes_of(&rect),
    );

    queue.write_buffer(
        &buffers.index,
        std::mem::size_of::<u32>() as BufferAddress * buffers.index_len as BufferAddress,
        bytes_of(&indices),
    );

    buffers.index_len += indices.len() as u32;
    buffers.vertex_len += rect.len() as u32;
}

fn draw_texture(texture_object: &TextureObject, buffers: &mut Buffers, queue: &Queue) {
    let scale = Vec2 {
        x: texture_object.scale.x,
        y: texture_object.scale.y,
    };

    let size = texture_object.size * scale;
    let z = texture_object.origin.z - 1.0;

    let rotation_mat = Mat2::from_angle(texture_object.angles.z.to_radians());
    let rotated = vec![
        Vec2::new(-size.x / 2.0, size.y / 2.0),
        Vec2::new(size.x / 2.0, size.y / 2.0),
        Vec2::new(size.x / 2.0, -size.y / 2.0),
        Vec2::new(-size.x / 2.0, -size.y / 2.0),
    ]
    .iter()
    .map(|vertex| {
        (rotation_mat * vertex) + Vec2::new(texture_object.origin.x, texture_object.origin.y)
    })
    .collect::<Vec<Vec2>>();

    let rect = [
        Vec3::new(rotated[0].x, rotated[0].y, z),
        Vec3::new(rotated[1].x, rotated[1].y, z),
        Vec3::new(rotated[2].x, rotated[2].y, z),
        Vec3::new(rotated[3].x, rotated[3].y, z),
    ];

    draw_rect(buffers, queue, rect);
}
