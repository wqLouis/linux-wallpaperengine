use std::{collections::BTreeMap, rc::Rc};

use serde_json::Value;
use wgpu::*;

use crate::scene::renderer::{
    post_process::PostProcess,
    post_processor::{effect_param, pipeline_handler::EffectPipelineData, shader_header::WM_SAMPLER_BINDING},
};

pub struct EffectBindGroup {
    pub uniform_buffer: Option<Buffer>,
    pub uniform_layout: effect_param::UniformLayout,
    pub material_keys: BTreeMap<String, String>,
    pub constants: BTreeMap<String, Value>,
    pub tex_resolutions: BTreeMap<String, [f32; 4]>,
    pub blank_view: TextureView,
    pub mask_view: Option<TextureView>,
    pub noise_view: Option<TextureView>,
    pub _mask_tex: Option<Texture>,
    pub _noise_tex: Option<Texture>,
}

impl EffectBindGroup {
    pub fn new(
        device: &Device, post_process: &PostProcess, pipedata: &EffectPipelineData,
        source_view: &TextureView, mask_view: Option<&TextureView>, noise_view: Option<&TextureView>,
        _pipeline: Rc<RenderPipeline>,
        material_keys: BTreeMap<String, String>, constants: BTreeMap<String, Value>,
        tex_resolutions: BTreeMap<String, [f32; 4]>,
        mask_tex: Option<Texture>, noise_tex: Option<Texture>,
    ) -> Option<Self> {
        let blank_view = post_process.blank_texture.create_view(&Default::default());

        let uniform_buffer = if !pipedata.layout.uniform_decls.is_empty() {
            Some(device.create_buffer(&BufferDescriptor {
                label: None, size: pipedata.uniform_layout.total_size(),
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST, mapped_at_creation: false,
            }))
        } else { None };

        let mut entries: Vec<BindGroupEntry<'_>> = Vec::with_capacity(pipedata.layout.sampler_count() + 2);
        for i in 0..pipedata.layout.sampler_count() {
            let view = match i {
                0 => source_view,
                1 => mask_view.unwrap_or(&blank_view),
                2 => noise_view.unwrap_or(&blank_view),
                _ => &blank_view,
            };
            entries.push(BindGroupEntry { binding: i as u32 * 2, resource: BindingResource::TextureView(view) });
        }
        entries.push(BindGroupEntry { binding: WM_SAMPLER_BINDING, resource: BindingResource::Sampler(&post_process.sampler) });
        if let Some(ref buf) = uniform_buffer {
            entries.push(BindGroupEntry { binding: pipedata.layout.uniform_binding, resource: buf.as_entire_binding() });
        }

        Some(Self {
            uniform_buffer,
            uniform_layout: pipedata.uniform_layout.clone(),
            material_keys, constants, tex_resolutions, blank_view,
            mask_view: mask_view.map(|v| v.clone()),
            noise_view: noise_view.map(|v| v.clone()),
            _mask_tex: mask_tex, _noise_tex: noise_tex,
        })
    }
}
