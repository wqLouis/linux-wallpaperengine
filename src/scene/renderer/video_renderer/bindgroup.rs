use std::rc::Rc;

use wgpu::*;

use crate::scene::renderer::video_renderer::vk_decode::VkVideoDecoder;

pub struct VideoBindGroups {
    video_layout: BindGroupLayout,
    video_bindgroup: Option<BindGroup>,
    video: Rc<Vec<u8>>,
}

impl VideoBindGroups {
    pub fn new(device: &Device, video: Rc<Vec<u8>>) -> Self {
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
            video_bindgroup: None,
            video,
        }
    }

    pub fn bind(&self, device: &Device, surface: &Surface) {
        let decoder = VkVideoDecoder::new(Rc::clone(&self.video), device, surface);
        let frames = decoder.decode();
        let frames = frames
            .into_iter()
            .map(|frame| frame.data)
            .collect::<Vec<Texture>>();
    }
}
