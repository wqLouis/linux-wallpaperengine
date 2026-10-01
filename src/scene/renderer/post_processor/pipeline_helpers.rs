use std::collections::BTreeMap;

use wgpu::*;

use crate::scene::loader::shader::{EffectLayout, WM_SAMPLER_BINDING};

pub fn apply_texture_combos(
    defines: &mut BTreeMap<String, String>,
    pass_textures: &[Option<String>],
) {
    // The textures array index is the GL texture unit number.
    // textures[0] = source (g_Texture0, handled separately),
    // textures[1] = first user texture (g_Texture1, MASK combo),
    // textures[2] = second user texture (g_Texture2, TIMEOFFSET combo).
    if pass_textures.get(1).and_then(|t| t.as_deref()).is_some() {
        defines
            .entry("MASK".to_string())
            .or_insert_with(|| "1".to_string());
    }
    if pass_textures.get(2).and_then(|t| t.as_deref()).is_some() {
        defines
            .entry("TIMEOFFSET".to_string())
            .or_insert_with(|| "1".to_string());
    }
}

pub fn create_effect_bindgroup_layout(device: &Device, layout: &EffectLayout) -> BindGroupLayout {
    let mut entries = Vec::new();

    for (i, _name) in layout.sampler_names.iter().enumerate() {
        entries.push(BindGroupLayoutEntry {
            binding: i as u32 * 2,
            visibility: ShaderStages::VERTEX_FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
    }

    entries.push(BindGroupLayoutEntry {
        binding: WM_SAMPLER_BINDING,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Sampler(SamplerBindingType::Filtering),
        count: None,
    });

    // When using immediates (push constants), uniforms are supplied via
    // set_immediates() rather than a buffer binding. Skip the UBO entry
    // so the bind group layout matches the bind group (which also omits it).
    if !layout.uniform_decls.is_empty() && !layout.use_immediates {
        entries.push(BindGroupLayoutEntry {
            binding: layout.uniform_binding,
            visibility: ShaderStages::VERTEX_FRAGMENT,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
    }

    device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: None,
        entries: &entries,
    })
}
