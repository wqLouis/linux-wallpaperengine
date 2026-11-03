use crate::collections::{Map, new_map, sorted_entries};

use super::preprocessor;

#[derive(Debug, Clone)]
pub struct EffectLayout {
    pub sampler_names: Vec<String>,
    pub uniform_decls: Vec<(String, String)>,
    pub uniform_material_keys: Map<String, String>,
    pub uniform_binding: u32,
    pub varying_locations: Map<String, u32>,
    pub varying_types: Map<String, String>,
    /// The number of elements of the varyings that are arrays.
    ///
    /// A varying that is declared as an array has the type of its elements,
    /// so the size is what tells an array apart from a plain vector.
    pub varying_array_sizes: Map<String, u32>,
    /// Varyings that exist in the vertex shader source (includes conditional ones).
    /// Fragment shader `in` declarations are only emitted for varyings in this set.
    pub vertex_varyings: Vec<String>,
    pub attribute_locations: Map<String, u32>,
    /// Whether this shader uses immediates (push constants) for uniforms.
    /// Set by the pipeline handler after computing the total uniform size.
    pub use_immediates: bool,
    /// The path of the shader pair in the wallpaper.
    ///
    /// It is only used for the log messages of the preprocessing: a shader
    /// that could not be preprocessed has to be named, or the failure cannot
    /// be found.  It is empty when the layout does not belong to a shader of
    /// a wallpaper.
    pub shader_path: String,
}

impl EffectLayout {
    pub fn sampler_count(&self) -> usize {
        self.sampler_names.len()
    }

    /// Compute the total size of the uniform block in bytes (std140 layout).
    /// Returns the aligned total size.
    pub fn total_uniform_size(&self) -> u64 {
        compute_uniform_size(&self.uniform_decls)
    }
}

/// Collect the interface of a pair of shaders.
///
/// The layout is collected when the scene is loaded, before the material of
/// an effect is known: the default values of the combos of the shader and the
/// macros of the shader itself decide which branch of the shader is
/// collected.
pub fn collect_layout(source1: &str, source2: &str, headers: &Map<String, String>) -> EffectLayout {
    let defines = super::layout_defines(source1, source2);
    collect_layout_with_defines(source1, source2, headers, &defines)
}

/// Collect the interface of a pair of shaders with an explicit set of macros.
///
/// The macros decide which branch of the shader is compiled, so the layout
/// has to be collected with the same macros as the compilation: a uniform
/// that is only declared in a branch that is not taken must not be in the
/// layout, and a uniform of the branch that is taken has to be.
pub fn collect_layout_with_defines(
    source1: &str,
    source2: &str,
    headers: &Map<String, String>,
    defines: &Map<String, String>,
) -> EffectLayout {
    let vertex = preprocessor::flatten(source1, headers, defines);
    let fragment = preprocessor::flatten(source2, headers, defines);
    collect_layout_from_flattened(&vertex, &fragment)
}

/// Collect the interface of a pair of shaders that are already flattened.
///
/// The flattened sources are the ones the shader compiler sees, so the
/// layout and the shader always describe the same branch.
fn collect_layout_from_flattened(
    vertex: &preprocessor::Flattened,
    fragment: &preprocessor::Flattened,
) -> EffectLayout {
    let mut collector = LayoutCollector::default();

    // Process vertex shader first, tracking all its varyings.
    collector.collect(&vertex.lines);
    // Snapshot all varyings found from the vertex shader.
    let mut vert_varyings: Vec<String> = collector.varying_set.keys().cloned().collect();

    // Then process fragment shader for the full layout.
    collector.collect(&fragment.lines);

    collector.sampler_names.sort();
    collector.sampler_names.dedup();

    // Sorted by name: the declaration order fixes the std140 offsets of
    // the uniform block, so it has to be stable across runs.
    let uniform_decls: Vec<(String, String)> = sorted_entries(&collector.uniform_map)
        .into_iter()
        .map(|(name, ty)| (name.clone(), ty.clone()))
        .collect();
    let uniform_binding = collector.sampler_names.len() as u32 * 2 + 2;

    let mut varying_names: Vec<String> = collector.varying_set.keys().cloned().collect();
    varying_names.sort();
    let mut varying_locations: Map<String, u32> = new_map();
    let mut next_location: u32 = 0;
    for name in &varying_names {
        let array_size = collector
            .varying_array_sizes
            .get(name)
            .copied()
            .unwrap_or(1);
        varying_locations.insert(name.clone(), next_location);
        next_location += array_size;
    }

    let mut attribute_names: Vec<String> = collector.attribute_set.keys().cloned().collect();
    attribute_names.sort();
    let attribute_locations: Map<String, u32> = attribute_names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.clone(), i as u32))
        .collect();

    vert_varyings.sort();
    vert_varyings.dedup();

    EffectLayout {
        sampler_names: collector.sampler_names,
        uniform_decls,
        uniform_material_keys: collector.material_keys,
        uniform_binding,
        varying_locations,
        varying_types: collector.varying_types,
        varying_array_sizes: collector.varying_array_sizes,
        vertex_varyings: vert_varyings,
        attribute_locations,
        use_immediates: false,
        shader_path: String::new(),
    }
}

/// Accumulates the shader interface while recursively walking a source and
/// its `#include`d headers.
#[derive(Default)]
struct LayoutCollector {
    sampler_names: Vec<String>,
    uniform_map: Map<String, String>,
    varying_set: Map<String, u32>,
    varying_types: Map<String, String>,
    varying_array_sizes: Map<String, u32>,
    attribute_set: Map<String, u32>,
    material_keys: Map<String, String>,
}

impl LayoutCollector {
    /// Collect the declarations of the flattened source of a shader.
    ///
    /// The flattened source holds the lines of the branch that is compiled:
    /// the headers are inlined and the lines of the branches that are not
    /// taken are gone.
    fn collect(&mut self, lines: &[preprocessor::Line]) {
        for line in lines {
            let trimmed = line.text.trim();

            if trimmed.starts_with('#') || trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }

            let cleaned = super::strip_material_comments(&line.text);
            let declaration = cleaned.trim();
            if declaration.is_empty() {
                continue;
            }

            if let Some(rest) = declaration.strip_prefix("varying ") {
                let rest = rest.trim();
                if let Some(name) = extract_variable_name(rest) {
                    self.varying_set.entry(name.clone()).or_insert(0);
                    if let Some(ty) = extract_type(rest) {
                        self.varying_types.entry(name.clone()).or_insert(ty);
                    }
                    // Extract array size if present, e.g. vec2 v_TexCoord[4]
                    let array_size = extract_array_size(rest);
                    self.varying_array_sizes.entry(name).or_insert(array_size);
                }
                continue;
            }

            if declaration.contains("attribute ") {
                let rest = declaration.split("attribute ").nth(1).unwrap_or("").trim();
                if let Some(name) = extract_variable_name(rest) {
                    self.attribute_set.entry(name).or_insert(0);
                }
                continue;
            }

            if let Some(rest) = declaration.strip_prefix("uniform ") {
                let rest = rest.trim();
                if rest.starts_with("sampler2D ") || rest.starts_with("sampler2D\t") {
                    let name = rest["sampler2D".len()..].trim().trim_end_matches(';');
                    if !self.sampler_names.contains(&name.to_string()) {
                        self.sampler_names.push(name.to_string());
                    }
                } else {
                    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                    if parts.len() == 2 {
                        let ty = parts[0].trim().to_string();
                        let full_decl = parts[1]
                            .trim()
                            .trim_end_matches(';')
                            .split('=')
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        // Extract base name (strip array brackets) for lookup
                        let name = full_decl.split('[').next().unwrap_or("").to_string();
                        if !name.is_empty() {
                            // Store full declaration including array brackets for proper emission
                            let full_ty = if full_decl.contains('[') {
                                format!("{} {}", ty, &full_decl[name.len()..])
                            } else {
                                ty
                            };
                            self.uniform_map.entry(name.clone()).or_insert(full_ty);
                            if let Some(mk) = extract_material_key(&line.text) {
                                self.material_keys.entry(mk).or_insert(name);
                            }
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
            return obj
                .get("material")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
    }
    None
}

/// The declaration of a uniform in GLSL.
///
/// The type of the layout holds the array brackets behind the type
/// (`float [32]`), but GLSL wants them behind the name
/// (`float name[32]`).  The shader compiler rejects the first form.
pub(crate) fn uniform_declaration(ty: &str, name: &str) -> String {
    match ty.find('[') {
        Some(start) => {
            let base = ty[..start].trim_end();
            let array = ty[start..].trim();
            format!("{} {}{};", base, name, array)
        }
        None => format!("{} {};", ty, name),
    }
}

pub fn extract_variable_name(rest: &str) -> Option<String> {
    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
    if parts.len() >= 2 {
        let name = parts[1]
            .trim()
            .trim_end_matches(';')
            .split('[')
            .next()
            .unwrap_or("")
            .to_string();
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

/// Extract array size from a variable declaration like `vec2 v_TexCoord[4]`.
/// Returns the array size, or 1 if not an array.
pub fn extract_array_size(decl: &str) -> u32 {
    // Look for `[N]` after the variable name
    if let Some(bracket_start) = decl.find('[')
        && let Some(bracket_end) = decl[bracket_start..].find(']')
    {
        let num_str = &decl[bracket_start + 1..bracket_start + bracket_end];
        if let Ok(n) = num_str.parse::<u32>() {
            return n;
        }
    }
    1
}

// ---------------------------------------------------------------------------
// std140 uniform layout helpers
// ---------------------------------------------------------------------------

pub(crate) fn align_up(val: u64, align: u64) -> u64 {
    (val + align - 1) & !(align - 1)
}

/// Compute the total size of a uniform block given its declarations.
pub(crate) fn compute_uniform_size(decls: &[(String, String)]) -> u64 {
    let mut offset: u64 = 0;
    for (_name, ty) in decls {
        offset = align_up(offset, type_align(ty));
        offset += type_size(ty);
    }
    align_up(offset, 16).max(16)
}

/// Parse a type string that may include array brackets, e.g. `"float [32]"`.
/// Returns (base_type, array_size) where array_size is 1 for non-array types.
fn parse_type(ty: &str) -> (&str, u64) {
    let ty = ty.trim();
    if let Some(bracket) = ty.find('[') {
        let base = ty[..bracket].trim();
        let array_part = &ty[bracket..];
        // Parse the array size: [N] or [N][M] (multi-dimensional)
        let mut total_size: u64 = 1;
        let mut rest = array_part;
        while let Some(inner_start) = rest.find('[') {
            if let Some(inner_end) = rest[inner_start..].find(']') {
                let num_str = &rest[inner_start + 1..inner_start + inner_end];
                if let Ok(n) = num_str.parse::<u64>() {
                    total_size *= n;
                }
                rest = &rest[inner_start + inner_end + 1..];
            } else {
                break;
            }
        }
        (base, total_size)
    } else {
        (ty, 1)
    }
}

pub(crate) fn type_align(ty: &str) -> u64 {
    let (base, array_size) = parse_type(ty);
    let base_align = match base {
        "mat4" | "mat3" | "vec4" | "vec3" => 16,
        "vec2" => 8,
        _ => 4,
    };
    if array_size > 1 {
        // std140: array alignment is the element alignment rounded up to vec4 (16)
        align_up(base_align, 16)
    } else {
        base_align
    }
}

pub(crate) fn type_size(ty: &str) -> u64 {
    let (base, array_size) = parse_type(ty);
    let base_size = match base {
        "mat4" => 64,
        "mat3" => 48,
        "vec4" => 16,
        "vec3" => 12,
        "vec2" => 8,
        _ => 4,
    };
    if array_size > 1 {
        // std140: array element stride is the base alignment rounded up to vec4 (16)
        let stride = align_up(base_size, 16);
        stride * array_size
    } else {
        base_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The uniform block layout is derived from the declaration order, so
    /// the declarations have to be emitted in a stable order even though
    /// they are collected in a hash map.
    #[test]
    fn uniform_declarations_are_sorted_by_name() {
        let source = "uniform float g_Beta;\nuniform vec4 g_Alpha;\nuniform float g_Gamma;\n";
        let layout = collect_layout(source, source, &new_map());

        let names: Vec<&str> = layout
            .uniform_decls
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, ["g_Alpha", "g_Beta", "g_Gamma"]);

        // The layout must not depend on the hash seed, so collecting the
        // same source twice has to yield the same declaration order.
        let again = collect_layout(source, source, &new_map());
        assert_eq!(again.uniform_decls, layout.uniform_decls);
    }
}
