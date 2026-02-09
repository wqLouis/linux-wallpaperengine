use bytemuck::bytes_of;
use depkg::pkg_parser::tex_parser::Tex;
use wgpu::*;

use crate::scene::{Camera, Root, camera::CameraUniform};

pub fn create_tex_bind_group(
    device: &Device,
    queue: &Queue,
    bind_group_layout: &BindGroupLayout,
    tex: &Tex,
    root: &Root,
    projection_buffer: &Buffer,
) -> BindGroup {
    let diffuse_tex = device.create_texture(&TextureDescriptor {
        size: Extent3d {
            width: tex.dimension[0],
            height: tex.dimension[1],
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

    let bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: bind_group_layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(
                    &diffuse_tex.create_view(&TextureViewDescriptor::default()),
                ),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(&diffuse_sampler),
            },
            BindGroupEntry {
                binding: 2,
                resource: projection_buffer.as_entire_binding(),
            },
        ],
    });

    queue.write_texture(
        TexelCopyTextureInfo {
            texture: &diffuse_tex,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        &tex.payload,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * tex.dimension[0]),
            rows_per_image: Some(tex.dimension[1]),
        },
        Extent3d {
            width: tex.dimension[0],
            height: tex.dimension[1],
            depth_or_array_layers: 1,
        },
    );

    queue.write_buffer(
        &projection_buffer,
        0,
        bytes_of(&root.camera.new(&root.general).create_projection_matrix()),
    );

    bind_group
}
