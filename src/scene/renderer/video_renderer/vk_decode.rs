use std::rc::Rc;

struct VkVideoRenderer {
    decoder: vk_video::WgpuTexturesDecoder,
    video: Rc<Vec<u8>>,
}
