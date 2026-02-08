use depkg::pkg_parser::tex_parser::Tex;
use wgpu::*;

pub fn create_tex_bind_group(
    device: &Device,
    queue: &Queue,
    bind_group_layout: &BindGroupLayout,
    tex: &Tex,
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

    bind_group
}
