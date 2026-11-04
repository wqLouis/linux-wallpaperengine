//! The shader loader of the scene.
//!
//! A Wallpaper Engine effect is a pair of GLSL shaders - a `.vert` and a
//! `.frag` file - that are written in the dialect of the engine and that use
//! the conditionals of a C preprocessor.  The shaders are preprocessed here
//! and handed to the shader compiler of wgpu as plain GLSL.
//!
//! The preprocessing has four stages:
//!
//! 1. [`preprocessor`] resolves the conditionals of the shader, inlines the
//!    headers it includes and collects the macros of the shader and of its
//!    headers.  The conditions are evaluated by [`c_expr`], which implements
//!    the operators and the precedence of C.
//! 2. [`compat`] rewrites the constructs of the Wallpaper Engine dialect
//!    that have no GLSL equivalent (`mul`, `saturate`, `gl_FragColor`, ...).
//! 3. [`glsl`] normalises the types of the shader, so that it satisfies the
//!    strict typing rules of GLSL.
//! 4. [`dialect`] rewrites the rest of the dialect of the engine: the `mul`
//    calls in their row vector form, the precision qualifiers, the sampler
//!    types and the macros of the engine.
//!
//! The layout of the shader - its uniforms, its samplers and its varyings -
//! is collected with the same preprocessor and the same macros, so that it
//! always describes the branch that is really compiled.

pub mod asset;
mod c_expr;
mod compat;
mod dialect;
#[cfg(test)]
mod engine_assets;
mod glsl;
pub mod header;
mod layout;
mod preprocessor;
mod varying;

use crate::collections::{Map, Set, new_map, new_set};
use serde_json::Value;
use wgpu::naga::ShaderStage;

pub use asset::ShaderProgram;
pub use header::{WM_SAMPLER_BINDING, get_headers};
pub use layout::EffectLayout;
pub use layout::collect_layout;
pub(crate) use layout::{align_up, type_align, type_size};

/// Collect `// [COMBO] {"combo":"NAME","default":N}` annotations from a
/// shader source.
pub fn collect_default_defines(vert_source: &str, frag_source: &str) -> Map<String, String> {
    let mut defines = new_map();

    for source in &[vert_source, frag_source] {
        for line in source.lines() {
            let trimmed = line.trim();
            if let Some(combo_start) = trimmed.find("[COMBO]") {
                let json_str = trimmed[combo_start + 7..].trim();
                let Ok(combo) = serde_json::from_str::<Value>(json_str) else {
                    continue;
                };
                if let (Some(combo_key), Some(default_val)) = (
                    combo.get("combo").and_then(|v: &Value| v.as_str()),
                    combo.get("default"),
                ) {
                    let default_str = match default_val {
                        Value::Number(n) => n.to_string(),
                        Value::String(s) => s.clone(),
                        Value::Bool(b) => (*b as i32).to_string(),
                        _ => continue,
                    };
                    defines.entry(combo_key.to_string()).or_insert(default_str);
                }
            }
        }
    }

    defines
}

/// Collect the `#define` macros of a shader source.
///
/// Only the macros of the branches that are taken are collected: a macro
/// that is defined in a branch that is not taken does not exist.  The
/// branches are evaluated with the macros that were defined before them, so
/// a macro that depends on a combo of the material is not collected here -
/// the preprocessor defines it when the shader is compiled.
pub fn collect_source_defines(source: &str) -> Map<String, String> {
    let mut macros = new_map();
    for (name, definition) in preprocessor::collect_defines(source) {
        macros.insert(name, definition.body);
    }
    macros
}

/// The macros the layout of a shader is collected with.
///
/// The layout is collected when the scene is loaded, before the material of
/// an effect is known: the default values of the combos of the shader and the
/// macros of the shader itself are the best guess that is available then.
/// The layout is collected again when the shader is compiled, with the macros
/// of the material.
fn layout_defines(source1: &str, source2: &str) -> Map<String, String> {
    let mut defines = collect_default_defines(source1, source2);
    for (name, value) in collect_source_defines(source1) {
        defines.entry(name).or_insert(value);
    }
    for (name, value) in collect_source_defines(source2) {
        defines.entry(name).or_insert(value);
    }
    defines
}

/// Preprocess a shader and also track which varyings were emitted in the output.
///
/// `defines` holds the macros that come from outside the shader - the values
/// of the combos of the material - and they decide which branch of the shader
/// is compiled.
///
/// The name of the shader in the wallpaper is taken from the layout: it is
/// the only way to tell which shader of a wallpaper could not be
/// preprocessed.
pub fn preprocess_with_layout_tracked(
    source: &str,
    stage: ShaderStage,
    layout: &EffectLayout,
    headers: &Map<String, String>,
    defines: &Map<String, String>,
    has_subgroup: bool,
) -> (String, Vec<String>) {
    let flattened = preprocessor::flatten(source, headers, defines);
    report_warnings(&shader_name(layout, stage), &flattened.warnings);
    emit_stage(&flattened, stage, layout, has_subgroup)
}

/// The name of a shader, for the log messages of the preprocessing.
///
/// The path of the shader pair is in the layout; the stage tells which of the
/// two shaders is meant.
fn shader_name(layout: &EffectLayout, stage: ShaderStage) -> String {
    let stage = match stage {
        ShaderStage::Vertex => "the vertex stage",
        ShaderStage::Fragment => "the fragment stage",
        _ => "the stage",
    };
    if layout.shader_path.is_empty() {
        format!("the shader ({stage})")
    } else {
        format!("{} ({stage})", layout.shader_path)
    }
}

/// Report the warnings of the preprocessing.
///
/// A header that cannot be resolved, a stray `#endif` or a conditional that
/// is never closed does not stop the shader from being compiled: the
/// reference engine is lenient there too.  The warning names the shader, so
/// that a shader that is not the one the author wrote can be found.
fn report_warnings(name: &str, warnings: &[String]) {
    for warning in warnings {
        log::warn!(
            "the shader `{}` was preprocessed with a warning: {}",
            name,
            warning
        );
    }
}

/// Emit the GLSL of a stage from the flattened source of the shader.
fn emit_stage(
    flattened: &preprocessor::Flattened,
    stage: ShaderStage,
    layout: &EffectLayout,
    has_subgroup: bool,
) -> (String, Vec<String>) {
    let mut result = String::with_capacity(4096);
    let mut emitted_varyings: Vec<String> = Vec::new();

    result.push_str("#version 450\n");

    // Add subgroup extension when available (Vulkan only)
    if has_subgroup {
        result.push_str("#extension GL_KHR_shader_subgroup_arithmetic : enable\n");
        result.push_str("#extension GL_KHR_shader_subgroup_basic : enable\n");
        result.push_str("#extension GL_KHR_shader_subgroup_vote : enable\n");
        result.push_str("#extension GL_KHR_shader_subgroup_ballot : enable\n");
        result.push_str("#extension GL_KHR_shader_subgroup_shuffle : enable\n");
    }

    emit_declarations(&mut result, stage, layout);

    let sampler_set: Set<&str> = layout.sampler_names.iter().map(|s| s.as_str()).collect();

    for line in &flattened.lines {
        let trimmed = line.text.trim();

        if trimmed.is_empty() {
            result.push('\n');
            continue;
        }

        // A directive that survived the preprocessing: a `#define` of the
        // shader or of one of its headers, an `#extension`, ...
        if trimmed.starts_with('#') {
            result.push_str(trimmed);
            result.push('\n');
            continue;
        }

        if trimmed.starts_with("//") {
            result.push_str(trimmed);
            result.push('\n');
            continue;
        }

        if line.from_header {
            // The declarations of a header are part of the interface of the
            // shader: they are emitted from the layout, not from here.
            let lower = trimmed.to_lowercase();
            if lower.starts_with("uniform ")
                || lower.starts_with("sampler2d ")
                || lower.starts_with("varying ")
                || lower.starts_with("attribute ")
            {
                continue;
            }

            let transformed = compat::apply_line(trimmed, &sampler_set, &new_map());
            result.push_str(&transformed);
            result.push('\n');
            continue;
        }

        let cleaned = strip_material_comments(&line.text);
        let declaration = cleaned.trim();
        if declaration.is_empty() {
            result.push('\n');
            continue;
        }

        if declaration.starts_with("uniform ") || declaration.starts_with("sampler2D ") {
            continue;
        }

        if declaration.contains("attribute ") {
            let rest = declaration.split("attribute ").nth(1).unwrap_or("").trim();
            let loc = layout::extract_variable_name(rest)
                .and_then(|n| layout.attribute_locations.get(&n).copied())
                .unwrap_or(0);
            result.push_str(&format!("layout(location={}) in {}\n", loc, rest));
            continue;
        }

        if let Some(rest) = declaration.strip_prefix("varying ") {
            let rest = rest.trim();
            let keyword = if matches!(stage, ShaderStage::Vertex) {
                "out"
            } else {
                "in"
            };
            let name = layout::extract_variable_name(rest);

            // The varying is declared, whatever the vertex shader does with
            // it: a varying the vertex shader does not produce is declared
            // by the vertex shader itself, see the synthesis of the missing
            // varyings.
            let location = name
                .as_ref()
                .and_then(|n| layout.varying_locations.get(n))
                .copied()
                .unwrap_or(0);
            result.push_str(&format!(
                "layout(location={}) {} {}\n",
                location, keyword, rest
            ));

            // Track the varyings the vertex shader emits: the ones it does
            // not emit have to be synthesised.
            if stage == ShaderStage::Vertex
                && let Some(n) = name
            {
                emitted_varyings.push(n);
            }
            continue;
        }

        let transformed = compat::apply_body_line(&cleaned, &sampler_set, &layout.varying_types);

        result.push_str(&transformed);
        result.push('\n');
    }

    // The type aware stage: the shader is normalised so that it satisfies
    // the strict typing rules of GLSL.  Wallpaper Engine's own compiler is
    // lenient about vectors of a different width, naga is not.
    let result = glsl::normalise(&result, layout);

    let result = apply_dialect(result);

    (result, emitted_varyings)
}

/// The stage that rewrites the dialect of the shader.
///
/// The dialect module (`src/scene/loader/shader/dialect.rs`) rewrites the
/// constructs of the Wallpaper Engine dialect that the type aware stage
/// does not know about: the `mul` calls (in the row vector form the engine
/// uses), the precision qualifiers, the sampler types the engine has no
/// binding for, the macros the engine provides and the scalar arguments of
/// the built-in calls that want a vector.
///
/// It runs after the type aware stage, on the whole source: it works on the
/// text and needs neither the layout nor the headers of the engine.
fn apply_dialect(source: String) -> String {
    dialect::normalize(&source)
}

/// Emit the declarations of the interface of the shader.
fn emit_declarations(result: &mut String, stage: ShaderStage, layout: &EffectLayout) {
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
        // Push constants: data supplied via set_immediates(), no buffer binding.
        // Naga GLSL frontend expects `layout(push_constant)` for immediate data.
        if layout.use_immediates {
            result.push_str("layout(push_constant) uniform EffectParams {\n");
        } else {
            result.push_str(&format!(
                "layout(binding={}, std140) uniform EffectParams {{\n",
                layout.uniform_binding
            ));
        }
        for (name, ty) in &layout.uniform_decls {
            result.push_str(&format!("    {}\n", layout::uniform_declaration(ty, name)));
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
    if before.is_empty() {
        String::new()
    } else {
        before.to_string()
    }
}

/// Preprocess a vertex/fragment shader pair using a layout that was already
/// collected when the scene was loaded.
///
/// `defines` must already contain both the shader source macros and the
/// runtime combo/pass defines; callers obtain the source macros from
/// [`asset::ShaderProgram::source_defines`].
///
/// The layout of the shader is collected again with the macros of this
/// compilation, so that it describes the branch that is really compiled: a
/// uniform that is only declared in a branch that is not taken must not be in
/// the layout, and a uniform of the branch that is taken has to be.  The
/// layout that was collected when the scene was loaded is only used to report
/// a change of the uniforms.
///
/// The path of the shaders is in the layout: it is the only way to tell which
/// shader of a wallpaper a warning or a failure belongs to.
pub fn preprocess_pair_with_layout(
    vert: &str,
    frag: &str,
    headers: &Map<String, String>,
    defines: &Map<String, String>,
    collected: &EffectLayout,
    has_subgroup: bool,
    use_immediates: bool,
) -> (String, String, EffectLayout) {
    // The layout is collected with the macros of this compilation, so that
    // it describes the branch that is really compiled: a uniform that is
    // only declared in a branch that is not taken must not be in the layout,
    // and a uniform of the branch that is taken has to be.
    let mut layout = layout::collect_layout_with_defines(vert, frag, headers, defines);
    // The path of the shaders is in the layout: the warnings of the
    // preprocessing name the shader they are about.
    layout.shader_path = collected.shader_path.clone();
    if layout.uniform_decls != collected.uniform_decls {
        // The layout of the scene was collected with the default values of
        // the combos; the material of the effect can enable other branches
        // of the shader.  It is not an error - the layout that is compiled
        // is the one that counts - but it is worth knowing.
        log::info!(
            "the uniforms of the shader `{}` changed with the macros of the material: {:?} -> {:?}",
            layout.shader_path,
            collected.uniform_decls,
            layout.uniform_decls
        );
    }
    layout.use_immediates = use_immediates;

    let (mut vert_out, vert_emitted) = preprocess_with_layout_tracked(
        vert,
        ShaderStage::Vertex,
        &layout,
        headers,
        defines,
        has_subgroup,
    );
    let (mut frag_out, _) = preprocess_with_layout_tracked(
        frag,
        ShaderStage::Fragment,
        &layout,
        headers,
        defines,
        has_subgroup,
    );

    // Fix fragment shader varying writes: Vulkan GLSL `in` variables are read-only.
    // If a varying is assigned to in the fragment shader, rename the input and
    // add a local variable copy at the top of main().
    frag_out = varying::fix_fragment_writes(&frag_out, &layout);

    // The vertex shader has to produce every varying the fragment shader
    // reads: wgpu rejects a program whose fragment inputs have no vertex
    // output.  A varying the vertex shader declares in a header is not
    // emitted either - the declarations of a header are the interface of
    // the shader, they are emitted from the layout.
    let missing: Vec<&String> = layout
        .vertex_varyings
        .iter()
        .chain(layout.fragment_varyings.iter())
        .filter(|v| !vert_emitted.iter().any(|e| e == *v))
        .collect();

    if !missing.is_empty() {
        vert_out = synthesise_missing_varyings(&vert_out, &layout, &missing);
    }

    (vert_out, frag_out, layout)
}

/// Declare the varyings the vertex shader does not produce.
///
/// The varyings that are missing are declared at the top of the vertex
/// shader and initialised with zero, so that the shader compiles: the
/// fragment shader reads them, and a program whose fragment inputs have no
/// vertex output is rejected by wgpu.
fn synthesise_missing_varyings(
    output: &str,
    layout: &EffectLayout,
    missing: &[&String],
) -> String {
    if missing.is_empty() {
        return output.to_string();
    }

    // The varyings the vertex shader declares itself.
    let mut declared: Set<String> = new_set();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("layout(")
            && trimmed.contains(") out ")
            && let Some(name) = extract_pp_varying_name(trimmed)
        {
            declared.insert(name);
        }
    }

    // The declarations of the varyings that are missing, and the way they
    // are initialised.
    let mut declarations: Vec<String> = Vec::new();
    let mut initialisations: Vec<String> = Vec::new();
    for name in missing {
        if declared.contains(name.as_str()) {
            continue;
        }
        declared.insert(name.to_string());

        let location = layout
            .varying_locations
            .get(name.as_str())
            .copied()
            .unwrap_or(0);
        let type_name = layout
            .varying_types
            .get(name.as_str())
            .map(|s: &String| s.as_str())
            .unwrap_or("vec4");
        let size = layout
            .varying_array_sizes
            .get(name.as_str())
            .copied()
            .unwrap_or(1);

        if size > 1 {
            declarations.push(format!(
                "layout(location={location}) out {type_name} {name}[{size}];"
            ));
            initialisations.push(format!(
                "{name} = {type_name}[{size}]({values});",
                values = zero_values(type_name, size)
            ));
        } else {
            declarations.push(format!(
                "layout(location={location}) out {type_name} {name};"
            ));
            initialisations.push(format!("{name} = {type_name}(0.0);"));
        }
    }

    if declarations.is_empty() {
        return output.to_string();
    }

    // The declarations go after the header of the shader: the `#version`,
    // the `#extension` and the `#define` lines have to stay at the top.
    let insertion = find_decl_insertion_point(output);
    let mut result = String::with_capacity(output.len() + 512);
    result.push_str(&output[..insertion]);
    for declaration in &declarations {
        result.push_str(declaration);
        result.push('\n');
    }
    result.push_str(&output[insertion..]);

    // The varyings are initialised with zero: the shader writes to them
    // when it wants to.
    for initialisation in &initialisations {
        varying::insert_at_main(&mut result, initialisation);
    }

    result
}

/// The values of an array of `size` elements of a type, all of them zero.
fn zero_values(type_name: &str, size: u32) -> String {
    let value = format!("{type_name}(0.0)");
    (0..size)
        .map(|_| value.clone())
        .collect::<Vec<String>>()
        .join(", ")
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

/// Find the insertion point of the declarations of a shader.
///
/// The declarations go after the header of the shader: the `#version`, the
/// `#extension` and the `#define` lines have to stay at the top, the
/// directives of GLSL are only allowed before the first declaration.
fn find_decl_insertion_point(output: &str) -> usize {
    output
        .lines()
        .take_while(|l| l.trim().starts_with('#'))
        .map(|l| l.len() + 1)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::new_map;

    /// A shader with a conditional in its body.
    ///
    /// The branch that is taken sets the value to one, the branch that is not
    /// taken sets it to two.
    fn conditional_shader(condition: &str) -> String {
        format!(
            "varying vec2 v_TexCoord;\n\
             \n\
             void main() {{\n\
             \x20   float value = 0.0;\n\
             #if {condition}\n\
             \x20   value = 1.0;\n\
             #else\n\
             \x20   value = 2.0;\n\
             #endif\n\
             \x20   v_TexCoord = vec2(value);\n\
             \x20   gl_Position = vec4(0.0, 0.0, 0.0, 1.0);\n\
             }}\n"
        )
    }

    /// Preprocess a shader with a set of macros.
    fn preprocess(source: &str, macros: &[(&str, &str)]) -> String {
        let headers = new_map();
        let mut defines = new_map();
        for (name, value) in macros {
            defines.insert(name.to_string(), value.to_string());
        }
        let layout = collect_layout(source, source, &headers);
        let (_vertex, fragment, _) =
            preprocess_pair_with_layout(source, source, &headers, &defines, &layout, false, false);
        fragment
    }

    /// A condition of the Wallpaper Engine shaders, with the macros it is
    /// evaluated with and the result the C preprocessor computes.
    struct ConditionCase {
        /// The condition of the directive.
        condition: &'static str,
        /// The macros of the condition.
        macros: &'static [(&'static str, &'static str)],
        /// Whether the condition holds.
        holds: bool,
    }

    /// The conditions of the real Wallpaper Engine shaders.
    const CONDITIONS: &[ConditionCase] = &[
        ConditionCase {
            condition: "A == 1 && B == 2",
            macros: &[("A", "1"), ("B", "2")],
            holds: true,
        },
        ConditionCase {
            condition: "A == 1 || B == 2",
            macros: &[("A", "1"), ("B", "2")],
            holds: true,
        },
        ConditionCase {
            condition: "(A || B) && C",
            macros: &[("A", "1"), ("B", "1"), ("C", "1")],
            holds: true,
        },
        ConditionCase {
            condition: "A && (B || C)",
            macros: &[("A", "1"), ("B", "0"), ("C", "1")],
            holds: true,
        },
        ConditionCase {
            condition: "A >= 1",
            macros: &[("A", "2")],
            holds: true,
        },
        ConditionCase {
            condition: "defined(A) && defined(B)",
            macros: &[("A", "1"), ("B", "1")],
            holds: true,
        },
        ConditionCase {
            condition: "defined(A) || defined(B)",
            macros: &[("A", "1")],
            holds: true,
        },
        ConditionCase {
            condition: "TEX4FORMAT == FORMAT_R8 || TEX4FORMAT == FORMAT_RG88",
            macros: &[
                ("TEX4FORMAT", "8"),
                ("FORMAT_R8", "9"),
                ("FORMAT_RG88", "8"),
            ],
            holds: true,
        },
        ConditionCase {
            condition: "HDR && (LIGHTING || REFLECTION) && EMISSIVE_MAP",
            macros: &[
                ("HDR", "1"),
                ("LIGHTING", "1"),
                ("REFLECTION", "0"),
                ("EMISSIVE_MAP", "1"),
            ],
            holds: true,
        },
        ConditionCase {
            condition: "NORMALMAP == 0",
            macros: &[("NORMALMAP", "0")],
            holds: true,
        },
        ConditionCase {
            condition: "COMPOSITE != 0",
            macros: &[("COMPOSITE", "3")],
            holds: true,
        },
        // The conditions that do not hold.
        ConditionCase {
            condition: "A == 1 && B == 2",
            macros: &[("A", "1"), ("B", "3")],
            holds: false,
        },
        ConditionCase {
            condition: "A >= 1",
            macros: &[("A", "0")],
            holds: false,
        },
        ConditionCase {
            condition: "defined(A) && defined(B)",
            macros: &[("A", "1")],
            holds: false,
        },
        ConditionCase {
            condition: "REFLECTION && NORMALMAP",
            macros: &[("REFLECTION", "1"), ("NORMALMAP", "0")],
            holds: false,
        },
        ConditionCase {
            condition: "TEX1FORMAT >= FORMAT_ETC1_RGB8 && TEX1FORMAT <= FORMAT_DXT1 || TEX1FORMAT == FORMAT_BC7",
            macros: &[
                ("TEX1FORMAT", "1"),
                ("FORMAT_ETC1_RGB8", "3"),
                ("FORMAT_DXT1", "7"),
                ("FORMAT_BC7", "12"),
            ],
            holds: false,
        },
        ConditionCase {
            condition: "TEX1FORMAT >= FORMAT_ETC1_RGB8 && TEX1FORMAT <= FORMAT_DXT1 || TEX1FORMAT == FORMAT_BC7",
            macros: &[
                ("TEX1FORMAT", "12"),
                ("FORMAT_ETC1_RGB8", "3"),
                ("FORMAT_DXT1", "7"),
                ("FORMAT_BC7", "12"),
            ],
            holds: true,
        },
    ];

    #[test]
    fn the_conditions_of_the_shaders_are_evaluated_like_c_does() {
        for case in CONDITIONS {
            let source = preprocess(&conditional_shader(case.condition), case.macros);
            let taken = source.contains("value = 1.0;");
            let not_taken = source.contains("value = 2.0;");
            assert_eq!(taken, case.holds, "the condition `{}`", case.condition);
            assert_eq!(not_taken, !case.holds, "the condition `{}`", case.condition);
        }
    }

    #[test]
    fn the_defines_of_the_source_are_scoped_like_c_does() {
        // A define of a branch that is taken exists afterwards.
        let source =
            "#define OUTSIDE 1\n#if FOO\n#define INSIDE 1\n#endif\n#ifdef INSIDE\ntaken\n#endif\n";
        let source = preprocess(source, &[("FOO", "1")]);
        assert!(source.contains("taken"));

        // A define of a branch that is not taken does not exist.
        let source = "#if 0\n#define INSIDE 1\n#endif\n#ifdef INSIDE\ntaken\n#endif\n";
        let source = preprocess(source, &[]);
        assert!(!source.contains("taken"));

        // An `#undef` removes a macro.
        let source = "#define MYCONST 1\n#undef MYCONST\n#ifdef MYCONST\ntaken\n#endif\n";
        let source = preprocess(source, &[]);
        assert!(!source.contains("taken"));
    }

    #[test]
    fn the_macros_of_a_header_are_visible_to_the_file_that_includes_it() {
        let mut headers = new_map();
        headers.insert("common.h".to_string(), "#define MYCONST 1\n".to_string());

        let source = "#include \"common.h\"\n#ifdef MYCONST\ntaken\n#endif\n";
        let headers_ref = &headers;
        let layout = collect_layout(source, source, headers_ref);
        let (_, fragment, _) = preprocess_pair_with_layout(
            source,
            source,
            headers_ref,
            &new_map(),
            &layout,
            false,
            false,
        );
        assert!(fragment.contains("taken"));

        // The macro of the header does not exist without the include.
        let source = "#ifdef MYCONST\ntaken\n#endif\n";
        let layout = collect_layout(source, source, headers_ref);
        let (_, fragment, _) = preprocess_pair_with_layout(
            source,
            source,
            headers_ref,
            &new_map(),
            &layout,
            false,
            false,
        );
        assert!(!fragment.contains("taken"));
    }

    #[test]
    fn a_header_can_be_included_with_a_path() {
        let mut headers = new_map();
        headers.insert(
            "model_vertex_v1.h".to_string(),
            "#define MYCONST 1\n".to_string(),
        );

        let source = "#include \"base/model_vertex_v1.h\"\n#ifdef MYCONST\ntaken\n#endif\n";
        let layout = collect_layout(source, source, &headers);
        let (_, fragment, _) = preprocess_pair_with_layout(
            source,
            source,
            &headers,
            &new_map(),
            &layout,
            false,
            false,
        );
        assert!(fragment.contains("taken"));
    }

    #[test]
    fn the_mathematical_constants_exist_without_the_assets() {
        let source = preprocess("void main() {}\n", &[]);
        for name in ["M_PI", "M_PI_HALF", "M_PI_2", "SQRT_2", "SQRT_3"] {
            assert!(
                source.contains(&format!("#define {name} ")),
                "the constant {name} is missing:\n{source}"
            );
        }
    }

    /// The vertex shader of a workshop wallpaper that has a stray `#endif`.
    ///
    /// The reference engine of Wallpaper Engine ignores the stray `#endif`,
    /// so the preprocessor has to ignore it too: the shader has to be
    /// preprocessed and compiled, not skipped.
    const IRIS_MOVEMENT_VERTEX: &str = include_str!("fixtures/iris_movement.vert");

    /// The fragment shader that goes with [`IRIS_MOVEMENT_VERTEX`].
    const IRIS_MOVEMENT_FRAGMENT: &str = include_str!("fixtures/iris_movement.frag");

    /// The vertex shader of a second workshop wallpaper with a stray
    /// `#endif`.
    const SIMPLE_AUDIO_BARS_VERTEX: &str = include_str!("fixtures/simple_audio_bars.vert");

    #[test]
    fn a_shader_with_a_stray_endif_is_preprocessed() {
        let headers = new_map();
        let mut defines = new_map();
        for (name, value) in collect_default_defines(IRIS_MOVEMENT_VERTEX, IRIS_MOVEMENT_FRAGMENT) {
            defines.insert(name, value);
        }

        let layout = collect_layout(IRIS_MOVEMENT_VERTEX, IRIS_MOVEMENT_FRAGMENT, &headers);
        let (vertex, fragment, _) = preprocess_pair_with_layout(
            IRIS_MOVEMENT_VERTEX,
            IRIS_MOVEMENT_FRAGMENT,
            &headers,
            &defines,
            &layout,
            false,
            false,
        );

        // The shader is usable: it has an entry point and the branch that
        // the combos select is compiled.
        assert!(
            vertex.contains("void main()"),
            "the vertex shader is broken"
        );
        assert!(
            fragment.contains("void main()"),
            "the fragment shader is broken"
        );
        assert!(
            vertex.contains("g_CursorScale"),
            "the branch is not compiled"
        );

        // The uniforms of the branches that are not taken are not in the
        // shader.
        assert!(
            !vertex.contains("g_ManualScale"),
            "an inactive branch is compiled"
        );
        assert!(
            !vertex.contains("g_Scale;"),
            "an inactive branch is compiled"
        );
    }

    #[test]
    fn a_shader_with_a_stray_endif_is_preprocessed_in_every_stage() {
        let headers = new_map();
        let mut defines = new_map();
        for (name, value) in collect_default_defines(SIMPLE_AUDIO_BARS_VERTEX, "") {
            defines.insert(name, value);
        }

        let layout = collect_layout(SIMPLE_AUDIO_BARS_VERTEX, SIMPLE_AUDIO_BARS_VERTEX, &headers);
        let (vertex, _, _) = preprocess_pair_with_layout(
            SIMPLE_AUDIO_BARS_VERTEX,
            SIMPLE_AUDIO_BARS_VERTEX,
            &headers,
            &defines,
            &layout,
            false,
            false,
        );

        assert!(
            vertex.contains("void main()"),
            "the vertex shader is broken"
        );
        // The branch of the combos is compiled: the varying of the
        // `BAR_STYLE == 1` branch is there.
        assert!(
            vertex.contains("i_DCorrectingFactor"),
            "the branch of the combos is not compiled:\n{vertex}"
        );
    }

    #[test]
    fn an_array_uniform_is_declared_the_way_glsl_wants_it() {
        // GLSL wants the array brackets behind the name, the shader compiler
        // rejects `float [32] name;`.
        let source = "uniform float g_Spectrum[32];\nvoid main() {}\n";
        let headers = new_map();
        let layout = collect_layout(source, source, &headers);
        let (_, fragment, layout) = preprocess_pair_with_layout(
            source,
            source,
            &headers,
            &new_map(),
            &layout,
            false,
            false,
        );

        assert_eq!(
            layout.uniform_decls,
            vec![("g_Spectrum".to_string(), "float [32]".to_string())]
        );
        assert!(
            fragment.contains("float g_Spectrum[32];"),
            "the array uniform is declared the wrong way:\n{fragment}"
        );
    }

    /// The vertex shader of the blend mode scenario.
    const BLENDMODE_VERTEX: &str = "\
varying vec2 v_TexCoord;

void main() {
    v_TexCoord = vec2(0.5, 0.5);
    gl_Position = vec4(0.0, 0.0, 0.0, 1.0);
}
";

    /// The fragment shader of the blend mode scenario.
    ///
    /// The shader defines the macro itself, and the material sets it to
    /// another value: the macro of the material has to win, and the shader
    /// compiler must not see the macro twice.
    const BLENDMODE_FRAGMENT: &str = "\
#define BLENDMODE 0

varying vec2 v_TexCoord;

void main() {
    float value = 0.0;
#if BLENDMODE == 26
    value = 1.0;
#else
    value = 2.0;
#endif
    gl_FragColor = vec4(value, v_TexCoord.x, 0.0, 1.0);
}
";

    #[test]
    fn the_macro_of_the_material_wins_over_the_definition_of_the_source() {
        let headers = new_map();
        let mut defines = new_map();
        defines.insert("BLENDMODE".to_string(), "26".to_string());

        let layout = collect_layout(BLENDMODE_VERTEX, BLENDMODE_FRAGMENT, &headers);
        let (vertex, fragment, _) = preprocess_pair_with_layout(
            BLENDMODE_VERTEX,
            BLENDMODE_FRAGMENT,
            &headers,
            &defines,
            &layout,
            false,
            false,
        );

        // The definition of the source is dropped: the shader compiler gets
        // the macro of the material, and it must not see it twice.
        assert!(!fragment.contains("#define BLENDMODE"));
        assert!(!vertex.contains("#define BLENDMODE"));

        // The branch of the material is the one that is compiled.
        assert!(fragment.contains("value = 1.0;"));
        assert!(!fragment.contains("value = 2.0;"));

        compile(&vertex, naga::ShaderStage::Vertex, &[("BLENDMODE", "26")]);
        compile(
            &fragment,
            naga::ShaderStage::Fragment,
            &[("BLENDMODE", "26")],
        );
    }

    /// A fragment shader that reads a varying the vertex shader does not
    /// produce.
    ///
    /// The varying cannot be dropped: the fragment shader would not compile
    /// (`UnknownVariable`).  The vertex shader gets an output for it, so
    /// that the program is valid.
    #[test]
    fn a_varying_the_vertex_shader_does_not_produce_is_synthesised() {
        let vertex = "\
attribute vec3 a_Position;

void main() {
    gl_Position = vec4(a_Position, 1.0);
}
";
        let fragment = "\
varying vec2 v_TexCoord;

void main() {
    gl_FragColor = vec4(v_TexCoord, 0.0, 1.0);
}
";
        let headers = new_map();
        let layout = collect_layout(vertex, fragment, &headers);
        let (vertex, fragment, _) = preprocess_pair_with_layout(
            vertex,
            fragment,
            &headers,
            &new_map(),
            &layout,
            false,
            false,
        );

        // The fragment shader keeps the varying it reads.
        assert!(
            fragment.contains("in vec2 v_TexCoord;"),
            "the varying of the fragment shader is dropped:\n{fragment}"
        );
        // The vertex shader produces it.
        assert!(
            vertex.contains("out vec2 v_TexCoord;"),
            "the vertex shader does not produce the varying:\n{vertex}"
        );

        // The program is valid.
        compile(&vertex, naga::ShaderStage::Vertex, &[]);
        compile(&fragment, naga::ShaderStage::Fragment, &[]);
    }

    /// A varying that is an array keeps its size when it is synthesised.
    #[test]
    fn an_array_varying_that_is_missing_is_synthesised() {
        let vertex = "\
attribute vec3 a_Position;

void main() {
    gl_Position = vec4(a_Position, 1.0);
}
";
        let fragment = "\
varying vec2 v_TexCoord[4];

void main() {
    gl_FragColor = vec4(v_TexCoord[0], 0.0, 1.0);
}
";
        let headers = new_map();
        let layout = collect_layout(vertex, fragment, &headers);
        let (vertex, fragment, _) = preprocess_pair_with_layout(
            vertex,
            fragment,
            &headers,
            &new_map(),
            &layout,
            false,
            false,
        );

        assert!(
            vertex.contains("out vec2 v_TexCoord[4];"),
            "the array varying is not declared:\n{vertex}"
        );

        // The program is valid.
        compile(&vertex, naga::ShaderStage::Vertex, &[]);
        compile(&fragment, naga::ShaderStage::Fragment, &[]);
    }

    /// A fragment shader that writes to a varying is fixed.
    ///
    /// Vulkan GLSL does not allow the fragment shader to write to an input:
    /// the input is renamed and a local variable takes its place.
    #[test]
    fn a_fragment_shader_that_writes_to_a_varying_is_fixed() {
        let vertex = "\
varying vec2 v_UV;

void main() {
    v_UV = vec2(0.0);
}
";
        let fragment = "\
varying vec2 v_UV;
varying vec2 my_v_UV;

void main() {
    my_v_UV = v_UV;
    gl_FragColor = vec4(my_v_UV, 0.0, 1.0);
}
";
        let headers = new_map();
        let layout = collect_layout(vertex, fragment, &headers);
        let (_, fragment, _) = preprocess_pair_with_layout(
            vertex,
            fragment,
            &headers,
            &new_map(),
            &layout,
            false,
            false,
        );

        // The varying that is written to is renamed and copied into a local
        // variable.
        assert!(
            fragment.contains("in vec2 _in_my_v_UV;"),
            "the varying that is written to is not renamed:\n{fragment}"
        );
        assert!(
            fragment.contains("vec2 my_v_UV = _in_my_v_UV;"),
            "the local copy of the varying is missing:\n{fragment}"
        );
        // The varying that is only read keeps its name: the name of the one
        // is the tail of the name of the other, a text search would mix
        // them up.
        assert!(
            fragment.contains("in vec2 v_UV;"),
            "the varying that is only read is renamed:\n{fragment}"
        );

        compile(&fragment, naga::ShaderStage::Fragment, &[]);
    }

    /// Compile a GLSL source with naga and validate the result.
    fn compile(source: &str, stage: naga::ShaderStage, defines: &[(&str, &str)]) {
        let mut frontend = naga::front::glsl::Frontend::default();
        let options = naga::front::glsl::Options {
            stage,
            defines: defines
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
        };
        let module = match frontend.parse(&options, source) {
            Ok(module) => module,
            Err(errors) => panic!(
                "naga could not parse the {} shader:\n{}\n{}",
                stage_name(stage),
                errors.emit_to_string(source),
                source
            ),
        };
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        if let Err(error) = validator.validate(&module) {
            panic!(
                "naga rejected the {} shader: {:?}\n{}",
                stage_name(stage),
                error,
                source
            );
        }
    }

    /// The name of a shader stage, for the error messages.
    fn stage_name(stage: naga::ShaderStage) -> &'static str {
        match stage {
            naga::ShaderStage::Vertex => "vertex",
            naga::ShaderStage::Fragment => "fragment",
            naga::ShaderStage::Compute => "compute",
            _ => "unknown",
        }
    }
}
