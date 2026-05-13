use std::collections::{BTreeMap, HashSet};

pub fn replace_texture_calls(line: &str, sampler_set: &HashSet<&str>) -> String {
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

pub fn fix_implicit_truncation(line: &str, varying_types: &BTreeMap<String, String>) -> String {
    // Only handle simple assignments: `varying = expression;`
    if !line.ends_with(';') || line.contains(|c: char| matches!(c, '*' | '+' | '-')) {
        return line.to_string();
    }
    let Some((lhs, rhs)) = line.trim_end_matches(';').split_once('=') else {
        return line.to_string();
    };
    let (lhs, rhs) = (lhs.trim(), rhs.trim());
    if lhs.contains('.') {
        return line.to_string();
    }
    let (Some(l_ty), Some(r_ty)) = (varying_types.get(lhs), varying_types.get(rhs.split('.').next().unwrap_or(rhs))) else {
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

pub fn replace_mul(line: &str) -> String {
    if !line.contains("mul(") {
        return line.to_string();
    }

    let mut result = line.to_string();
    let mut search_start = 0;

    while let Some(mul_start) = result[search_start..].find("mul(") {
        let abs_start = search_start + mul_start;
        let args_start = abs_start + 4;

        let Some(args_end) = find_matching_paren(&result, args_start) else {
            break;
        };

        let args = &result[args_start..args_end];

        if let Some(comma_pos) = find_top_level_comma(args) {
            let arg1 = args[..comma_pos].trim();
            let arg2 = args[comma_pos + 1..].trim();
            let mut replacement = format!("{} * {}", arg2, arg1);
            if result.as_bytes().get(args_end + 1) == Some(&b'.') {
                replacement = format!("({})", replacement);
            }
            result.replace_range(abs_start..args_end + 1, &replacement);
            search_start = abs_start + replacement.len();
        } else {
            search_start = abs_start + 1;
        }
    }

    result
}

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
/// `sample` and `packed` are reserved GLSL keywords that may appear
/// as variable names in Wallpaper Engine shaders.
pub fn replace_reserved_identifiers(line: &str) -> String {
    let mut result = replace_keyword_identifier(&line, "sample", "sampleColor");
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
                .map_or(false, |c| c.is_alphanumeric() || c == '_');

        // Check following character: must be non-alphanumeric/non-underscore
        let followed_by_word = result[abs_end..]
            .chars()
            .next()
            .map_or(false, |c| c.is_alphanumeric() || c == '_');

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
        let has_comparison = inside.contains('<') || inside.contains('>')
            || inside.contains("==") || inside.contains("!=")
            || inside.contains("<=") || inside.contains(">=");

        if !has_comparison {
            search_start = close_pos + 1;
            continue;
        }

        // Skip if this is a function call like float(…), clamp(…), if(…), etc.
        if open_pos > 0 {
            let before = result[..open_pos].trim_end();
            if let Some(last_char) = before.chars().last() {
                if last_char.is_alphanumeric() || last_char == '_' {
                    search_start = close_pos + 1;
                    continue;
                }
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
        if before.ends_with(')') {
            if let Some(open_pos) = find_matching_open_paren(&result, before.len() - 1) {
                let inside = &result[open_pos + 1..before.len() - 1];
                // If inside contains comparison operators, it's already a bool — skip
                if inside.contains('<') || inside.contains('>')
                    || inside.contains("==") || inside.contains("!=")
                    || inside.contains("<=") || inside.contains(">=")
                    || inside.contains("&&") || inside.contains("||")
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
        }

        // Check if the condition is a simple identifier (potential float variable)
        if let Some(last_word) = before.rsplit(|c: char| !c.is_alphanumeric() && c != '_').next() {
            if !last_word.is_empty()
                && !last_word.starts_with(|c: char| c.is_ascii_digit())
                && last_word != "true"
                && last_word != "false"
            {
                // Check this is a standalone identifier (not preceded by a comparison op)
                let before_word = before[..before.len() - last_word.len()].trim_end();
                let preceded_by_cmp = before_word.ends_with('<') || before_word.ends_with('>')
                    || before_word.ends_with("==") || before_word.ends_with("!=")
                    || before_word.ends_with("<=") || before_word.ends_with(">=")
                    || before_word.ends_with("&&") || before_word.ends_with("||");

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
}
