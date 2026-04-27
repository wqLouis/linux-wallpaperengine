#![allow(dead_code)]
use std::collections::{BTreeMap, HashSet};
use wgpu::naga::ShaderStage;

const COMMON_H: &str = r#"#define M_PI 3.14159265359
#define M_PI_HALF 1.57079632679
#define M_PI_2 6.28318530718
#define SQRT_2 1.41421356237
#define SQRT_3 1.73205080756

vec3 hsv2rgb(vec3 c) {
    vec4 K = vec4(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    vec3 p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, 0.0, 1.0), c.y);
}

vec3 rgb2hsv(vec3 c) {
    vec4 K = vec4(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    vec4 p = mix(vec4(c.bg, K.wz), vec4(c.gb, K.xy), step(c.b, c.g));
    vec4 q = mix(vec4(p.xyw, c.r), vec4(c.r, p.yzx), step(p.x, c.r));
    float d = q.x - min(q.w, q.y);
    float e = 1.0e-10;
    return vec3(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

vec2 rotateVec2(vec2 v, float r) {
    return vec2(v.x * cos(r) - v.y * sin(r), v.x * sin(r) + v.y * cos(r));
}

float greyscale(vec3 c) {
    return dot(c, vec3(0.299, 0.587, 0.114));
}
"#;

const COMMON_PERSPECTIVE_H: &str = r#"
mat3 squareToQuad(vec2 p0, vec2 p1, vec2 p2, vec2 p3) {
    float dx1 = p1.x - p2.x;
    float dy1 = p1.y - p2.y;
    float dx2 = p3.x - p2.x;
    float dy2 = p3.y - p2.y;
    float dx3 = p0.x - p1.x + p2.x - p3.x;
    float dy3 = p0.y - p1.y + p2.y - p3.y;

    float det = dx1 * dy2 - dy1 * dx2;
    if (abs(det) < 1e-10) {
        return mat3(1.0);
    }

    float g = (dx3 * dy2 - dy3 * dx2) / det;
    float h = (dx1 * dy3 - dy1 * dx3) / det;

    return mat3(
        p1.x - p0.x + g * p1.x,
        p3.x - p0.x + h * p3.x,
        p0.x,
        p1.y - p0.y + g * p1.y,
        p3.y - p0.y + h * p3.y,
        p0.y,
        g,
        h,
        1.0
    );
}
"#;

pub const WM_SAMPLER_BINDING: u32 = 1;

fn get_headers() -> BTreeMap<&'static str, &'static str> {
    let mut map = BTreeMap::new();
    map.insert("common.h", COMMON_H);
    map.insert("common_perspective.h", COMMON_PERSPECTIVE_H);
    map
}

#[derive(Debug, Clone)]
pub struct EffectLayout {
    pub sampler_names: Vec<String>,
    pub sampler_bindings: Vec<u32>,
    pub uniform_decls: Vec<(String, String)>,
    pub uniform_material_keys: BTreeMap<String, String>,
    pub uniform_binding: u32,
    pub varying_names: Vec<String>,
    pub varying_locations: BTreeMap<String, u32>,
    pub varying_types: BTreeMap<String, String>,
    pub attribute_names: Vec<String>,
    pub attribute_locations: BTreeMap<String, u32>,
}

impl EffectLayout {
    pub fn sampler_count(&self) -> usize {
        self.sampler_names.len()
    }

    pub fn uniform_count(&self) -> usize {
        self.uniform_decls.len()
    }
}

pub fn preprocess_pair(vert: &str, frag: &str) -> (String, String, EffectLayout) {
    let layout = collect_layout(vert, frag);
    let vert_out = preprocess_with_layout(vert, ShaderStage::Vertex, &layout);
    let frag_out = preprocess_with_layout(frag, ShaderStage::Fragment, &layout);
    (vert_out, frag_out, layout)
}

pub fn preprocess(source: &str, stage: ShaderStage) -> String {
    let layout = collect_layout(source, "");
    preprocess_with_layout(source, stage, &layout)
}

fn collect_layout(source1: &str, source2: &str) -> EffectLayout {
    let mut sampler_names: Vec<String> = Vec::new();
    let mut uniform_map: BTreeMap<String, String> = BTreeMap::new();
    let mut varying_set: BTreeMap<String, u32> = BTreeMap::new();
    let mut varying_types: BTreeMap<String, String> = BTreeMap::new();
    let mut attribute_set: BTreeMap<String, u32> = BTreeMap::new();
    let mut material_keys: BTreeMap<String, String> = BTreeMap::new();

    collect_from_source(
        source1,
        &mut sampler_names,
        &mut uniform_map,
        &mut varying_set,
        &mut varying_types,
        &mut attribute_set,
        &mut material_keys,
    );
    collect_from_source(
        source2,
        &mut sampler_names,
        &mut uniform_map,
        &mut varying_set,
        &mut varying_types,
        &mut attribute_set,
        &mut material_keys,
    );

    sampler_names.sort();
    sampler_names.dedup();

    let sampler_bindings: Vec<u32> = sampler_names
        .iter()
        .enumerate()
        .map(|(i, _)| i as u32 * 2)
        .collect();

    let uniform_decls: Vec<(String, String)> = uniform_map.into_iter().collect();

    let uniform_binding = sampler_names.len() as u32 * 2 + 2;

    let mut varying_names: Vec<String> = varying_set.keys().cloned().collect();
    varying_names.sort();

    let varying_locations: BTreeMap<String, u32> = varying_names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.clone(), i as u32))
        .collect();

    let mut attribute_names: Vec<String> = attribute_set.keys().cloned().collect();
    attribute_names.sort();

    let attribute_locations: BTreeMap<String, u32> = attribute_names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.clone(), i as u32))
        .collect();

    EffectLayout {
        sampler_names,
        sampler_bindings,
        uniform_decls,
        uniform_material_keys: material_keys,
        uniform_binding,
        varying_names,
        varying_locations,
        varying_types,
        attribute_names,
        attribute_locations,
    }
}

fn collect_from_source(
    source: &str,
    sampler_names: &mut Vec<String>,
    uniform_map: &mut BTreeMap<String, String>,
    varying_set: &mut BTreeMap<String, u32>,
    varying_types: &mut BTreeMap<String, String>,
    attribute_set: &mut BTreeMap<String, u32>,
    material_keys: &mut BTreeMap<String, String>,
) {
    let headers = get_headers();

    for line in source.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("#include") {
            if let Some(start) = trimmed.find('"') {
                if let Some(end) = trimmed[start + 1..].find('"') {
                    let include_file = &trimmed[start + 1..start + 1 + end];
                    if let Some(header_content) = headers.get(include_file) {
                        collect_from_source(
                            header_content,
                            sampler_names,
                            uniform_map,
                            varying_set,
                            varying_types,
                            attribute_set,
                            material_keys,
                        );
                    }
                }
            }
            continue;
        }

        if trimmed.starts_with('#') || trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }

        let cleaned = strip_material_comments(line);
        if cleaned.is_empty() {
            continue;
        }

        if cleaned.starts_with("varying ") {
            let rest = cleaned["varying ".len()..].trim();
            if let Some(name) = extract_variable_name(rest) {
                varying_set.entry(name.clone()).or_insert(0);
                if let Some(ty) = extract_type(rest) {
                    varying_types.entry(name).or_insert(ty);
                }
            }
            continue;
        }

        if cleaned.contains("attribute ") {
            let rest = cleaned.split("attribute ").nth(1).unwrap_or("").trim();
            if let Some(name) = extract_variable_name(rest) {
                attribute_set.entry(name).or_insert(0);
            }
            continue;
        }

        if cleaned.starts_with("uniform ") {
            let rest = cleaned["uniform ".len()..].trim();

            if rest.starts_with("sampler2D ") || rest.starts_with("sampler2D\t") {
                let name = rest["sampler2D".len()..]
                    .trim()
                    .trim_end_matches(';');
                if !sampler_names.contains(&name.to_string()) {
                    sampler_names.push(name.to_string());
                }
            } else {
                let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                if parts.len() == 2 {
                    let ty = parts[0].trim().to_string();
                    let name = parts[1]
                        .trim()
                        .trim_end_matches(';')
                        .split('=')
                        .next()
                        .unwrap_or("")
                        .split('[')
                        .next()
                        .unwrap_or("")
                        .to_string();
                    if !name.is_empty() {
                        uniform_map.entry(name.clone()).or_insert(ty);

                        let material_key = extract_material_key(line);
                        if let Some(mk) = material_key {
                            material_keys.entry(mk).or_insert(name);
                        }
                    }
                }
            }
        }
    }
}

fn extract_material_key(line: &str) -> Option<String> {
    if let Some(comment_pos) = line.find("//") {
        let comment = line[comment_pos + 2..].trim();
        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(comment) {
            return obj.get("material").and_then(|v| v.as_str()).map(|s| s.to_string());
        }
    }
    None
}

fn extract_variable_name(rest: &str) -> Option<String> {
    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
    if parts.len() >= 2 {
        let name = parts[1]
            .trim()
            .trim_end_matches(';')
            .to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

fn extract_type(rest: &str) -> Option<String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if !parts.is_empty() {
        return Some(parts[0].to_string());
    }
    None
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

fn preprocess_with_layout(source: &str, stage: ShaderStage, layout: &EffectLayout) -> String {
    let mut result = String::with_capacity(source.len() + 4096);
    result.push_str("#version 450\n");

    emit_declarations(&mut result, stage, layout);

    let sampler_set: HashSet<&str> = layout
        .sampler_names
        .iter()
        .map(|s| s.as_str())
        .collect();
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
                            if htrim.is_empty() || htrim.starts_with("//") || htrim.starts_with('#') {
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
            let name = extract_variable_name(rest);
            let location = name
                .as_ref()
                .and_then(|n| layout.attribute_locations.get(n))
                .copied()
                .unwrap_or(0);
            let transformed = format!("layout(location={}) in {}", location, rest);
            result.push_str(&transformed);
            result.push('\n');
            continue;
        }

        if cleaned.starts_with("varying ") {
            let rest = cleaned["varying ".len()..].trim();
            let keyword = match stage {
                ShaderStage::Vertex => "out",
                ShaderStage::Fragment => "in",
                _ => "in",
            };
            let name = extract_variable_name(rest);
            let location = name
                .as_ref()
                .and_then(|n| layout.varying_locations.get(n))
                .copied()
                .unwrap_or(0);
            let transformed = format!("layout(location={}) {} {}", location, keyword, rest);
            result.push_str(&transformed);
            result.push('\n');
            continue;
        }

        let mut transformed = cleaned;
        transformed = transformed.replace("texSample2D(", "texture(");
        transformed = transformed.replace("texSample2DLod(", "textureLod(");
        transformed = transformed.replace("gl_FragColor", "_fragColor");
        transformed = fix_implicit_truncation(&transformed, &layout.varying_types);
        transformed = replace_mul(&transformed);
        transformed = replace_texture_calls(&transformed, &sampler_set);
        transformed = transformed.replace("CAST2(", "vec2(");
        transformed = transformed.replace("CAST3(", "vec3(");
        transformed = transformed.replace("CAST4(", "vec4(");
        transformed = transformed.replace("CAST3X3(", "mat3(");
        transformed = replace_saturate(&transformed);
        transformed = replace_frac(&transformed);
        transformed = transformed.replace("ddx(", "dFdx(");
        transformed = transformed.replace("ddy(", "dFdy(");
        transformed = replace_atan2(&transformed);

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
        let binding = i as u32 * 2;
        result.push_str(&format!(
            "layout(binding={}) uniform texture2D {};\n",
            binding, name
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

fn replace_texture_calls(line: &str, sampler_set: &HashSet<&str>) -> String {
    let mut result = line.to_string();

    for func in &["texture(", "textureLod("] {
        let mut search_start = 0;
        while let Some(pos) = result[search_start..].find(*func) {
            let abs_start = search_start + pos;
            let args_start = abs_start + func.len();

            let mut depth = 1;
            let mut arg1_end = args_start;
            let mut found_comma = false;
            for (i, ch) in result[args_start..].char_indices() {
                if ch == '(' {
                    depth += 1;
                } else if ch == ')' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                } else if ch == ',' && depth == 1 && !found_comma {
                    arg1_end = args_start + i;
                    found_comma = true;
                }
            }

            if !found_comma || arg1_end <= args_start {
                search_start = abs_start + 1;
                continue;
            }

            let arg1 = result[args_start..arg1_end].trim().to_string();
            if sampler_set.contains(arg1.as_str()) {
                let replacement = format!("{}sampler2D({}, _wm_sampler), ", func, arg1);
                result.replace_range(abs_start..arg1_end + 1, &replacement);
                search_start = abs_start + replacement.len();
            } else {
                search_start = abs_start + 1;
            }
        }
    }

    result
}

fn fix_implicit_truncation(line: &str, varying_types: &BTreeMap<String, String>) -> String {
    if !line.contains('=') || line.contains('*') || line.contains('+') || line.contains('-') {
        return line.to_string();
    }
    if !line.ends_with(';') {
        return line.to_string();
    }

    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() != 2 {
        return line.to_string();
    }

    let lhs = parts[0].trim();
    let rhs = parts[1].trim().trim_end_matches(';').trim();

    let lhs_base = lhs.split('.').next().unwrap_or(lhs).trim();
    let _rhs_swizzle = rhs.contains('.');
    let rhs_base = rhs.split('.').next().unwrap_or(rhs).trim();

    let lhs_type = varying_types.get(lhs_base);
    let rhs_type = varying_types.get(rhs_base);

    match (lhs_type, rhs_type) {
        (Some(l), Some(r)) if l != r && !lhs.contains('.') && !_rhs_swizzle => {
            let swizzle = match (r.as_str(), l.as_str()) {
                ("vec4", "vec2") | ("vec3", "vec2") => ".xy",
                ("vec4", "vec3") => ".xyz",
                _ => return line.to_string(),
            };
            format!("{} = {}{};", lhs, rhs, swizzle)
        }
        _ => line.to_string(),
    }
}

fn replace_mul(line: &str) -> String {
    if !line.contains("mul(") {
        return line.to_string();
    }

    let mut result = line.to_string();
    let mut search_start = 0;

    while let Some(mul_start) = result[search_start..].find("mul(") {
        let abs_start = search_start + mul_start;
        let args_start = abs_start + 4;

        let mut depth = 1;
        let mut args_end = args_start;
        for (i, ch) in result[args_start..].char_indices() {
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                depth -= 1;
                if depth == 0 {
                    args_end = args_start + i;
                    break;
                }
            }
        }

        if depth != 0 {
            break;
        }

        let args = &result[args_start..args_end];

        if let Some(comma_pos) = find_top_level_comma(args) {
            let arg1 = args[..comma_pos].trim();
            let arg2 = args[comma_pos + 1..].trim();

            let mut replacement = format!("{} * {}", arg2, arg1);
            let mul_end = args_end + 1;
            if result.as_bytes().get(mul_end) == Some(&b'.') {
                replacement = format!("({})", replacement);
            }
            result.replace_range(abs_start..mul_end, &replacement);
            search_start = abs_start + replacement.len();
        } else {
            search_start = abs_start + 1;
        }
    }

    result
}

fn find_top_level_comma(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn replace_saturate(line: &str) -> String {
    if !line.contains("saturate(") {
        return line.to_string();
    }
    let mut result = line.to_string();
    let mut search_start = 0;

    while let Some(sat_start) = result[search_start..].find("saturate(") {
        let abs_start = search_start + sat_start;
        let args_start = abs_start + 9;

        let mut depth = 1;
        let mut args_end = args_start;
        for (i, ch) in result[args_start..].char_indices() {
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                depth -= 1;
                if depth == 0 {
                    args_end = args_start + i;
                    break;
                }
            }
        }

        if depth != 0 {
            break;
        }

        let arg = &result[args_start..args_end].trim();
        let replacement = format!("clamp({}, 0.0, 1.0)", arg);
        let sat_end = args_end + 1;
        result.replace_range(abs_start..sat_end, &replacement);
        search_start = abs_start + replacement.len();
    }

    result
}

fn replace_frac(line: &str) -> String {
    if !line.contains("frac(") {
        return line.to_string();
    }
    let mut result = line.to_string();
    let mut search_start = 0;
    while let Some(frac_start) = result[search_start..].find("frac(") {
        let abs_start = search_start + frac_start;
        if abs_start > 0
            && result[..abs_start]
                .chars()
                .last()
                .map_or(false, |c| c.is_alphanumeric() || c == '_')
        {
            search_start = abs_start + 1;
            continue;
        }
        result.replace_range(abs_start..abs_start + 4, "fract");
        search_start = abs_start + 5;
    }
    result
}

fn replace_atan2(line: &str) -> String {
    if !line.contains("atan2(") {
        return line.to_string();
    }
    line.replace("atan2(", "atan(")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_mul_vec_matrix() {
        let input = "gl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix);";
        let output = replace_mul(input);
        assert_eq!(
            output,
            "gl_Position = g_ModelViewProjectionMatrix * vec4(a_Position, 1.0);"
        );
    }

    #[test]
    fn test_replace_saturate() {
        let input = "float a = saturate(g_Sensitivity) + step(0.0001, negPerspective);";
        let output = replace_saturate(input);
        assert_eq!(
            output,
            "float a = clamp(g_Sensitivity, 0.0, 1.0) + step(0.0001, negPerspective);"
        );
    }

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
        let vert = include_str!("../../../../test/yurucamp/shaders/effects/waterwaves.vert");
        let frag = include_str!("../../../../test/yurucamp/shaders/effects/waterwaves.frag");
        let (vert_out, frag_out, layout) = preprocess_pair(vert, frag);

        // Layout: 3 samplers -> [g_Texture0, g_Texture1, g_Texture2]
        assert_eq!(layout.sampler_names.len(), 3);
        assert!(layout.sampler_names.contains(&"g_Texture0".to_string()));
        assert!(layout.sampler_names.contains(&"g_Texture1".to_string()));
        assert!(layout.sampler_names.contains(&"g_Texture2".to_string()));

        // Vertex output — varyings sorted alphabetically: v_Direction(0), v_Direction2(1), v_TexCoord(2), v_TexCoordPerspective(3)
        // attributes sorted alphabetically: a_Position(0), a_TexCoord(1)
        assert!(vert_out.contains("layout(location=0) in vec3 a_Position;"));
        assert!(vert_out.contains("layout(location=1) in vec2 a_TexCoord;"));
        assert!(vert_out.contains("layout(location=2) out vec4 v_TexCoord;"));
        assert!(vert_out.contains("layout(binding=0) uniform texture2D g_Texture0;"));
        assert!(vert_out.contains("layout(binding=1) uniform sampler _wm_sampler;"));
        assert!(vert_out.contains("g_ModelViewProjectionMatrix * vec4(a_Position, 1.0)"));
        assert!(vert_out.contains("uniform EffectParams"));

        // Fragment output — varyings get same locations via layout.varying_locations
        assert!(frag_out.contains("layout(location=2) in vec4 v_TexCoord;"));
        assert!(frag_out.contains("layout(binding=0) uniform texture2D g_Texture0;"));
        assert!(frag_out.contains("layout(binding=2) uniform texture2D g_Texture1;"));
        assert!(frag_out.contains("layout(binding=4) uniform texture2D g_Texture2;"));
        assert!(frag_out.contains("sampler2D(g_Texture0, _wm_sampler)"));
        assert!(frag_out.contains("rotateVec2"));
    }

    #[test]
    fn test_preprocess_cloudmotion() {
        let vert = include_str!("../../../../test/yurucamp/shaders/effects/cloudmotion.vert");
        let frag = include_str!("../../../../test/yurucamp/shaders/effects/cloudmotion.frag");
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
