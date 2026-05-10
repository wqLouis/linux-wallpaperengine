mod layout;
mod replace;

use std::collections::HashSet;

use wgpu::naga::ShaderStage;

use super::shader_header::get_headers;
pub use layout::EffectLayout;
pub use layout::collect_layout;

// Re-export WM_SAMPLER_BINDING from shader_header for convenience
pub use super::shader_header::WM_SAMPLER_BINDING;

pub fn preprocess_with_layout(source: &str, stage: ShaderStage, layout: &EffectLayout) -> String {
    let (result, _) = preprocess_with_layout_tracked(source, stage, layout);
    result
}

/// Preprocess a shader and also track which varyings were emitted in the output.
pub fn preprocess_with_layout_tracked(
    source: &str,
    stage: ShaderStage,
    layout: &EffectLayout,
) -> (String, Vec<String>) {
    let mut result = String::with_capacity(source.len() + 4096);
    let mut emitted_varyings: Vec<String> = Vec::new();
    let mut if_depth: u32 = 0;
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
            // Track #if/#endif depth to identify unconditional varying declarations
            if trimmed.starts_with("#if") {
                if_depth += 1;
            } else if trimmed.starts_with("#endif") {
                if_depth = if_depth.saturating_sub(1);
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
            let name = layout::extract_variable_name(rest);
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
            let name = layout::extract_variable_name(rest);

            // For fragment shaders, skip varyings not present in the vertex shader
            // source at all (these are dead code / unused declarations).
            // Conditional vertex varyings (inside #if blocks) are handled by
            // the hoisting logic in preprocess_pair.
            if stage == ShaderStage::Fragment {
                if let Some(ref n) = name {
                    if !layout.vertex_varyings.iter().any(|v| v == n) {
                        continue;
                    }
                }
            }

            let location = name
                .as_ref()
                .and_then(|n| layout.varying_locations.get(n))
                .copied()
                .unwrap_or(0);
            result.push_str(&format!(
                "layout(location={}) {} {}\n",
                location, keyword, rest
            ));
            // Track varyings emitted unconditionally (outside #if blocks)
            // for vertex→fragment varying matching
            if stage == ShaderStage::Vertex && if_depth == 0 {
                if let Some(n) = name {
                    emitted_varyings.push(n);
                }
            }
            continue;
        }

        let mut transformed = cleaned;
        transformed = transformed.replace("texSample2D(", "texture(");
        transformed = transformed.replace("texSample2DLod(", "textureLod(");
        transformed = transformed.replace("gl_FragColor", "_fragColor");
        transformed = replace::fix_implicit_truncation(&transformed, &layout.varying_types);
        transformed = replace::replace_mul(&transformed);
        transformed = replace::replace_texture_calls(&transformed, &sampler_set);
        transformed = transformed.replace("CAST2(", "vec2(");
        transformed = transformed.replace("CAST3(", "vec3(");
        transformed = transformed.replace("CAST4(", "vec4(");
        transformed = transformed.replace("CAST3X3(", "mat3(");
        transformed = replace::replace_saturate(&transformed);
        transformed = replace::replace_frac(&transformed);
        transformed = transformed.replace("ddx(", "dFdx(");
        transformed = transformed.replace("ddy(", "dFdy(");
        transformed = replace::replace_atan2(&transformed);
        transformed = replace::replace_reserved_identifiers(&transformed);

        result.push_str(&transformed);
        result.push('\n');
    }

    (result, emitted_varyings)
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

/// Preprocess a vertex and fragment shader pair, returning the transformed
/// source code and collected layout information.
pub fn preprocess_pair(vert: &str, frag: &str) -> (String, String, EffectLayout) {
    let layout = collect_layout(vert, frag);
    let (mut vert_out, vert_emitted) =
        preprocess_with_layout_tracked(vert, ShaderStage::Vertex, &layout);
    let frag_out = preprocess_with_layout(frag, ShaderStage::Fragment, &layout);

    // Ensure vertex always outputs all varyings unconditionally.
    // Some varyings are only declared inside #if blocks in the vertex source,
    // but the fragment may reference them unconditionally. wgpu requires all
    // fragment inputs to have corresponding vertex outputs.
    let missing: Vec<&String> = layout
        .vertex_varyings
        .iter()
        .filter(|v| !vert_emitted.iter().any(|e| e == *v))
        .collect();

    if !missing.is_empty() {
        vert_out = hoist_conditional_varyings(&vert_out, &layout, &missing);
    }

    (vert_out, frag_out, layout)
}

/// Hoist varying declarations out of #if blocks so they are always available
/// as vertex outputs, while keeping the assignment code inside #if blocks.
fn hoist_conditional_varyings(output: &str, layout: &EffectLayout, missing: &[&String]) -> String {
    let mut result = String::with_capacity(output.len() + 512);
    let mut if_depth: u32 = 0;
    let mut hoisted_decls: Vec<String> = Vec::new();
    let mut hoisted_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in output.lines() {
        let trimmed = line.trim();

        // Track #if depth
        if trimmed.starts_with("#if") {
            if_depth += 1;
        } else if trimmed.starts_with("#endif") {
            if_depth = if_depth.saturating_sub(1);
        }

        // Check if this is a conditional varying declaration (vertex stage: "out")
        if if_depth > 0 && trimmed.starts_with("layout(") && trimmed.contains(") out ") {
            let name = extract_pp_varying_name(trimmed);
            if let Some(ref n) = name {
                if missing.iter().any(|v| v == &n) && !hoisted_names.contains(n) {
                    // Collect this declaration to hoist outside #if blocks
                    hoisted_names.insert(n.clone());
                    hoisted_decls.push(line.to_string());
                    continue; // Skip the inside-#if copy
                }
            }
        }

        result.push_str(line);
        result.push('\n');
    }

    // Prepend hoisted declarations at the appropriate position
    // (after #version and #define headers, before main code)
    if !hoisted_decls.is_empty() {
        let insertion = find_decl_insertion_point(&result);
        let decl_block: String = hoisted_decls.iter().map(|d| format!("{}\n", d)).collect();
        result.insert_str(insertion, &decl_block);
    }

    // Add zero-initialization in main() for hoisted varyings
    for var_name in &hoisted_names {
        let ty = layout
            .varying_types
            .get(var_name.as_str())
            .map(|s: &String| s.as_str())
            .unwrap_or("vec4");
        result = add_varying_init(&result, var_name, ty);
    }

    result
}

/// Extract the variable name from a preprocessed varying line like
/// "layout(location=1) out vec2 v_TexCoordMask;"
fn extract_pp_varying_name(line: &str) -> Option<String> {
    // Split on ") out " or ") in " to get "TYPE NAME;"
    let after_qualifier = line.split(") out ").nth(1)?;
    // Split on whitespace: "vec2 v_TexCoordMask;" -> ["vec2", "v_TexCoordMask;"]
    let parts: Vec<&str> = after_qualifier.split_whitespace().collect();
    if parts.len() >= 2 {
        Some(parts[1].trim_end_matches(';').to_string())
    } else {
        None
    }
}

/// Find the insertion point for hoisted declarations: after #version and #define headers.
fn find_decl_insertion_point(output: &str) -> usize {
    let mut pos = 0usize;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#version") || trimmed.starts_with("#define") {
            pos += line.len() + 1;
        } else {
            break;
        }
    }
    pos
}

/// Insert a zero-initialization for a varying at the beginning of main().
fn add_varying_init(output: &str, var_name: &str, ty: &str) -> String {
    if let Some(main_pos) = output.find("void main()") {
        // Find the opening brace of main
        if let Some(brace_pos) = output[main_pos..].find('{') {
            let insert_pos = main_pos + brace_pos + 1;
            let init_line = match ty {
                "vec2" => format!("\n    {} = vec2(0.0);", var_name),
                "vec3" => format!("\n    {} = vec3(0.0);", var_name),
                "vec4" => format!("\n    {} = vec4(0.0);", var_name),
                "float" => format!("\n    {} = 0.0;", var_name),
                "int" => format!("\n    {} = 0;", var_name),
                _ => format!("\n    {} = {}(0.0);", var_name, ty),
            };
            let mut result = output.to_string();
            result.insert_str(insert_pos, &init_line);
            return result;
        }
    }
    output.to_string()
}
