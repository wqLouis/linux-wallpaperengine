use std::collections::BTreeMap;

use serde_json::Value;
use wgpu::*;

use super::transform::{EffectLayout, WM_SAMPLER_BINDING};

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

pub fn collect_default_defines(vert_source: &str, frag_source: &str) -> BTreeMap<String, String> {
    let mut defines = BTreeMap::new();

    for source in &[vert_source, frag_source] {
        for line in source.lines() {
            let trimmed = line.trim();
            if let Some(combo_start) = trimmed.find("[COMBO]") {
                let json_str = trimmed[combo_start + 7..].trim();
                let Ok(combo) = serde_json::from_str::<Value>(json_str) else {
                    continue;
                };
                if let (Some(combo_key), Some(default_val)) = (
                    combo.get("combo").and_then(|v: &Value| v.as_str()),
                    combo.get("default"),
                ) {
                    let default_str = match default_val {
                        Value::Number(n) => n.to_string(),
                        Value::String(s) => s.clone(),
                        Value::Bool(b) => (*b as i32).to_string(),
                        _ => continue,
                    };
                    defines.entry(combo_key.to_string()).or_insert(default_str);
                }
            }
        }
    }

    defines
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

    if !layout.uniform_decls.is_empty() {
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
