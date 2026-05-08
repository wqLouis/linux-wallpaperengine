use std::{collections::BTreeMap, rc::Rc};

use serde_json::Value;
use wgpu::*;

use crate::scene::renderer::{
    post_process::PostProcess,
    post_processor::{
        effect_param, pipeline_handler::EffectPipelineData, shader_preprocessor::WM_SAMPLER_BINDING,
    },
};

pub fn make_effect_intermediate_bindgroup(
    device: &Device,
    pipedata: &EffectPipelineData,
    effect_bg: &EffectBindGroup,
    source_view: &TextureView,
    sampler: &Sampler,
) -> BindGroup {
    let mut entries = Vec::new();

    for i in 0..pipedata.layout.sampler_count() {
        let view: &TextureView = match i {
            0 => source_view,
            1 => effect_bg
                .mask_view
                .as_ref()
                .unwrap_or(&effect_bg.blank_view),
            2 => effect_bg
                .noise_view
                .as_ref()
                .unwrap_or(&effect_bg.blank_view),
            _ => &effect_bg.blank_view,
        };
        entries.push(BindGroupEntry {
            binding: i as u32 * 2,
            resource: BindingResource::TextureView(view),
        });
    }

    entries.push(BindGroupEntry {
        binding: WM_SAMPLER_BINDING,
        resource: BindingResource::Sampler(sampler),
    });

    if let Some(ref buf) = effect_bg.uniform_buffer {
        entries.push(BindGroupEntry {
            binding: pipedata.layout.uniform_binding,
            resource: buf.as_entire_binding(),
        });
    }

    device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &pipedata.bindgroup_layout,
        entries: &entries,
    })
}

pub struct EffectBindGroup {
    pub pipeline: Rc<RenderPipeline>,
    // Bind group recreated per-frame via make_effect_intermediate_bindgroup;
    // kept here for potential single-pass rendering path
    #[allow(dead_code)]
    pub bindgroup: BindGroup,
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
        device: &Device,
        post_process: &PostProcess,
        pipedata: &EffectPipelineData,
        source_view: &TextureView,
        mask_view: Option<&TextureView>,
        noise_view: Option<&TextureView>,
        pipeline: Rc<RenderPipeline>,
        material_keys: BTreeMap<String, String>,
        constants: BTreeMap<String, Value>,
        tex_resolutions: BTreeMap<String, [f32; 4]>,
        mask_tex: Option<Texture>,
        noise_tex: Option<Texture>,
    ) -> Option<Self> {
        let sampler_count = pipedata.layout.sampler_count();
        let has_uniforms = !pipedata.layout.uniform_decls.is_empty();

        let blank_view = post_process.blank_texture.create_view(&Default::default());

        let uniform_buffer = if has_uniforms {
            Some(device.create_buffer(&BufferDescriptor {
                label: None,
                size: pipedata.uniform_layout.total_size(),
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }))
        } else {
            None
        };

        let mut entries: Vec<BindGroupEntry<'_>> = Vec::new();
        for i in 0..sampler_count {
            let view: &TextureView = match i {
                0 => source_view,
                1 => mask_view.unwrap_or(&blank_view),
                2 => noise_view.unwrap_or(&blank_view),
                _ => &blank_view,
            };
            entries.push(BindGroupEntry {
                binding: i as u32 * 2,
                resource: BindingResource::TextureView(view),
            });
        }

        entries.push(BindGroupEntry {
            binding: WM_SAMPLER_BINDING,
            resource: BindingResource::Sampler(&post_process.sampler),
        });

        if let Some(ref buf) = uniform_buffer {
            entries.push(BindGroupEntry {
                binding: pipedata.layout.uniform_binding,
                resource: buf.as_entire_binding(),
            });
        }

        let bindgroup = device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &pipedata.bindgroup_layout,
            entries: &entries,
        });

        // Clone views for storage (TextureView is just a handle, cheap to clone)
        let stored_mask_view = mask_view.map(|v| v.clone());
        let stored_noise_view = noise_view.map(|v| v.clone());

        Some(Self {
            pipeline,
            bindgroup,
            uniform_buffer,
            uniform_layout: pipedata.uniform_layout.clone(),
            material_keys,
            constants,
            tex_resolutions,
            blank_view,
            mask_view: stored_mask_view,
            noise_view: stored_noise_view,
            _mask_tex: mask_tex,
            _noise_tex: noise_tex,
        })
    }
}
