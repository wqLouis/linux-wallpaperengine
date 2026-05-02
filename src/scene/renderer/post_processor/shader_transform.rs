use std::collections::HashSet;

use wgpu::naga::ShaderStage;

use super::{
    shader_header::{WM_SAMPLER_BINDING, get_headers},
    shader_layout::{self, EffectLayout},
    shader_replace,
};

pub fn preprocess_with_layout(source: &str, stage: ShaderStage, layout: &EffectLayout) -> String {
    let mut result = String::with_capacity(source.len() + 4096);
    result.push_str("#version 450\n");

    emit_declarations(&mut result, stage, layout);

    let sampler_set: HashSet<&str> = layout.sampler_names.iter().map(|s| s.as_str()).collect();
    let headers = get_headers();

    for line in source.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("#include") {
            if let Some(start) = trimmed.find('"') {
                if let Some(end) = trimmed[start + 1..].find('"') {
                    let include_file = &trimmed[start + 1..start + 1 + end];
                    if let Some(header_content) = headers.get(include_file) {
                        for hline in header_content.lines() {
                            let htrim = hline.trim();
                            if htrim.is_empty() || htrim.starts_with("//") || htrim.starts_with('#')
                            {
                                continue;
                            }
                            result.push_str(hline);
                            result.push('\n');
                        }
                        continue;
                    }
                }
            }
            continue;
        }

        if trimmed.is_empty() || trimmed.starts_with("//") {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        if trimmed.starts_with('#') {
            if trimmed.starts_with("#define M_PI")
                || trimmed.starts_with("#define M_PI_HALF")
                || trimmed.starts_with("#define M_PI_2")
                || trimmed.starts_with("#define SQRT_2")
                || trimmed.starts_with("#define SQRT_3")
                || trimmed.starts_with("#version")
            {
                continue;
            }
            result.push_str(line);
            result.push('\n');
            continue;
        }

        let cleaned = strip_material_comments(line);
        if cleaned.is_empty() {
            result.push('\n');
            continue;
        }

        if cleaned.contains("uniform ")
            || cleaned.starts_with("uniform ")
            || cleaned.starts_with("sampler2D ")
        {
            continue;
        }

        if cleaned.contains("attribute ") {
            let rest = cleaned.split("attribute ").nth(1).unwrap_or("").trim();
            let name = shader_layout::extract_variable_name(rest);
            let location = name
                .as_ref()
                .and_then(|n| layout.attribute_locations.get(n))
                .copied()
                .unwrap_or(0);
            result.push_str(&format!("layout(location={}) in {}\n", location, rest));
            continue;
        }

        if cleaned.starts_with("varying ") {
            let rest = cleaned["varying ".len()..].trim();
            let keyword = match stage {
                ShaderStage::Vertex => "out",
                ShaderStage::Fragment => "in",
                _ => "in",
            };
            let name = shader_layout::extract_variable_name(rest);
            let location = name
                .as_ref()
                .and_then(|n| layout.varying_locations.get(n))
                .copied()
                .unwrap_or(0);
            result.push_str(&format!(
                "layout(location={}) {} {}\n",
                location, keyword, rest
            ));
            continue;
        }

        let mut transformed = cleaned;
        transformed = transformed.replace("texSample2D(", "texture(");
        transformed = transformed.replace("texSample2DLod(", "textureLod(");
        transformed = transformed.replace("gl_FragColor", "_fragColor");
        transformed = shader_replace::fix_implicit_truncation(&transformed, &layout.varying_types);
        transformed = shader_replace::replace_mul(&transformed);
        transformed = shader_replace::replace_texture_calls(&transformed, &sampler_set);
        transformed = transformed.replace("CAST2(", "vec2(");
        transformed = transformed.replace("CAST3(", "vec3(");
        transformed = transformed.replace("CAST4(", "vec4(");
        transformed = transformed.replace("CAST3X3(", "mat3(");
        transformed = shader_replace::replace_saturate(&transformed);
        transformed = shader_replace::replace_frac(&transformed);
        transformed = transformed.replace("ddx(", "dFdx(");
        transformed = transformed.replace("ddy(", "dFdy(");
        transformed = shader_replace::replace_atan2(&transformed);

        result.push_str(&transformed);
        result.push('\n');
    }

    result
}

fn emit_declarations(result: &mut String, stage: ShaderStage, layout: &EffectLayout) {
    let headers = get_headers();
    for content in headers.values() {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }
            if trimmed.starts_with('#') {
                result.push_str(line);
                result.push('\n');
            }
        }
    }

    if stage == ShaderStage::Fragment {
        result.push_str("out vec4 _fragColor;\n");
    }

    for (i, name) in layout.sampler_names.iter().enumerate() {
        result.push_str(&format!(
            "layout(binding={}) uniform texture2D {};\n",
            i as u32 * 2,
            name
        ));
    }

    result.push_str(&format!(
        "layout(binding={}) uniform sampler _wm_sampler;\n",
        WM_SAMPLER_BINDING
    ));

    if !layout.uniform_decls.is_empty() {
        result.push_str(&format!(
            "layout(binding={}, std140) uniform EffectParams {{\n",
            layout.uniform_binding
        ));
        for (name, ty) in &layout.uniform_decls {
            result.push_str(&format!("    {} {};\n", ty, name));
        }
        result.push_str("};\n");
    }
}

fn strip_material_comments(line: &str) -> String {
    if let Some(comment_pos) = line.find("//") {
        let before = &line[..comment_pos];
        let after_comment = &line[comment_pos..];
        if after_comment.contains("[COMBO]") {
            return line.to_string();
        }
        let trimmed_before = before.trim_end();
        if trimmed_before.is_empty() {
            return String::new();
        }
        return trimmed_before.to_string();
    }
    line.to_string()
}
