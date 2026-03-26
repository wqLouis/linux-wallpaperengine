use std::{collections::BTreeMap, rc::Rc};

use bytemuck::bytes_of;
use glam::{Mat2, Vec2, Vec3};
use wgpu::*;

use crate::scene::{
    loader::object_loader::TextureObject,
    renderer::{app::WgpuApp, bindgroups::get_bindgroup, buffer::Buffers},
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

pub struct PostProcess {
    pub sampler: Sampler,
    pub layout: BindGroupLayout,
    pub blank_texture: Texture,
    pub blank_buffers: Buffers,
    pub blank_bindgroup: BindGroup,
}

impl PostProcess {
    pub fn new(device: &Device, res: [u32; 2]) -> Self {
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: None,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Nearest,
            mipmap_filter: MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
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

        let blank_desc = TextureDescriptor {
            label: None,
            size: Extent3d {
                width: res[0],
                height: res[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };

        let blank_texture = device.create_texture(&blank_desc);
        let blank_buffers = Buffers::new(device, 6, 4);

        let blank_bindgroup = device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(
                        &blank_texture.create_view(&Default::default()),
                    ),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&sampler),
                },
            ],
        });

        Self {
            sampler,
            layout,
            blank_texture,
            blank_buffers,
            blank_bindgroup,
        }
    }
}

impl DrawQueue {
    pub fn new(
        device: &Device,
        queue: &Queue,
        buffers: &mut Buffers,
        texture_objects: Vec<TextureObject>,
        image_pipeline: RenderPipeline,
        post_process: &PostProcess,
    ) -> Self {
        let render_pipelines = BTreeMap::<String, Rc<RenderPipeline>>::new();

        let draw_objects = texture_objects
            .into_iter()
            .map(|texture_object| {
                DrawObject::new(
                    device,
                    queue,
                    texture_object,
                    post_process,
                    &render_pipelines,
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
        texture_object: TextureObject,
        post_process: &PostProcess,
        pipelines: &BTreeMap<String, Rc<RenderPipeline>>,
        buffers: &mut Buffers,
    ) -> Self {
        let index_start = buffers.index_len.clone();

        let pipelines = texture_object
            .effects
            .iter()
            .filter_map(|effect| pipelines.get(&effect.file))
            .collect::<Vec<&Rc<RenderPipeline>>>()
            .into_iter()
            .map(|rc| Rc::clone(rc))
            .collect();

        let bindgroup = get_bindgroup(
            device,
            queue,
            &texture_object,
            &post_process.sampler,
            &post_process.layout,
        );

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

impl WgpuApp {
    /// This function process textures with multiple render pipelines
    /// WIP
    pub(super) fn pipelines_process_texture(
        &mut self,
        pipelines: &Vec<&Rc<RenderPipeline>>,
        draw_object: &DrawObject,
    ) {
        let resolution = self.resolution.unwrap();
        let mut post_process = self.post_process.as_mut().unwrap();

        let mut source: &Texture = &post_process.blank_texture;
        let source_view = source.create_view(&Default::default());

        let render_pass_desc = RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &post_process.blank_texture.create_view(&Default::default()),
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

        draw_rect(
            &mut post_process.blank_buffers,
            &self.queue,
            [
                Vec3::new(0.0, 0.0, -1.0),
                Vec3::new(resolution[0] as f32, 0.0, -1.0),
                Vec3::new(resolution[0] as f32, resolution[1] as f32, -1.0),
                Vec3::new(0.0, resolution[1] as f32, -1.0),
            ],
        );

        let mut is_first_draw: bool = true;

        for pipeline in pipelines {
            let mut encoder = self
                .device
                .create_command_encoder(&CommandEncoderDescriptor::default());

            {
                {
                    let mut render_pass = encoder.begin_render_pass(&render_pass_desc);

                    if is_first_draw {
                        render_pass.set_vertex_buffer(0, self.buffers.vertex.slice(..));
                        render_pass
                            .set_index_buffer(self.buffers.index.slice(..), IndexFormat::Uint32);
                    } else {
                        render_pass
                            .set_vertex_buffer(0, post_process.blank_buffers.vertex.slice(..));
                        render_pass.set_index_buffer(
                            post_process.blank_buffers.index.slice(..),
                            IndexFormat::Uint32,
                        );
                    }

                    render_pass.set_pipeline(pipeline);
                    render_pass.set_bind_group(1, &self.projection_bindgroup.projection, &[]);

                    if is_first_draw {
                        render_pass.set_bind_group(0, &draw_object.bindgroup, &[]); // The intermediate texture
                    } else {
                        render_pass.set_bind_group(0, &post_process.blank_bindgroup, &[]);
                    }

                    if is_first_draw {
                        render_pass.draw_indexed(
                            draw_object.index_range[0]..draw_object.index_range[1],
                            0,
                            0..1,
                        );
                    } else {
                        render_pass.draw_indexed(0..6, 0, 0..1);
                    }

                    is_first_draw = false;
                }

                self.queue.submit(Some(encoder.finish()));

                source = &post_process.blank_texture;
            }
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
        std::mem::size_of::<Vertex>() as u64 * buffers.vertex_len as u64,
        bytes_of(&rect),
    );

    queue.write_buffer(
        &buffers.index,
        std::mem::size_of::<[u32; 6]>() as u64 * buffers.index_len as u64,
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
