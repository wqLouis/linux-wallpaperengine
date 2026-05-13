mod layout;
mod replace;

use std::collections::{BTreeMap, HashSet};

use wgpu::naga::ShaderStage;

pub use layout::EffectLayout;
pub use layout::collect_layout;

// Re-export WM_SAMPLER_BINDING from shader_header for convenience
pub use super::shader_header::WM_SAMPLER_BINDING;

/// Collect simple `#define NAME VALUE` macros from a shader/header source.
/// Only top-level defines (not inside `#if` blocks) are collected.
pub fn collect_source_defines(source: &str) -> BTreeMap<String, String> {
    let mut macros = BTreeMap::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#define ") {
            let rest = trimmed["#define ".len()..].trim();
            if let Some(space_pos) = rest.find(|c: char| c.is_whitespace()) {
                let name = rest[..space_pos].trim().to_string();
                let value = rest[space_pos..].trim().to_string();
                // Only collect simple numeric or identifier values
                if !name.is_empty() {
                    macros.insert(name, value);
                }
            }
        }
    }
    macros
}

/// Evaluate a preprocessor condition like `BLENDMODE == 26` or `defined(MACRO)`.
/// `defines` contains combo values; `source_macros` contains `#define` macros from
/// shader source (e.g. `BOTTOM` → `0`).
fn eval_if_condition(cond: &str, defines: &BTreeMap<String, String>) -> bool {
    let cond = cond.trim();

    // Handle `defined(NAME)`
    if let Some(inner) = cond.strip_prefix("defined(") {
        if let Some(name) = inner.strip_suffix(')') {
            return defines.contains_key(name.trim());
        }
    }

    // Handle `!defined(NAME)`
    if let Some(rest) = cond.strip_prefix('!') {
        return !eval_if_condition(rest.trim(), defines);
    }

    // Handle `NAME == VALUE` or `NAME != VALUE`
    for (op, op_len) in &[("==", 2), ("!=", 2)] {
        if let Some(pos) = cond.find(op) {
            let name = cond[..pos].trim();
            let value = cond[pos + op_len..].trim();
            let def = defines.get(name).map(|s| s.as_str()).unwrap_or("0");
            let rhs = defines.get(value).map(|s| s.as_str()).unwrap_or(value);
            return if *op == "==" { def == rhs } else { def != rhs };
        }
    }

    // Handle `||` and `&&` (simple left-to-right, no precedence — enough for WE headers)
    if let Some(or_pos) = cond.find("||") {
        return eval_if_condition(&cond[..or_pos], defines)
            || eval_if_condition(&cond[or_pos + 2..], defines);
    }
    if let Some(and_pos) = cond.find("&&") {
        return eval_if_condition(&cond[..and_pos], defines)
            && eval_if_condition(&cond[and_pos + 2..], defines);
    }

    // Handle `!NAME` (negation of a single macro)
    if let Some(name) = cond.strip_prefix('!') {
        let trimmed = name.trim();
        return !defines.contains_key(trimmed)
            || defines.get(trimmed).map(|s| s.as_str()) == Some("0");
    }

    // Bare macro name: truthy if defined and != "0"
    let val = defines.get(cond);
    match val {
        Some(v) => v != "0",
        None => false,
    }
}

enum IfBlockState {
    /// This block is being output
    Active,
    /// This block is skipped
    Inactive,
    /// An `#else` or active `#elif` has been seen for this chain
    Done,
}

/// Tracks the state of `#if`/`#ifdef`/`#ifndef`/`#elif`/`#else`/`#endif` blocks
/// during shader preprocessing.
struct IfBlockProcessor {
    stack: Vec<IfBlockState>,
}

impl IfBlockProcessor {
    fn new() -> Self {
        Self { stack: Vec::new() }
    }

    fn is_active(&self) -> bool {
        self.stack.iter().all(|s| matches!(s, IfBlockState::Active))
    }

    /// Process a conditional preprocessor directive.
    /// Returns `Some(true)` if the line was handled (caller should `continue`).
    /// Returns `None` if the line is not a conditional directive, or is
    /// `#else` with trailing content (caller decides how to handle it).
    fn process_line(&mut self, line: &str, defines: &BTreeMap<String, String>) -> Option<bool> {
        let trimmed = line.trim();

        if trimmed.starts_with("#ifdef") {
            let macro_name = trimmed["#ifdef".len()..].trim();
            let cond_true = defines.contains_key(macro_name);
            self.stack.push(if cond_true { IfBlockState::Active } else { IfBlockState::Inactive });
            return Some(true);
        }
        if trimmed.starts_with("#ifndef") {
            let macro_name = trimmed["#ifndef".len()..].trim();
            let cond_true = defines.contains_key(macro_name);
            self.stack.push(if !cond_true { IfBlockState::Active } else { IfBlockState::Inactive });
            return Some(true);
        }
        if trimmed.starts_with("#if")
            && !trimmed.starts_with("#ifdef")
            && !trimmed.starts_with("#ifndef")
        {
            let cond = trimmed["#if".len()..].trim();
            let cond_true = eval_if_condition(cond, defines);
            self.stack.push(if cond_true { IfBlockState::Active } else { IfBlockState::Inactive });
            return Some(true);
        }

        if trimmed.starts_with("#elif") {
            if let Some(top) = self.stack.last_mut() {
                match top {
                    IfBlockState::Active | IfBlockState::Done => {
                        *top = IfBlockState::Done;
                    }
                    IfBlockState::Inactive => {
                        let cond = trimmed["#elif".len()..].trim();
                        if eval_if_condition(cond, defines) {
                            *top = IfBlockState::Active;
                        }
                    }
                }
            }
            return Some(true);
        }

        if trimmed.starts_with("#else") {
            if let Some(top) = self.stack.last_mut() {
                match top {
                    IfBlockState::Active => *top = IfBlockState::Done,
                    IfBlockState::Inactive => *top = IfBlockState::Active,
                    IfBlockState::Done => {}
                }
            }
            return Some(true);
        }

        if trimmed.starts_with("#endif") {
            self.stack.pop();
            return Some(true);
        }

        None
    }
}

/// Preprocess a shader and also track which varyings were emitted in the output.
pub fn preprocess_with_layout_tracked(
    source: &str,
    stage: ShaderStage,
    layout: &EffectLayout,
    headers: &BTreeMap<String, String>,
    defines: &BTreeMap<String, String>,
) -> (String, Vec<String>) {
    let mut result = String::with_capacity(source.len() + 4096);
    let mut emitted_varyings: Vec<String> = Vec::new();
    let mut ifp = IfBlockProcessor::new();

    result.push_str("#version 450\n");

    emit_declarations(&mut result, stage, layout, headers);

    let sampler_set: HashSet<&str> = layout.sampler_names.iter().map(|s| s.as_str()).collect();

    for line in source.lines() {
        let trimmed = line.trim();

        // #include handling (header inlining with #if evaluation)
        if trimmed.starts_with("#include") {
            if ifp.is_active() {
                if let Some(start) = trimmed.find('"') {
                    if let Some(end) = trimmed[start + 1..].find('"') {
                        let file = &trimmed[start + 1..start + 1 + end];
                        if let Some(hdr) = headers.get(file) {
                            include_header_lines(hdr, defines, &sampler_set, &mut result, headers);
                        }
                    }
                }
            }
            continue;
        }

        // Preprocessor directives
        if trimmed.starts_with('#') {
            let was_active_before = ifp.is_active();
            if ifp.process_line(line, defines) == Some(true) {
                continue;
            }
            if !was_active_before && !ifp.is_active() {
                continue;
            }
            // Skip standard constants already emitted by emit_declarations
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

        // Skip lines inside inactive #if blocks (preserve line count with newlines)
        if !ifp.is_active() {
            result.push('\n');
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with("//") {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        let cleaned = strip_material_comments(line);
        if cleaned.is_empty() {
            result.push('\n');
            continue;
        }

        if cleaned.starts_with("uniform ") || cleaned.starts_with("sampler2D ") {
            continue;
        }

        if cleaned.contains("attribute ") {
            let rest = cleaned.split("attribute ").nth(1).unwrap_or("").trim();
            let loc = layout::extract_variable_name(rest)
                .and_then(|n| layout.attribute_locations.get(&n).copied())
                .unwrap_or(0);
            result.push_str(&format!("layout(location={}) in {}\n", loc, rest));
            continue;
        }

        if cleaned.starts_with("varying ") {
            let rest = cleaned["varying ".len()..].trim();
            let keyword = if matches!(stage, ShaderStage::Vertex) { "out" } else { "in" };
            let name = layout::extract_variable_name(rest);

            // Fragment: skip varyings not present in vertex shader source
            if stage == ShaderStage::Fragment {
                if let Some(ref n) = name {
                    if !layout.vertex_varyings.iter().any(|v| v == n) {
                        continue;
                    }
                }
            }

            let location = name.as_ref()
                .and_then(|n| layout.varying_locations.get(n))
                .copied().unwrap_or(0);
            result.push_str(&format!("layout(location={}) {} {}\n", location, keyword, rest));

            // Track emitted varyings for hoisting logic in preprocess_pair
            if stage == ShaderStage::Vertex && ifp.is_active() {
                if let Some(n) = name {
                    emitted_varyings.push(n);
                }
            }
            continue;
        }

        let mut transformed = apply_shader_transforms(&cleaned, &sampler_set, &layout.varying_types);
        transformed = transformed.replace("ddx(", "dFdx(");
        transformed = transformed.replace("ddy(", "dFdy(");
        transformed = transformed.replace("atan2(", "atan(");
        transformed = replace::replace_reserved_identifiers(&transformed);

        result.push_str(&transformed);
        result.push('\n');
    }

    (result, emitted_varyings)
}

fn emit_declarations(
    result: &mut String,
    stage: ShaderStage,
    layout: &EffectLayout,
    headers: &BTreeMap<String, String>,
) {
    // Emit #define lines from all headers, skipping those inside #if blocks.
    let mut if_depth: u32 = 0;
    for content in headers.values() {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("#include") {
                continue;
            }
            if trimmed.starts_with("#if") {
                if_depth += 1;
                continue;
            }
            if trimmed.starts_with("#endif") {
                if_depth = if_depth.saturating_sub(1);
                continue;
            }
            if trimmed.starts_with("#else") || trimmed.starts_with("#elif") {
                continue;
            }
            if if_depth > 0 {
                continue;
            }
            if trimmed.starts_with('#') {
                let transformed = apply_shader_transforms(trimmed, &HashSet::new(), &BTreeMap::new());
                result.push_str(&transformed);
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
    let Some(comment_pos) = line.find("//") else {
        return line.to_string();
    };
    // Preserve [COMBO] annotations even inside comments
    if line[comment_pos..].contains("[COMBO]") {
        return line.to_string();
    }
    let before = line[..comment_pos].trim_end();
    if before.is_empty() { String::new() } else { before.to_string() }
}

/// Shared GLSL→Vulkan transformations applied to every non-preprocessor line.
fn apply_shader_transforms(line: &str, sampler_set: &HashSet<&str>,
                           varying_types: &BTreeMap<String, String>) -> String {
    let mut t = line.to_string();
    t = t.replace("CAST2(", "vec2(");
    t = t.replace("CAST3(", "vec3(");
    t = t.replace("CAST4(", "vec4(");
    t = t.replace("CAST3X3(", "mat3(");
    t = replace::replace_saturate(&t);
    t = replace::replace_frac(&t);
    t = t.replace("texSample2D(", "texture(");
    t = t.replace("texSample2DLod(", "textureLod(");
    t = t.replace("gl_FragColor", "_fragColor");
    t = replace::replace_mul(&t);
    t = replace::replace_texture_calls(&t, sampler_set);
    t = replace::fix_implicit_truncation(&t, varying_types);
    t = replace::replace_bool_arithmetic(&t);
    t = replace::replace_float_as_bool(&t);
    t
}

/// Preprocess a vertex and fragment shader pair, returning the transformed
/// source code and collected layout information.
/// Recursively expand a header's content into `result`, evaluating
/// `#if`/`#ifdef` blocks using `defines` so only active branches are emitted.
fn include_header_lines(
    content: &str,
    defines: &BTreeMap<String, String>,
    sampler_set: &HashSet<&str>,
    result: &mut String,
    headers: &BTreeMap<String, String>,
) {
    let mut ifp = IfBlockProcessor::new();

    for hline in content.lines() {
        let htrim = hline.trim();

        // Handle nested #include (recursively expand)
        if htrim.starts_with("#include") {
            if ifp.is_active() {
                if let Some(start) = htrim.find('"') {
                    if let Some(end) = htrim[start + 1..].find('"') {
                        let include_file = &htrim[start + 1..start + 1 + end];
                        if let Some(nested) = headers.get(include_file) {
                            include_header_lines(nested, defines, sampler_set, result, headers);
                        }
                    }
                }
            }
            continue;
        }

        // Handle preprocessor directives
        if htrim.starts_with('#') {
            if ifp.process_line(hline, defines) == Some(true) {
                continue;
            }
            continue; // skip #define etc. (already emitted by emit_declarations)
        }

        if htrim.is_empty() || htrim.starts_with("//") || !ifp.is_active() {
            continue;
        }

        // Skip declarations (already emitted by emit_declarations)
        let htrim_lower = htrim.to_lowercase();
        if htrim_lower.starts_with("uniform ")
            || htrim_lower.starts_with("sampler2d ")
            || htrim_lower.starts_with("varying ")
            || htrim_lower.starts_with("attribute ")
        {
            continue;
        }

        let transformed = apply_shader_transforms(htrim, sampler_set, &BTreeMap::new());
        result.push_str(&transformed);
        result.push('\n');
    }
}

pub fn preprocess_pair(
    vert: &str,
    frag: &str,
    headers: &BTreeMap<String, String>,
    defines: &BTreeMap<String, String>,
) -> (String, String, EffectLayout) {
    // Merge source-level #define macros into the defines map.
    // Combo values (from defines) take priority over source macros.
    let mut merged_defines = BTreeMap::new();
    // Collect macros from shader sources only (headers are already emitted as
    // #define directives in the output; we only need shader-level macros for
    // resolving conditional expressions like SHAPE == BOTTOM).
    for (name, value) in collect_source_defines(vert) {
        merged_defines.entry(name).or_insert(value);
    }
    for (name, value) in collect_source_defines(frag) {
        merged_defines.entry(name).or_insert(value);
    }
    // Override with combo/pass defines (higher priority)
    for (k, v) in defines {
        merged_defines.insert(k.clone(), v.clone());
    }

    let layout = collect_layout(vert, frag, headers);
    let (mut vert_out, vert_emitted) =
        preprocess_with_layout_tracked(vert, ShaderStage::Vertex, &layout, headers, &merged_defines);
    let (mut frag_out, _) = preprocess_with_layout_tracked(frag, ShaderStage::Fragment, &layout, headers, &merged_defines);

    // Fix fragment shader varying writes: Vulkan GLSL `in` variables are read-only.
    // If a varying is assigned to in the fragment shader, rename the input and
    // add a local variable copy at the top of main().
    frag_out = fix_fragment_varying_writes(&frag_out, &layout);

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
///
/// For varyings that cannot be found in the preprocessed output at all
/// (e.g. declared in a header that was entirely excluded by `#if`),
/// synthesizes a top-level `out` declaration with the correct type and location.
fn hoist_conditional_varyings(output: &str, layout: &EffectLayout, missing: &[&String]) -> String {
    let mut result = String::with_capacity(output.len() + 512);
    let mut if_depth: u32 = 0;
    let mut hoisted_decls: Vec<String> = Vec::new();
    let mut hoisted_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Track ALL varying names that appear anywhere in the output,
    // including top-level ones from included headers that weren't
    // in `emitted_varyings`.
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in output.lines() {
        let trimmed = line.trim();

        // Track #if depth
        if trimmed.starts_with("#if") {
            if_depth += 1;
        } else if trimmed.starts_with("#endif") {
            if_depth = if_depth.saturating_sub(1);
        }

        // Track all varying declarations (top-level and conditional).
        if trimmed.starts_with("layout(") && trimmed.contains(") out ") {
            if let Some(n) = extract_pp_varying_name(trimmed) {
                seen_names.insert(n);
            }
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

    // Synthesize declarations for any missing varyings that weren't found
    // in the preprocessed output (e.g. from excluded headers).
    for var_name in missing {
        if !hoisted_names.contains(var_name.as_str())
            && !seen_names.contains(var_name.as_str())
        {
            let loc = layout
                .varying_locations
                .get(var_name.as_str())
                .copied()
                .unwrap_or(0);
            let ty = layout
                .varying_types
                .get(var_name.as_str())
                .map(|s: &String| s.as_str())
                .unwrap_or("vec4");
            let synth = format!("layout(location={}) out {} {};", loc, ty, var_name);
            hoisted_names.insert(var_name.to_string());
            hoisted_decls.push(synth);
        }
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
        insert_at_main(&mut result, &format!("{} = {}(0.0);", var_name, ty));
    }

    result
}

/// Fix fragment shader code that writes to varying inputs.
/// Vulkan GLSL does not allow writing to `in` variables.
fn fix_fragment_varying_writes(output: &str, layout: &EffectLayout) -> String {
    let mut written_varyings: Vec<String> = Vec::new();
    for var_name in layout.varying_locations.keys() {
        if output.contains(&format!("{} =", var_name))
            || output.contains(&format!("{}.x =", var_name))
            || output.contains(&format!("{}.y =", var_name))
            || output.contains(&format!("{}.z =", var_name))
            || output.contains(&format!("{}.w =", var_name))
            || output.contains(&format!("{}.xy =", var_name))
            || output.contains(&format!("{}.xyz =", var_name))
            || output.contains(&format!("{}.xyzw =", var_name))
        {
            written_varyings.push(var_name.clone());
        }
    }
    if written_varyings.is_empty() {
        return output.to_string();
    }

    let mut result = output.to_string();
    for var_name in &written_varyings {
        let ty = layout.varying_types.get(var_name.as_str()).map(|s| s.as_str()).unwrap_or("vec4");
        let in_name = format!("_in_{}", var_name);
        // Rename the declaration: `in TYPE VARNAME;` → `in TYPE _IN_VARNAME;`
        result = result.replace(&format!(" in {} {};", ty, var_name), &format!(" in {} {};", ty, in_name));
        // Insert local copy at top of main()
        insert_at_main(&mut result, &format!("{} {} = {};", ty, var_name, in_name));
    }
    result
}

/// Insert a statement after the opening brace of main().
fn insert_at_main(output: &mut String, stmt: &str) {
    if let Some(main_pos) = output.find("void main()") {
        if let Some(brace_pos) = output[main_pos..].find('{') {
            output.insert_str(main_pos + brace_pos + 1, &format!("\n    {}", stmt));
        }
    }
}

/// Extract the variable name from a preprocessed varying line like
/// "layout(location=1) out vec2 v_TexCoordMask;"
fn extract_pp_varying_name(line: &str) -> Option<String> {
    // Split on ") out " or ") in " to get "TYPE NAME;"
    let after_qualifier = line.split(") out ").nth(1)?;
    // Split on whitespace: "vec2 v_TexCoord[13];" -> ["vec2", "v_TexCoord[13];"]
    let parts: Vec<&str> = after_qualifier.split_whitespace().collect();
    if parts.len() >= 2 {
        // Strip array brackets: v_TexCoord[13] → v_TexCoord
        let name = parts[1]
            .trim_end_matches(';')
            .split('[')
            .next()
            .unwrap_or("")
            .to_string();
        Some(name)
    } else {
        None
    }
}

/// Find the insertion point after #version and #define headers (for hoisted declarations).
fn find_decl_insertion_point(output: &str) -> usize {
    output.lines()
        .take_while(|l| {
            let t = l.trim();
            t.starts_with("#version") || t.starts_with("#define")
        })
        .map(|l| l.len() + 1)
        .sum()
}
