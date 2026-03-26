use wgpu::*;

use crate::scene::loader::object_loader::TextureObject;

pub fn get_bindgroup(
    device: &Device,
    queue: &Queue,
    texture_object: &TextureObject,
    sampler: &Sampler,
    layout: &BindGroupLayout,
) -> BindGroup {
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
        layout: layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(
                    &texture.create_view(&TextureViewDescriptor::default()),
                ),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(sampler),
            },
        ],
    });

    bindgroup
}
