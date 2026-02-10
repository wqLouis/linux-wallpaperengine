use bytemuck::bytes_of;
use depkg::pkg_parser::tex_parser::Tex;
use wgpu::{wgt::TextureViewDescriptor, *};
use winit::dpi::PhysicalSize;

use crate::scene::Root;

pub fn create_tex_bind_group(
    device: &Device,
    queue: &Queue,
    bind_group_layout: &BindGroupLayout,
    projection_bind_group_layout: &BindGroupLayout,
    texs: &Vec<Tex>,
    root: &Root,
    projection_buffer: &Buffer,
    window_size: &PhysicalSize<f32>,
) -> (BindGroup, BindGroup) {
    const TEX_COUNT: usize = 256;

    let diffuse_texs: Vec<Texture> = texs
        .iter()
        .map(|tex| {
            device.create_texture(&TextureDescriptor {
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
            })
        })
        .collect();

    let mut padding_tex: Vec<TextureView> = Vec::new();
    if diffuse_texs.len() < TEX_COUNT {
        let dummy_tex = vec![vec![0u8; 4]; TEX_COUNT - diffuse_texs.len()];
        padding_tex = dummy_tex
            .iter()
            .map(|tex_raw| {
                let tex = device.create_texture(&TextureDescriptor {
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
                        texture: &tex,
                        mip_level: 0,
                        origin: Origin3d::ZERO,
                        aspect: TextureAspect::All,
                    },
                    tex_raw,
                    TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4),
                        rows_per_image: Some(1),
                    },
                    Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                );
                tex.create_view(&TextureViewDescriptor::default())
            })
            .collect::<Vec<TextureView>>();
    }

    let mut diffuse_tex_views = diffuse_texs
        .iter()
        .map(|tex| tex.create_view(&TextureViewDescriptor::default()))
        .collect::<Vec<TextureView>>();
    if diffuse_tex_views.len() < TEX_COUNT {
        diffuse_tex_views.append(&mut padding_tex);
    }

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

    let tex_bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: bind_group_layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureViewArray(
                    diffuse_tex_views
                        .iter()
                        .map(|view| view)
                        .collect::<Vec<&TextureView>>()
                        .as_slice(),
                ),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(&diffuse_sampler),
            },
        ],
    });

    let projection_bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: projection_bind_group_layout,
        entries: &[BindGroupEntry {
            binding: 0,
            resource: projection_buffer.as_entire_binding(),
        }],
    });

    for (diffuse_tex, tex) in diffuse_texs.iter().zip(texs) {
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
    }

    queue.write_buffer(
        &projection_buffer,
        0,
        bytes_of(
            &root
                .camera
                .new(&root.general)
                .create_projection_matrix(window_size),
        ),
    );

    (tex_bind_group, projection_bind_group)
}
