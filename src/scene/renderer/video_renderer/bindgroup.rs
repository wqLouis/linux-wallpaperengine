use wgpu::*;

use crate::scene::renderer::bindgroup::ProjectionBindGroups;

pub struct VideoBindGroups {
    video_layout: BindGroupLayout,
    texture: Option<BindGroup>,

    projection: ProjectionBindGroups,
}

impl VideoBindGroups {
    fn new(device: &Device) -> VideoBindGroups {
        let video_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2Array,
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
        let projection = ProjectionBindGroups::new(&device);

        Self {
            video_layout: video_layout,
            texture: None,
            projection: projection,
        }
    }
}
