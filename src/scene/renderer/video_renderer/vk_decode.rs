use vk_video::WgpuTexturesDecoder;
use wgpu::*;

pub struct VkVideoDecoder {
    video: Vec<u8>,
    decoder: WgpuTexturesDecoder,
}

impl VkVideoDecoder {
    pub fn new(video: Vec<u8>, surface: &Surface) -> Self {
        let instance = vk_video::VulkanInstance::new().unwrap();
        let adapter = instance.create_adapter(Some(surface)).unwrap();
        let device = adapter
            .create_device(
                wgpu::Features::empty(),
                wgpu::ExperimentalFeatures::disabled(),
                wgpu::Limits::defaults(),
            )
            .unwrap();
        let decoder = device
            .create_wgpu_textures_decoder(vk_video::parameters::DecoderParameters::default())
            .unwrap();

        Self { video, decoder }
    }

    pub fn decode(mut self) -> Vec<vk_video::Frame<Texture>> {
        self.decoder
            .decode(vk_video::EncodedInputChunk {
                data: self.video.as_slice(),
                pts: None,
            })
            .unwrap()
    }
}
