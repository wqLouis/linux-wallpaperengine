use std::num::NonZero;

use wgpu::*;

use crate::MAX_TEXTURE;

pub struct BindGroups {
    pub texture_layout: BindGroupLayout,
    pub texture: Option<BindGroup>,
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

        Self {
            texture_layout,
            texture: None,
        }
    }

    pub fn create_texture_bindgroup(&mut self, device: &Device, queue: &Queue) {}
}
