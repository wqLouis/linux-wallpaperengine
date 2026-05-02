use std::collections::{BTreeMap, HashSet};

pub fn replace_texture_calls(line: &str, sampler_set: &HashSet<&str>) -> String {
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

pub fn fix_implicit_truncation(line: &str, varying_types: &BTreeMap<String, String>) -> String {
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

    if lhs.contains('.') {
        return line.to_string();
    }

    let rhs_base = rhs.split('.').next().unwrap_or(rhs).trim();
    let rhs_swizzled = rhs.contains('.');

    match (varying_types.get(lhs_base), varying_types.get(rhs_base)) {
        (Some(l), Some(r)) if l != r && !rhs_swizzled => {
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

pub fn replace_mul(line: &str) -> String {
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

pub fn replace_saturate(line: &str) -> String {
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

pub fn replace_frac(line: &str) -> String {
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

pub fn replace_atan2(line: &str) -> String {
    if !line.contains("atan2(") {
        line.to_string()
    } else {
        line.replace("atan2(", "atan(")
    }
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
