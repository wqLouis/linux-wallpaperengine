use std::rc::Rc;

use wgpu::*;

use crate::scene::renderer::video_renderer::{ffmpeg_decode, vk_decode::VkVideoDecoder};

pub struct VideoBindGroups {
    video_layout: BindGroupLayout,
    video_bindgroup: Option<BindGroup>,
}

pub struct Video {
    frame_textures: Vec<TextureView>,
    res: [i32; 2],
    fps: f32,
    duration: i64,
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
            video_bindgroup: None,
        }
    }
}

pub fn decode(video: Rc<Vec<u8>>, surface: &Surface) -> Option<Video> {
    let payload = Rc::try_unwrap(video).unwrap();
    let h246 = ffmpeg_decode::decode_to_h264(payload).ok()?;

    let decoder = VkVideoDecoder::new(h246.payload, surface);
    let frames = decoder.decode();
    let frames = frames
        .into_iter()
        .map(|frame| frame.data.create_view(&TextureViewDescriptor::default()))
        .collect::<Vec<TextureView>>();

    Some(Video {
        frame_textures: frames,
        res: h246.res,
        fps: h246.fps,
        duration: h246.duration,
    })
}
