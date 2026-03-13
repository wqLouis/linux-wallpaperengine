use std::rc::Rc;

use wgpu::*;

pub struct VideoBindGroups {
    video_layout: BindGroupLayout,
    texture: Option<BindGroup>,
}

impl VideoBindGroups {
    pub fn new(device: &Device) -> Self {
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

        Self {
            video_layout: video_layout,
            texture: None,
        }
    }

    pub async fn play_video(video: Rc<Vec<u8>>) {}
}
