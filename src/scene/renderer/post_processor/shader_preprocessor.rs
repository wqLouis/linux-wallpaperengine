//! Shader preprocessing for converting GLSL to WGSL.
//!
//! This module transforms Wallpaper Engine GLSL shaders into WGSL format
//! compatible with WebGPU, handling layout collection, variable replacement,
//! and conditional varying hoisting.

use wgpu::naga::ShaderStage;

pub use super::shader_header::WM_SAMPLER_BINDING;
pub use super::transform::EffectLayout;
pub use super::transform::collect_layout;
pub use super::transform::preprocess_with_layout;

/// Preprocess a vertex and fragment shader pair, returning the transformed
/// source code and collected layout information.
pub fn preprocess_pair(vert: &str, frag: &str) -> (String, String, EffectLayout) {
    let layout = collect_layout(vert, frag);
    let (mut vert_out, vert_emitted) =
        super::transform::preprocess_with_layout_tracked(vert, ShaderStage::Vertex, &layout);
    let frag_out = preprocess_with_layout(frag, ShaderStage::Fragment, &layout);

    // Ensure vertex always outputs all varyings unconditionally.
    // Some varyings are only declared inside #if blocks in the vertex source,
    // but the fragment may reference them unconditionally. wgpu requires all
    // fragment inputs to have corresponding vertex outputs.
    //
    // Strategy: find varying declarations inside #if blocks and hoist them
    // outside, while keeping the assignment code inside the #if block.
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

pub fn preprocess(source: &str, stage: ShaderStage) -> String {
    let layout = collect_layout(source, "");
    preprocess_with_layout(source, stage, &layout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_declarations() {
        let vert = r#"
uniform mat4 g_ModelViewProjectionMatrix;
uniform float g_Time;
uniform sampler2D g_Texture0;
uniform sampler2D g_Texture1;
"#;
        let frag = r#"
uniform sampler2D g_Texture0;
uniform sampler2D g_Texture2;
uniform float g_Speed;
"#;
        let layout = collect_layout(vert, frag);
        assert_eq!(layout.sampler_names.len(), 3);
        assert!(layout.sampler_names.contains(&"g_Texture0".to_string()));
        assert!(layout.sampler_names.contains(&"g_Texture1".to_string()));
        assert!(layout.sampler_names.contains(&"g_Texture2".to_string()));
        assert_eq!(layout.uniform_decls.len(), 3);
    }

    #[test]
    fn test_preprocess_waterwaves() {
        let vert = include_str!("../../../../../test/yurucamp/shaders/effects/waterwaves.vert");
        let frag = include_str!("../../../../../test/yurucamp/shaders/effects/waterwaves.frag");
        let (vert_out, frag_out, layout) = preprocess_pair(vert, frag);

        assert_eq!(layout.sampler_names.len(), 3);
        assert!(layout.sampler_names.contains(&"g_Texture0".to_string()));
        assert!(layout.sampler_names.contains(&"g_Texture1".to_string()));
        assert!(layout.sampler_names.contains(&"g_Texture2".to_string()));

        assert!(vert_out.contains("layout(location=0) in vec3 a_Position;"));
        assert!(vert_out.contains("layout(location=1) in vec2 a_TexCoord;"));
        assert!(vert_out.contains("layout(location=2) out vec4 v_TexCoord;"));
        assert!(vert_out.contains("layout(binding=0) uniform texture2D g_Texture0;"));
        assert!(vert_out.contains("layout(binding=1) uniform sampler _wm_sampler;"));
        assert!(vert_out.contains("g_ModelViewProjectionMatrix * vec4(a_Position, 1.0)"));
        assert!(vert_out.contains("uniform EffectParams"));

        assert!(frag_out.contains("layout(location=2) in vec4 v_TexCoord;"));
        assert!(frag_out.contains("layout(binding=0) uniform texture2D g_Texture0;"));
        assert!(frag_out.contains("layout(binding=2) uniform texture2D g_Texture1;"));
        assert!(frag_out.contains("layout(binding=4) uniform texture2D g_Texture2;"));
        assert!(frag_out.contains("sampler2D(g_Texture0, _wm_sampler)"));
        assert!(frag_out.contains("rotateVec2"));
    }

    #[test]
    fn test_preprocess_cloudmotion() {
        let vert = include_str!("../../../../../test/yurucamp/shaders/effects/cloudmotion.vert");
        let frag = include_str!("../../../../../test/yurucamp/shaders/effects/cloudmotion.frag");
        let (vert_out, _frag_out, layout) = preprocess_pair(vert, frag);

        assert!(layout.varying_types.contains_key("v_NoiseCoord"));
        assert_eq!(layout.varying_types.get("v_NoiseCoord").unwrap(), "vec2");
        assert_eq!(layout.varying_types.get("v_TexCoord").unwrap(), "vec4");

        assert!(
            vert_out.contains("v_NoiseCoord = v_TexCoord.xy;"),
            "Expected truncation fix, got:\n{}",
            vert_out
        );
    }

    #[test]
    fn test_preprocess_pair_basic() {
        let vert = r#"
uniform mat4 g_ModelViewProjectionMatrix;
uniform float g_Time;
uniform sampler2D g_Texture0;
attribute vec3 a_Position;
attribute vec2 a_TexCoord;
varying vec4 v_TexCoord;

void main() {
    gl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix);
    v_TexCoord = a_TexCoord.xyxy;
}
"#;
        let frag = r#"
#include "common.h"
uniform sampler2D g_Texture0;
uniform float g_Time;
varying vec4 v_TexCoord;

void main() {
    gl_FragColor = texSample2D(g_Texture0, v_TexCoord.xy);
}
"#;
        let (vert_out, frag_out, layout) = preprocess_pair(vert, frag);

        assert!(vert_out.contains("layout(location=0) in vec3 a_Position;"));
        assert!(vert_out.contains("layout(location=0) out vec4 v_TexCoord;"));
        assert!(vert_out.contains("layout(binding=0) uniform texture2D g_Texture0;"));
        assert!(vert_out.contains("g_ModelViewProjectionMatrix * vec4(a_Position, 1.0)"));

        assert!(frag_out.contains("layout(location=0) in vec4 v_TexCoord;"));
        assert!(frag_out.contains("sampler2D(g_Texture0, _wm_sampler)"));

        assert_eq!(layout.sampler_names, vec!["g_Texture0"]);
        assert_eq!(layout.sampler_bindings, vec![0]);
    }
}
