mod layout;
mod replace;

use std::collections::{BTreeMap, HashSet};

use wgpu::naga::ShaderStage;

pub use layout::EffectLayout;
pub use layout::collect_layout;

// Re-export WM_SAMPLER_BINDING from shader_header for convenience
pub use super::shader_header::WM_SAMPLER_BINDING;

pub fn preprocess_with_layout(
    source: &str,
    stage: ShaderStage,
    layout: &EffectLayout,
    headers: &BTreeMap<String, String>,
    defines: &BTreeMap<String, String>,
) -> String {
    let (result, _) = preprocess_with_layout_tracked(source, stage, layout, headers, defines);
    result
}

/// Evaluate a preprocessor condition like `BLENDMODE == 26` or `defined(MACRO)`.
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
    if let Some(eq_pos) = cond.find("==") {
        let name = cond[..eq_pos].trim();
        let value = cond[eq_pos + 2..].trim();
        let def = defines.get(name).map(|s| s.as_str()).unwrap_or("0");
        return def == value;
    }
    if let Some(ne_pos) = cond.find("!=") {
        let name = cond[..ne_pos].trim();
        let value = cond[ne_pos + 2..].trim();
        let def = defines.get(name).map(|s| s.as_str()).unwrap_or("0");
        return def != value;
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
    #[allow(dead_code)]
    depth: u32,
}

impl IfBlockProcessor {
    fn new() -> Self {
        Self { stack: Vec::new(), depth: 0 }
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
            self.depth += 1;
            return Some(true);
        }
        if trimmed.starts_with("#ifndef") {
            let macro_name = trimmed["#ifndef".len()..].trim();
            let cond_true = defines.contains_key(macro_name);
            self.stack.push(if !cond_true { IfBlockState::Active } else { IfBlockState::Inactive });
            self.depth += 1;
            return Some(true);
        }
        if trimmed.starts_with("#if")
            && !trimmed.starts_with("#ifdef")
            && !trimmed.starts_with("#ifndef")
        {
            let cond = trimmed["#if".len()..].trim();
            let cond_true = eval_if_condition(cond, defines);
            self.stack.push(if cond_true { IfBlockState::Active } else { IfBlockState::Inactive });
            self.depth += 1;
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
            if trimmed.len() == 5 {
                // plain `#else`
                if let Some(top) = self.stack.last_mut() {
                    match top {
                        IfBlockState::Active => *top = IfBlockState::Done,
                        IfBlockState::Inactive => *top = IfBlockState::Active,
                        IfBlockState::Done => {}
                    }
                }
                return Some(true);
            }
            // `#else` with trailing content — let the caller decide
            return None;
        }

        if trimmed == "#endif" {
            self.stack.pop();
            self.depth = self.depth.saturating_sub(1);
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

        // --- #include handling (header inlining with #if evaluation) ---
        if trimmed.starts_with("#include") {
            if !ifp.is_active() {
                continue;
            }
            if let Some(start) = trimmed.find('"') {
                if let Some(end) = trimmed[start + 1..].find('"') {
                    let include_file = &trimmed[start + 1..start + 1 + end];
                    if let Some(header_content) = headers.get(include_file) {
                        include_header_lines(
                            header_content,
                            defines,
                            &sampler_set,
                            &mut result,
                            headers,
                        );
                        continue;
                    }
                }
            }
            continue;
        }

        // --- Preprocessor directive handling with #if evaluation ---
        if trimmed.starts_with('#') {
            // Track whether we are in an active block for varying-hoisting purposes
            let was_active_before = ifp.is_active();

            // Handle conditional directives via the shared processor
            if ifp.process_line(line, defines) == Some(true) {
                continue;
            }
            // `#else` with trailing content or other directives — fall through

            // Other preprocessor directives: only emit if in active block
            if !was_active_before && !ifp.is_active() {
                continue;
            }

            // Skip duplicate #define of standard constants (already emitted by emit_declarations)
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

        // Skip lines inside inactive #if blocks
        if !ifp.is_active() {
            // Still need to push whitespace to keep line numbering
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
            // Track varyings that are emitted in the output.
            // Note: #if/#endif lines are stripped from the output, so
            // varyings inside active #if blocks appear unconditional to
            // wgpu. Only skip varyings inside INACTIVE #if blocks.
            if stage == ShaderStage::Vertex {
                if let Some(n) = name {
                    let active = ifp.is_active();
                    if active {
                        emitted_varyings.push(n);
                    }
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

fn emit_declarations(
    result: &mut String,
    stage: ShaderStage,
    layout: &EffectLayout,
    headers: &BTreeMap<String, String>,
) {
    // Build a set of header names that are actually reachable from the
    // shader (transitively, via #include) so we don't leak #define macros
    // from unreferenced headers.
    let reachable = collect_reachable_headers(headers);

    // Only emit #define lines that are at the top level (not inside #if blocks)
    // from reachable headers. This prevents leaking macros that are inside
    // inactive #ifdef blocks (e.g. SHADOW_ATLAS_SAMPLER).
    let mut if_depth: u32 = 0;
    for (name, content) in headers {
        if !reachable.contains(name.as_str()) {
            continue;
        }
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("#include") {
                continue;
            }
            // Track #if depth to skip defines inside conditional blocks
            if trimmed == "#ifdef" || trimmed == "#ifndef" || trimmed.starts_with("#if") {
                if_depth += 1;
                continue;
            }
            if trimmed == "#endif" {
                if_depth = if_depth.saturating_sub(1);
                continue;
            }
            if trimmed == "#else" || trimmed.starts_with("#elif") {
                continue;
            }
            if if_depth > 0 {
                continue;
            }
            if trimmed.starts_with('#') {
                // Apply CAST transformations to #define macros
                let mut transformed = trimmed.to_string();
                transformed = transformed.replace("CAST2(", "vec2(");
                transformed = transformed.replace("CAST3(", "vec3(");
                transformed = transformed.replace("CAST4(", "vec4(");
                transformed = transformed.replace("CAST3X3(", "mat3(");
                transformed = replace::replace_saturate(&transformed);
                transformed = replace::replace_frac(&transformed);
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

/// Collect the set of header names that are transitively reachable via
/// `#include` directives found in any header. Used to avoid emitting
/// `#define` macros from headers that the shader never includes.
fn collect_reachable_headers(headers: &BTreeMap<String, String>) -> HashSet<String> {
    let mut reachable: HashSet<String> = HashSet::new();
    let mut to_visit: Vec<String> = headers.keys().cloned().collect();

    // Simple iterative DFS: any header in the map is considered potentially
    // reachable since we can't know which ones the shader actually includes
    // at this point (we emit declarations before processing the main body).
    // We include all headers that aren't "orphaned" — i.e. we include all
    // known headers since they're all loaded by shader_header.rs.
    while let Some(name) = to_visit.pop() {
        if !reachable.insert(name.clone()) {
            continue;
        }
        if let Some(content) = headers.get(&name) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("#include") {
                    if let Some(start) = trimmed.find('"') {
                        if let Some(end) = trimmed[start + 1..].find('"') {
                            let include_file = &trimmed[start + 1..start + 1 + end];
                            if headers.contains_key(include_file)
                                && !reachable.contains(include_file)
                            {
                                to_visit.push(include_file.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    reachable
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
/// Inline a header's content into `result`, evaluating `#if`/`#ifdef` blocks
/// using the given `defines` so only active branches are emitted.
fn include_header_lines(
    content: &str,
    defines: &BTreeMap<String, String>,
    sampler_set: &HashSet<&str>,
    result: &mut String,
    headers: &BTreeMap<String, String>,
) {
    include_header_lines_impl(content, defines, sampler_set, result, headers);
}

/// Recursively expand a header's content, including nested `#include` directives.
fn include_header_lines_impl(
    content: &str,
    defines: &BTreeMap<String, String>,
    sampler_set: &HashSet<&str>,
    result: &mut String,
    headers: &BTreeMap<String, String>,
) {
    let mut ifp = IfBlockProcessor::new();
    // Tracks whether a `return` was emitted at the current brace depth.
    // When true, subsequent lines at the same depth are dead code.
    let mut seen_return: Vec<bool> = Vec::new();
    // Brace depth inside the function body (0 = global, 1 = inside a function, etc.)
    let mut brace_depth: usize = 0;

    for hline in content.lines() {
        let htrim = hline.trim();

        // Track brace depth for all lines (including inactive #if blocks)
        for ch in htrim.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    if brace_depth > 0 {
                        brace_depth -= 1;
                        if seen_return.len() > brace_depth {
                            seen_return.truncate(brace_depth);
                        }
                    }
                }
                _ => {}
            }
        }

        // Handle nested #include (recursively expand)
        if htrim.starts_with("#include") {
            if ifp.is_active() {
                if let Some(start) = htrim.find('"') {
                    if let Some(end) = htrim[start + 1..].find('"') {
                        let include_file = &htrim[start + 1..start + 1 + end];
                        if let Some(nested_content) = headers.get(include_file) {
                            include_header_lines_impl(
                                nested_content,
                                defines,
                                sampler_set,
                                result,
                                headers,
                            );
                        }
                    }
                }
            }
            continue;
        }

        // Handle all preprocessor directives via the shared processor
        if htrim.starts_with('#') {
            if ifp.process_line(hline, defines) == Some(true) {
                continue;
            }
            // Non-conditional # line (#define etc.) — skip, emitted by emit_declarations
            continue;
        }

        // Skip comment and empty lines
        if htrim.is_empty() || htrim.starts_with("//") {
            continue;
        }

        // Only emit lines from active blocks
        if !ifp.is_active() {
            continue;
        }

        // Skip dead code after a return at the same brace depth
        if brace_depth > 0 && seen_return.get(brace_depth).copied().unwrap_or(false) {
            continue;
        }
        let is_return_stmt = htrim.starts_with("return") && htrim.trim_end().ends_with(';');
        if is_return_stmt {
            while seen_return.len() <= brace_depth {
                seen_return.push(false);
            }
            seen_return[brace_depth] = true;
        }

        // Apply transformations (same as main body)
        // Skip uniform/sampler/varying declarations — already emitted
        // by emit_declarations (samplers, EffectParams block).
        let htrim_lower = htrim.to_lowercase();
        if htrim_lower.starts_with("uniform ")
            || htrim_lower.starts_with("sampler2d ")
            || htrim_lower.starts_with("varying ")
            || htrim_lower.starts_with("attribute ")
        {
            continue;
        }

        let mut transformed = htrim.to_string();
        transformed = transformed.replace("CAST2(", "vec2(");
        transformed = transformed.replace("CAST3(", "vec3(");
        transformed = transformed.replace("CAST4(", "vec4(");
        transformed = transformed.replace("CAST3X3(", "mat3(");
        transformed = replace::replace_saturate(&transformed);
        transformed = replace::replace_frac(&transformed);
        transformed = transformed.replace("texSample2D(", "texture(");
        transformed = transformed.replace("texSample2DLod(", "textureLod(");
        transformed = replace::replace_mul(&transformed);
        transformed = replace::replace_texture_calls(&transformed, sampler_set);
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
    let layout = collect_layout(vert, frag, headers);
    let (mut vert_out, vert_emitted) =
        preprocess_with_layout_tracked(vert, ShaderStage::Vertex, &layout, headers, defines);
    let frag_out = preprocess_with_layout(frag, ShaderStage::Fragment, &layout, headers, defines);

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
        result = add_varying_init(&result, var_name, ty);
    }

    result
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
