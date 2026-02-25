use std::num::NonZero;

use bytemuck::bytes_of;
use wgpu::*;

use crate::{
    MAX_TEXTURE,
    scene::renderer::{buffer::Buffers, draw::DrawQueue, projection::CameraUniform},
};

pub struct BindGroups {
    pub texture_layout: BindGroupLayout,
    pub texture: Option<BindGroup>,

    pub projection: ProjectionBindGroups,
}

pub struct ProjectionBindGroups {
    pub projection_layout: BindGroupLayout,
    pub projection: Option<BindGroup>,
}

impl BindGroups {
    pub fn new(device: &Device) -> Self {
        let texture_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("texture bindgroup layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: Some(NonZero::new(MAX_TEXTURE).unwrap()),
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let projection = ProjectionBindGroups::new(&device);

        Self {
            texture_layout,
            texture: None,
            projection: projection,
        }
    }

    pub fn create_texture_bindgroup(
        &mut self,
        draw_queue: &mut DrawQueue,
        device: &Device,
        queue: &Queue,
    ) {
        let diffuse_texs: &mut Vec<Texture> = &mut draw_queue
            .queue
            .iter()
            .map(|draw_obj| {
                let diffuse_tex = device.create_texture(&TextureDescriptor {
                    size: Extent3d {
                        width: draw_obj.texture.dimension[0],
                        height: draw_obj.texture.dimension[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: TextureFormat::Rgba8UnormSrgb,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    label: None,
                    view_formats: &[],
                });
                queue.write_texture(
                    TexelCopyTextureInfo {
                        texture: &diffuse_tex,
                        mip_level: 0,
                        origin: Origin3d::ZERO,
                        aspect: TextureAspect::All,
                    },
                    &draw_obj.texture.payload,
                    TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(&draw_obj.texture.dimension[0] * 4),
                        rows_per_image: None,
                    },
                    Extent3d {
                        width: draw_obj.texture.dimension[0],
                        height: draw_obj.texture.dimension[1],
                        depth_or_array_layers: 1,
                    },
                );
                diffuse_tex
            })
            .collect();

        if diffuse_texs.len() < MAX_TEXTURE as usize {
            let mut dummy_texs: Vec<Texture> =
                Vec::with_capacity(MAX_TEXTURE as usize - diffuse_texs.len());

            for _ in 0..(MAX_TEXTURE as usize - diffuse_texs.len()) {
                let diffuse_tex = device.create_texture(&TextureDescriptor {
                    size: Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: TextureFormat::Rgba8UnormSrgb,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    label: None,
                    view_formats: &[],
                });
                queue.write_texture(
                    TexelCopyTextureInfo {
                        texture: &diffuse_tex,
                        mip_level: 0,
                        origin: Origin3d::ZERO,
                        aspect: TextureAspect::All,
                    },
                    &[0, 0, 0, 0],
                    TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4),
                        rows_per_image: None,
                    },
                    Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                );
                dummy_texs.push(diffuse_tex);
            }

            diffuse_texs.append(&mut dummy_texs);
        }

        let diffuse_texs_views: Vec<TextureView> = diffuse_texs
            .iter()
            .map(|tex| tex.create_view(&TextureViewDescriptor::default()))
            .collect();

        let diffuse_sampler = device.create_sampler(&SamplerDescriptor {
            label: None,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Nearest,
            mipmap_filter: MipmapFilterMode::Nearest,
            ..Default::default()
        });

        self.texture = Some(
            device.create_bind_group(&BindGroupDescriptor {
                label: None,
                layout: &self.texture_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::TextureViewArray(
                            diffuse_texs_views
                                .iter()
                                .map(|tex| tex)
                                .collect::<Vec<&TextureView>>()
                                .as_slice(),
                        ),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::Sampler(&diffuse_sampler),
                    },
                ],
            }),
        );
    }
}

impl ProjectionBindGroups {
    pub fn new(device: &Device) -> Self {
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("projection bindgroup layout"),
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

        Self {
            projection_layout: layout,
            projection: None,
        }
    }

    pub fn create_projection_bindgroup(
        &mut self,
        buffers: &Buffers,
        device: &Device,
        queue: &Queue,
        camera_uniform: &CameraUniform,
    ) {
        self.projection = Some(device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &self.projection_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: buffers.projection.as_entire_binding(),
            }],
        }));

        queue.write_buffer(&buffers.projection, 0, bytes_of(camera_uniform));
    }
}
