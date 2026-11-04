//! The GLSL compatibility stage of the shader preprocessor.
//!
//! Wallpaper Engine's shaders are written in a dialect that mixes GLSL with
//! a few DirectX style helpers.  This stage rewrites the constructs that
//! have no GLSL equivalent into ones that do.  Every rewrite is a plain
//! textual substitution on a single line; none of them looks at the types of
//! the expressions, which is why the type aware stage ([`super::glsl`]) is
//! needed on top of them.
//!
//! The rewrites, in the order they are applied:
//!
//! * `CAST2`, `CAST3`, `CAST4` and `CAST3X3` become the GLSL constructors
//!   `vec2`, `vec3`, `vec4` and `mat3`.  The rewrite itself lives in the
//!   dialect stage ([`super::dialect`]), which also rewrites the casts of
//!   the body of a macro: a macro is expanded by the shader compiler, so
//!   a cast inside of one has to be rewritten as well.
//! * `saturate(x)` becomes `clamp(x, 0.0, 1.0)`.
//! * `frac(x)` becomes `fract(x)`.
//! * `texSample2D(s, uv)` becomes `texture(s, uv)` and `texSample2DLod`
//!   becomes `textureLod`.
//! * `texSample2DBackBuffer(s, uv, res)` becomes `texture(s, uv)`: the
//!   engine samples the back buffer of the effect like an ordinary
//!   texture.
//! * `texLoad2D(s, uv, res)` becomes `texelFetch(s, ivec2(uv * res), 0)`:
//!   the engine loads the texel of a texture without filtering, and the
//!   coordinate of the texel is the coordinate of the texture times its
//!   resolution.
//! * `clip(x)` becomes `if ((x) < 0.0) discard;`: naga does not know the
//!   `clip` of GLSL.
//! * `gl_FragColor` becomes `_fragColor`, the output of the fragment stage.
//! * `mul(a, b)` becomes `a * b`: Wallpaper Engine uses the row vector form
//!   of the product, so the operands keep their order.  The rewrite itself
//!   lives in the dialect stage ([`super::dialect`]), which also handles the
//!   nested calls.
//! * `texture(s, uv)` becomes `texture(s, uv, _wm_sampler)`, because
//!   Wallpaper Engine uses separate sampler objects while Vulkan GLSL binds
//!   the sampler and the texture together.
//! * A varying that is assigned a wider vector is truncated, which the
//!   type aware stage does as well - see the note on
//!   [`fix_implicit_truncation`].
//! * A boolean used in arithmetic becomes `float(condition)`.
//! * A float used as a condition becomes `condition != 0.0`.
//!
//! The body of a shader gets a few extra rewrites that only make sense
//! inside a function: `ddx` and `ddy` become `dFdx` and `dFdy`, `atan2`
//! becomes `atan` and the reserved words `sample` and `packed` are renamed
//! when they are used as identifiers.

use crate::collections::{Map, Set};

/// Apply the compatibility rewrites that apply to every line of shader code.
///
/// This is the set of rewrites that also apply to the declarations the
/// preprocessor emits and to the lines of the built-in headers.
pub fn apply_line(
    line: &str,
    sampler_set: &Set<&str>,
    varying_types: &Map<String, String>,
) -> String {
    let mut t = line.to_string();
    t = super::dialect::rewrite_casts(&t);
    t = replace_saturate(&t);
    t = replace_frac(&t);
    t = t.replace("texSample2D(", "texture(");
    t = t.replace("texSample2DLod(", "textureLod(");
    t = replace_back_buffer_samples(&t);
    t = replace_texture_loads(&t);
    t = replace_clip(&t);
    t = t.replace("gl_FragColor", "_fragColor");
    t = replace_mul(&t);
    t = replace_texture_calls(&t, sampler_set);
    t = fix_implicit_truncation(&t, varying_types);
    t = replace_bool_arithmetic(&t);
    t = replace_float_as_bool(&t);
    t
}

/// Apply the compatibility rewrites that apply to the body of a shader.
///
/// On top of [`apply_line`] this renames the DirectX derivatives and
/// `atan2`, and renames the identifiers that are reserved words in GLSL.
pub fn apply_body_line(
    line: &str,
    sampler_set: &Set<&str>,
    varying_types: &Map<String, String>,
) -> String {
    let mut t = apply_line(line, sampler_set, varying_types);
    t = t.replace("ddx(", "dFdx(");
    t = t.replace("ddy(", "dFdy(");
    t = t.replace("atan2(", "atan(");
    t = replace_reserved_identifiers(&t);
    t
}

/// Rewrite the texture sampling calls so that they use the sampler the
/// preprocessor binds for them.
///
/// `texture(s, uv)` becomes `texture(s, uv, _wm_sampler)`, but only when
/// the first argument really is one of the samplers of the effect.
pub fn replace_texture_calls(line: &str, sampler_set: &Set<&str>) -> String {
    let mut result = line.to_string();

    for func in &["texture(", "textureLod("] {
        let mut search_start = 0;
        while let Some(pos) = result[search_start..].find(*func) {
            let abs_start = search_start + pos;
            let args_start = abs_start + func.len();

            let Some(args_end) = find_matching_paren(&result, args_start) else {
                break;
            };

            let args = &result[args_start..args_end];
            let Some(comma_pos) = find_top_level_comma(args) else {
                search_start = abs_start + 1;
                continue;
            };
            let arg1_end = args_start + comma_pos;

            if arg1_end <= args_start {
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

/// Truncate a vector that is assigned to a narrower varying.
///
/// This is the legacy, per line version of the assignment truncation rule
/// of the type aware stage: it only looks at the varyings of the shader
/// interface and only at assignments of a plain variable.  The type aware
/// stage covers the same case - and a lot more - but it needs to parse the
/// shader, so this one is kept as a safety net.
pub fn fix_implicit_truncation(line: &str, varying_types: &Map<String, String>) -> String {
    // Only handle simple assignments: `varying = expression;`
    if !line.ends_with(';') || line.contains(['*', '+', '-']) {
        return line.to_string();
    }
    let Some((lhs, rhs)) = line.trim_end_matches(';').split_once('=') else {
        return line.to_string();
    };
    let (lhs, rhs) = (lhs.trim(), rhs.trim());
    if lhs.contains('.') {
        return line.to_string();
    }
    let (Some(l_ty), Some(r_ty)) = (
        varying_types.get(lhs),
        varying_types.get(rhs.split('.').next().unwrap_or(rhs)),
    ) else {
        return line.to_string();
    };
    if l_ty == r_ty || rhs.contains('.') {
        return line.to_string();
    }
    let swizzle = match (r_ty.as_str(), l_ty.as_str()) {
        ("vec4", "vec2") | ("vec3", "vec2") => ".xy",
        ("vec4", "vec3") => ".xyz",
        _ => return line.to_string(),
    };
    format!("{} = {}{};", lhs, rhs, swizzle)
}

/// Rewrite the `clip(x)` of GLSL, which naga does not know.
///
/// `clip(x)` ends the fragment when `x` is negative.  The equivalent in
/// naga is a `discard` in a condition, so the call becomes `if ((x) <
/// 0.0) discard;`.
pub fn replace_clip(line: &str) -> String {
    let mut result = line.to_string();
    let mut search_start = 0;

    while let Some(pos) = result[search_start..].find("clip(") {
        let abs_start = search_start + pos;
        // The name must not be the end of a longer name.
        if abs_start > 0 && is_name_byte(result.as_bytes()[abs_start - 1]) {
            search_start = abs_start + 4;
            continue;
        }
        let open = abs_start + 4;
        let Some(close) = find_matching_paren(&result, open + 1) else {
            break;
        };
        let expression = result[open + 1..close].trim().to_string();

        // The statement of the call ends with a semicolon.
        let mut end = close + 1;
        let rest = &result[end..];
        let trimmed = rest.trim_start();
        if trimmed.starts_with(';') {
            end += rest.len() - trimmed.len() + 1;
        }

        let replacement = format!("if (({expression}) < 0.0) discard;");
        result.replace_range(abs_start..end, &replacement);
        search_start = abs_start + replacement.len();
    }

    result
}

/// Rewrite the calls of the engine that sample the back buffer.
///
/// The engine writes `texSample2DBackBuffer(s, uv, res)`: it samples the
/// buffer of the previous pass of the effect.  The resolution is only
/// needed by the engine, which samples the buffer with a mip map, so the
/// call becomes an ordinary sample: `texture(s, uv)`.
pub fn replace_back_buffer_samples(line: &str) -> String {
    rewrite_texture_helper(line, "texSample2DBackBuffer", |arguments| {
        if arguments.len() < 2 {
            return None;
        }
        Some(format!(
            "texture({}, {})",
            arguments[0].trim(),
            arguments[1].trim()
        ))
    })
}

/// Rewrite the calls of the engine that load a texel of a texture.
///
/// The engine writes `texLoad2D(s, uv, res)`: it loads the texel of a
/// texture without filtering it.  The coordinate of the texel is the
/// coordinate of the texture times the resolution of the texture, so the
/// call becomes `texelFetch(s, ivec2(uv * res), 0)`.
pub fn replace_texture_loads(line: &str) -> String {
    rewrite_texture_helper(line, "texLoad2D", |arguments| {
        if arguments.len() < 3 {
            return None;
        }
        Some(format!(
            "texelFetch({}, ivec2({} * {}), 0)",
            arguments[0].trim(),
            arguments[1].trim(),
            arguments[2].trim()
        ))
    })
}

/// Rewrite the calls of a texture helper of the engine in a line.
///
/// The helper is a macro of the engine (`#define texLoad2D(s, u, r) ...`),
/// so the call is a name followed by a parenthesis.  `rewrite` gets the
/// arguments of the call and returns the text that replaces the whole
/// call; when it returns nothing the call is left alone.
fn rewrite_texture_helper(
    line: &str,
    name: &str,
    rewrite: impl Fn(&[&str]) -> Option<String>,
) -> String {
    let mut result = line.to_string();
    let mut search_start = 0;

    while let Some(pos) = result[search_start..].find(name) {
        let abs_start = search_start + pos;
        // The name must not be the end of a longer name.
        if abs_start > 0 && is_name_byte(result.as_bytes()[abs_start - 1]) {
            search_start = abs_start + name.len();
            continue;
        }
        let open = abs_start + name.len();
        if !result[open..].starts_with('(') {
            search_start = open;
            continue;
        }
        let Some(close) = find_matching_paren(&result, open + 1) else {
            break;
        };
        let arguments = split_arguments(&result[open + 1..close]);
        let Some(replacement) = rewrite(&arguments) else {
            search_start = open + 1;
            continue;
        };
        result.replace_range(abs_start..close + 1, &replacement);
        search_start = abs_start + replacement.len();
    }

    result
}

/// Whether the byte is a part of an identifier.
fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The arguments of a call, split at the commas that are not enclosed in
/// brackets.
fn split_arguments(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (index, byte) in text.as_bytes().iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(&text[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// Rewrite the DirectX style `mul(a, b)` into the GLSL `a * b`.
///
/// Wallpaper Engine uses the row vector form of the product: `mul(a, b)`
/// means `a * b`.  The operands are *not* swapped the way the DirectX `mul`
/// would need them: `mul(vec4(position, 1.0), matrix)` is the row vector
/// product, which is the `vec4 * matrix` of GLSL.
///
/// The rewrite is the one of the dialect stage, which also handles the
/// nested calls: `mul(mul(a, b), c)` becomes `(a * b) * c`.
pub fn replace_mul(line: &str) -> String {
    super::dialect::rewrite_mul(line)
}

/// Rewrite `saturate(x)` into `clamp(x, 0.0, 1.0)`.
pub fn replace_saturate(line: &str) -> String {
    if !line.contains("saturate(") {
        return line.to_string();
    }
    let mut result = line.to_string();
    let mut search_start = 0;

    while let Some(sat_start) = result[search_start..].find("saturate(") {
        let abs_start = search_start + sat_start;
        // Only replace standalone saturate(, not Desaturate( / Saturation( etc.
        if abs_start > 0 {
            let prev_byte = result.as_bytes()[abs_start - 1];
            if prev_byte.is_ascii_alphanumeric() || prev_byte == b'_' {
                search_start = abs_start + 8; // skip "saturate"
                continue;
            }
        }
        let args_start = abs_start + 9;

        let Some(args_end) = find_matching_paren(&result, args_start) else {
            break;
        };

        let arg = &result[args_start..args_end].trim();
        let replacement = format!("clamp({}, 0.0, 1.0)", arg);
        result.replace_range(abs_start..args_end + 1, &replacement);
        search_start = abs_start + replacement.len();
    }

    result
}

/// Rewrite `frac(x)` into `fract(x)`.
pub fn replace_frac(line: &str) -> String {
    if !line.contains("frac(") {
        return line.to_string();
    }
    let mut result = line.to_string();
    let mut search_start = 0;
    while let Some(pos) = result[search_start..].find("frac(") {
        let abs = search_start + pos;
        // Only replace standalone `frac(`, not `Desaturate` or already `fract`
        if abs > 0 && result.as_bytes()[abs - 1].is_ascii_alphanumeric() {
            search_start = abs + 1;
            continue;
        }
        result.replace_range(abs..abs + 4, "fract");
        search_start = abs + 5;
    }
    result
}

/// Replace GLSL reserved keywords used as identifiers.
///
/// `sample` and `packed` are reserved GLSL keywords that may appear
/// as variable names in Wallpaper Engine shaders.
pub fn replace_reserved_identifiers(line: &str) -> String {
    let mut result = replace_keyword_identifier(line, "sample", "sampleColor");
    result = replace_keyword_identifier(&result, "packed", "packedValue");
    result
}

/// Replace a single reserved keyword identifier with a safe alternative.
fn replace_keyword_identifier(line: &str, keyword: &str, replacement: &str) -> String {
    if !line.contains(keyword) {
        return line.to_string();
    }
    let mut result = line.to_string();
    let mut search_start = 0;
    let kw_len = keyword.len();
    while let Some(pos) = result[search_start..].find(keyword) {
        let abs_start = search_start + pos;
        let abs_end = abs_start + kw_len;

        // Check preceding character: must be non-alphanumeric/non-underscore
        let preceded_by_word = abs_start > 0
            && result[..abs_start]
                .chars()
                .last()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');

        // Check following character: must be non-alphanumeric/non-underscore
        let followed_by_word = result[abs_end..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');

        if !preceded_by_word && !followed_by_word {
            // Skip if this is a function call like keyword()
            let next_non_space = result[abs_end..].chars().next();
            if next_non_space == Some('(') {
                search_start = abs_start + 1;
                continue;
            }
            result.replace_range(abs_start..abs_end, replacement);
            search_start = abs_start + replacement.len();
        } else {
            search_start = abs_start + 1;
        }
    }
    result
}

/// Wrap boolean parenthesized expressions used in arithmetic with `float()`.
/// Example: `(depth < limit) * 6.0` → `float(depth < limit) * 6.0`
pub fn replace_bool_arithmetic(line: &str) -> String {
    if !line.contains('<') && !line.contains('>') && !line.contains('=') {
        return line.to_string();
    }
    let mut result = line.to_string();
    let mut search_start = 0;

    // Find `)` that is followed (maybe with whitespace) by *, /, +, -
    while let Some(paren_close) = result[search_start..].find(')') {
        let close_pos = search_start + paren_close;
        let after_paren = result[close_pos + 1..].trim_start();

        // Skip if `)` is followed by ternary `?` or `:` — it's a condition, not arithmetic
        if after_paren.starts_with('?') || after_paren.starts_with(':') {
            search_start = close_pos + 1;
            continue;
        }

        // Check if this `)` is followed by an arithmetic operator
        let followed_by_arith = after_paren.starts_with('*')
            || after_paren.starts_with('/')
            || after_paren.starts_with('+')
            || after_paren.starts_with('-');

        let Some(open_pos) = find_matching_open_paren(&result, close_pos) else {
            search_start = close_pos + 1;
            continue;
        };

        // Check if the matching `(` is preceded by an arithmetic/assignment operator
        let preceded_by_arith = if open_pos > 0 {
            let before_paren = result[..open_pos].trim_end();
            before_paren.ends_with('*')
                || before_paren.ends_with('/')
                || before_paren.ends_with('+')
                || before_paren.ends_with('-')
                || before_paren.ends_with('=')
        } else {
            false
        };

        if !followed_by_arith && !preceded_by_arith {
            search_start = close_pos + 1;
            continue;
        }

        // Check if the expression inside contains comparison operators
        let inside = &result[open_pos + 1..close_pos];
        let has_comparison = inside.contains('<')
            || inside.contains('>')
            || inside.contains("==")
            || inside.contains("!=")
            || inside.contains("<=")
            || inside.contains(">=");

        if !has_comparison {
            search_start = close_pos + 1;
            continue;
        }

        // Skip if this is a function call like float(…), clamp(…), if(…), etc.
        if open_pos > 0 {
            let before = result[..open_pos].trim_end();
            if let Some(last_char) = before.chars().last()
                && (last_char.is_alphanumeric() || last_char == '_')
            {
                search_start = close_pos + 1;
                continue;
            }
        }

        // Replace '(' with 'float(' — the existing ')' becomes the closing of float()
        result.replace_range(open_pos..open_pos + 1, "float(");
        search_start = close_pos + 5; // account for extra chars from "float("
    }

    result
}

/// Convert float variables used as boolean conditions in ternary operators.
/// Example: `outside ? a : b` → `outside != 0.0 ? a : b`
pub fn replace_float_as_bool(line: &str) -> String {
    if !line.contains('?') {
        return line.to_string();
    }
    let mut result = line.to_string();
    let mut search_start = 0;

    while let Some(q_pos) = result[search_start..].find('?') {
        let abs_q = search_start + q_pos;

        // Get the text before the `?`, trim trailing whitespace
        let before = result[..abs_q].trim_end();
        if before.is_empty() {
            search_start = abs_q + 1;
            continue;
        }

        // Check if the condition is a parenthesized expression
        if before.ends_with(")")
            && let Some(open_pos) = find_matching_open_paren(&result, before.len() - 1)
        {
            let inside = &result[open_pos + 1..before.len() - 1];
            // If inside contains comparison operators, it's already a bool — skip
            if inside.contains('<')
                || inside.contains('>')
                || inside.contains("==")
                || inside.contains("!=")
                || inside.contains("<=")
                || inside.contains(">=")
                || inside.contains("&&")
                || inside.contains("||")
            {
                search_start = abs_q + 1;
                continue;
            }
            // Check if already wrapped with bool()
            if open_pos >= 5 && &result[open_pos - 5..open_pos] == "bool(" {
                search_start = abs_q + 1;
                continue;
            }
            // Wrap the parenthesized expression with bool()
            result.replace_range(open_pos..open_pos + 1, "bool(");
            search_start = abs_q + 5; // account for extra chars
            continue;
        }

        // Check if the condition is a simple identifier (potential float variable)
        if let Some(last_word) = before
            .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
            .next()
            && !last_word.is_empty()
            && !last_word.starts_with(|c: char| c.is_ascii_digit())
            && last_word != "true"
            && last_word != "false"
        {
            // Check this is a standalone identifier (not preceded by a comparison op)
            let before_word = before[..before.len() - last_word.len()].trim_end();
            let preceded_by_cmp = before_word.ends_with('<')
                || before_word.ends_with('>')
                || before_word.ends_with("==")
                || before_word.ends_with("!=")
                || before_word.ends_with("<=")
                || before_word.ends_with(">=")
                || before_word.ends_with("&&")
                || before_word.ends_with("||");

            if !preceded_by_cmp {
                // Replace `identifier?` with `identifier != 0.0 ?`
                let word_start = before.len() - last_word.len();
                // Only add conversion if it's not already bool()
                if word_start < 5 || &result[word_start - 5..word_start] != "bool(" {
                    let replacement = format!("{} != 0.0 ", last_word);
                    result.replace_range(word_start..abs_q, &replacement);
                    search_start = word_start + replacement.len() + 1;
                    continue;
                }
            }
        }

        search_start = abs_q + 1;
    }

    result
}

/// Find the position of the matching opening parenthesis for a given close paren.
fn find_matching_open_paren(s: &str, close_pos: usize) -> Option<usize> {
    let mut depth = 1;
    for (i, ch) in s[..close_pos].char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Find the position of the matching closing parenthesis, starting from
/// the character immediately after the opening `(`.
fn find_matching_paren(s: &str, start: usize) -> Option<usize> {
    let mut depth = 1;
    for (i, ch) in s[start..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + i);
                }
            }
            _ => {}
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_mul_vec_matrix() {
        // Wallpaper Engine uses the row vector form: the operands keep their
        // order.
        let input = "gl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix);";
        let output = replace_mul(input);
        assert_eq!(
            output,
            "gl_Position = vec4(a_Position, 1.0) * g_ModelViewProjectionMatrix;"
        );
    }

    #[test]
    fn test_replace_nested_mul() {
        let input = "gl_Position = mul(mul(vec4(a_Position, 1.0), g_ModelMatrix), g_ViewMatrix);";
        let output = replace_mul(input);
        assert_eq!(
            output,
            "gl_Position = (vec4(a_Position, 1.0) * g_ModelMatrix) * g_ViewMatrix;"
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

    /// The sample of the back buffer of an effect becomes an ordinary
    /// sample: the resolution is only needed by the engine.
    #[test]
    fn the_back_buffer_sample_is_rewritten() {
        let input = "float d = texSample2DBackBuffer(g_Texture1, uv, g_Texture1Resolution.xy).r;";
        let output = replace_back_buffer_samples(input);
        assert_eq!(output, "float d = texture(g_Texture1, uv).r;");
    }

    /// The load of a texel becomes a fetch: the coordinate of the texel is
    /// the coordinate of the texture times the resolution of the texture.
    #[test]
    fn the_texture_load_is_rewritten() {
        let input = "float d = texLoad2D(g_Texture3, uv, g_Texture3Resolution.xy).r;";
        let output = replace_texture_loads(input);
        assert_eq!(
            output,
            "float d = texelFetch(g_Texture3, ivec2(uv * g_Texture3Resolution.xy), 0).r;"
        );
    }

    /// The `clip` of GLSL is not known by naga: it becomes a `discard` in a
    /// condition.
    #[test]
    fn the_clip_is_rewritten() {
        let input = "\tclip(depth - 0.5);";
        let output = replace_clip(input);
        assert_eq!(output, "\tif ((depth - 0.5) < 0.0) discard;");
    }

    /// A name that only starts with `clip` is not a call of the engine.
    #[test]
    fn a_name_that_is_not_a_clip_is_left_alone() {
        let input = "float a = clipboard + clip;";
        assert_eq!(replace_clip(input), input);
    }
}
