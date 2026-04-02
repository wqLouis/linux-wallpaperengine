use std::collections::BTreeMap;

use wgpu::*;

use crate::scene::renderer::buffer::Buffers;

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
