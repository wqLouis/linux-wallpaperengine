use std::collections::BTreeMap;

use super::super::shader_header::get_headers;

#[derive(Debug, Clone)]
pub struct EffectLayout {
    pub sampler_names: Vec<String>,
    // Pre-computed bindings (binding = index * 2); used by tests and for documentation
    #[allow(dead_code)]
    pub sampler_bindings: Vec<u32>,
    pub uniform_decls: Vec<(String, String)>,
    pub uniform_material_keys: BTreeMap<String, String>,
    pub uniform_binding: u32,
    pub varying_locations: BTreeMap<String, u32>,
    pub varying_types: BTreeMap<String, String>,
    /// Varyings that exist in the vertex shader source (includes conditional ones).
    /// Fragment shader `in` declarations are only emitted for varyings in this set.
    pub vertex_varyings: Vec<String>,
    pub attribute_locations: BTreeMap<String, u32>,
}

impl EffectLayout {
    pub fn sampler_count(&self) -> usize {
        self.sampler_names.len()
    }
}

pub fn collect_layout(source1: &str, source2: &str) -> EffectLayout {
    let mut sampler_names: Vec<String> = Vec::new();
    let mut uniform_map: BTreeMap<String, String> = BTreeMap::new();
    let mut varying_set: BTreeMap<String, u32> = BTreeMap::new();
    let mut varying_types: BTreeMap<String, String> = BTreeMap::new();
    let mut attribute_set: BTreeMap<String, u32> = BTreeMap::new();
    let mut material_keys: BTreeMap<String, String> = BTreeMap::new();

    // Track vertex varyings for fragment input validation.
    // wgpu requires all fragment inputs to have corresponding vertex outputs.
    let mut vert_varyings: Vec<String> = Vec::new();

    // Process vertex shader (source1) first, tracking all its varyings
    collect_from_source(
        source1,
        &mut sampler_names,
        &mut uniform_map,
        &mut varying_set,
        &mut varying_types,
        &mut attribute_set,
        &mut material_keys,
    );
    // Snapshot all varyings found from vertex shader
    for (name, _) in varying_set.clone() {
        vert_varyings.push(name);
    }

    // Then process fragment shader (source2) for full layout
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

    vert_varyings.sort();
    vert_varyings.dedup();

    EffectLayout {
        sampler_names,
        sampler_bindings,
        uniform_decls,
        uniform_material_keys: material_keys,
        uniform_binding,
        varying_locations,
        varying_types,
        vertex_varyings: vert_varyings,
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
                let name = rest["sampler2D".len()..].trim().trim_end_matches(';');
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
                        if let Some(mk) = extract_material_key(line) {
                            material_keys.entry(mk).or_insert(name);
                        }
                    }
                }
            }
        }
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

fn extract_material_key(line: &str) -> Option<String> {
    if let Some(comment_pos) = line.find("//") {
        let comment = line[comment_pos + 2..].trim();
        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(comment) {
            return obj
                .get("material")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
    }
    None
}

pub fn extract_variable_name(rest: &str) -> Option<String> {
    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
    if parts.len() >= 2 {
        let name = parts[1].trim().trim_end_matches(';').to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

pub fn extract_type(rest: &str) -> Option<String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if !parts.is_empty() {
        return Some(parts[0].to_string());
    }
    None
}
