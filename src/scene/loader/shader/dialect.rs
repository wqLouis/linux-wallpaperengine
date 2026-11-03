//! The Wallpaper Engine shader dialect.
//!
//! Wallpaper Engine's shaders are written in a dialect that looks like GLSL
//! but is not.  The engine's own compiler is lenient where GLSL is strict:
//! it promotes a scalar to a vector in the built-in calls, it knows the
//! DirectX style `mul` and it hands the shader a few macros the shader
//! expects to find.  naga, the shader compiler wgpu uses, rejects all of
//! that, so the real shaders do not compile.
//!
//! This module rewrites the dialect into the GLSL naga accepts.  It works
//! on the source text alone: it needs neither the layout of the effect nor
//! the headers of the engine, so it can be tested on its own.
//!
//! The rules, in the order they are applied:
//!
//! 1. The engine macros the pipeline does not provide are defined
//!    (`ENGINE_MACROS`), so that the shaders that use them compile.
//! 2. `mul(a, b)` becomes `a * b` (the `rewrite_mul` function).  Wallpaper
//!    Engine uses the row vector form of the product, so the operands are
//!    *not* swapped the way the DirectX `mul` would need them.  A nested
//!    call is rewritten from the inside out and a swizzle that follows the
//!    call is kept.
//! 3. The precision qualifiers `lowp`, `mediump` and `highp` are removed.
//!    They are legal GLSL, but naga's front end does not accept them in the
//!    uniform blocks the pipeline emits.
//! 4. The sampler types the engine has no binding for are approximated:
//!    `sampler2DComparison` and `sampler3D` become `sampler2D`.
//! 5. The built-in calls that want a vector are given one: a scalar
//!    argument is promoted to a vector of the size of the other arguments
//!    (`pow(v, 2.0)` becomes `pow(v, vec3(2.0))`).  The signatures of the
//!    built-ins are in the `SIGNATURES` table.
//!
//! Legal GLSL is left alone: every rule only fires on the constructs listed
//! above.

use std::collections::BTreeMap;

/// The macros the engine provides and the pipeline does not.
///
/// The engine tells the shader how many bones, blend rows or lights it has
/// to work with.  The values come from the material of the effect, but a
/// shader that is used without one - or with a material that does not set
/// the count - would not compile at all: the macro is the size of an array.
/// The dialect defines the macros a shader uses, so that the shader
/// compiles.
///
/// The default is `1` and not `0`: a zero length array is not a valid GLSL
/// array and naga rejects it ("Array size must be greater than zero").
///
/// Every definition is wrapped in an `#ifndef`, so a value the pipeline
/// does provide - the material sets the count - still wins.
const ENGINE_MACROS: &[(&str, &str)] = &[
    ("BONECOUNT", "1"),
    ("BLENDROWCOUNT", "1"),
    ("LIGHTS_DIRECTIONAL", "1"),
    ("LIGHTS_POINT", "1"),
    ("LIGHTS_SPOT", "1"),
    ("LIGHTS_TUBE", "1"),
];

/// A parameter of a built-in function.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Param {
    /// A float scalar.
    Scalar,
    /// A vector: all the vector parameters of one shape have the same size.
    Vector,
    /// A vector of exactly three components.
    Vector3,
}

/// The overloads of the built-in functions the dialect knows about.
///
/// A shape is the list of the parameters of one overload.  The shapes are
/// the overloads naga's GLSL front end offers, so a call that matches one
/// of them is left alone and a call that matches none of them is the one
/// the dialect has to fix: a scalar argument of a vector parameter is
/// promoted to a vector of the size of the other arguments.
///
/// The single argument built-ins (`sin`, `cos`, `length`, ...) are not in
/// the table: they take a scalar or a vector, never both, so there is
/// nothing to promote.
const SIGNATURES: &[(&str, &[&[Param]])] = &[
    (
        "atan",
        &[
            &[Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Vector],
        ],
    ),
    (
        "clamp",
        &[
            &[Param::Scalar, Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Vector, Param::Vector],
        ],
    ),
    ("cross", &[&[Param::Vector3, Param::Vector3]]),
    (
        "distance",
        &[
            &[Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Vector],
        ],
    ),
    (
        "dot",
        &[
            &[Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Vector],
        ],
    ),
    (
        "faceforward",
        &[
            &[Param::Scalar, Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Vector, Param::Vector],
        ],
    ),
    (
        "fma",
        &[
            &[Param::Scalar, Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Vector, Param::Vector],
        ],
    ),
    (
        "max",
        &[
            &[Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Scalar],
            &[Param::Scalar, Param::Vector],
            &[Param::Vector, Param::Vector],
        ],
    ),
    (
        "min",
        &[
            &[Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Scalar],
            &[Param::Scalar, Param::Vector],
            &[Param::Vector, Param::Vector],
        ],
    ),
    (
        "mix",
        &[
            &[Param::Scalar, Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Vector, Param::Scalar],
            &[Param::Vector, Param::Vector, Param::Vector],
        ],
    ),
    (
        "mod",
        &[
            &[Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Scalar],
            &[Param::Scalar, Param::Vector],
            &[Param::Vector, Param::Vector],
        ],
    ),
    ("pow", &[&[Param::Vector, Param::Vector]]),
    (
        "reflect",
        &[
            &[Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Vector],
        ],
    ),
    (
        "refract",
        &[
            &[Param::Scalar, Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Vector, Param::Scalar],
        ],
    ),
    (
        "smoothstep",
        &[
            &[Param::Scalar, Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Scalar, Param::Scalar],
            &[Param::Scalar, Param::Scalar, Param::Vector],
            &[Param::Vector, Param::Vector, Param::Vector],
        ],
    ),
    (
        "step",
        &[
            &[Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Scalar],
            &[Param::Scalar, Param::Vector],
            &[Param::Vector, Param::Vector],
        ],
    ),
];

/// The sampler types the engine has no binding for, and the type they are
/// approximated with.
///
/// The engine binds every texture as a two dimensional texture and it has
/// no comparison sampler, so a shadow map and a volume texture are sampled
/// like an ordinary texture.  The approximation is visible: the shader
/// compiles and runs, but the comparison of the shadow map is gone.
const SAMPLER_TYPES: &[(&str, &str)] = &[
    ("sampler2DComparison", "sampler2D"),
    ("sampler3D", "sampler2D"),
];

/// The type of an expression, as far as the dialect can work it out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    /// A scalar: a float, an integer or a boolean.
    Scalar,
    /// A vector of `n` components.
    Vector(u8),
    /// A matrix.
    Matrix,
    /// An array: indexing it yields a scalar.
    Array,
    /// A texture or a sampler.
    Opaque,
    /// Something the dialect cannot work out.
    Unknown,
}

/// The types of the variables a shader declares.
#[derive(Default)]
struct Symbols {
    /// The type of every variable the shader declares.
    types: BTreeMap<String, Ty>,
}

/// Normalise a shader source into the GLSL dialect naga accepts.
///
/// The log messages of the rules that have to report something name the
/// shader "shader"; use [`normalize_named`] to name it.
pub fn normalize(source: &str) -> String {
    normalize_named(source, "shader")
}

/// Normalise a shader source into the GLSL dialect naga accepts.
///
/// `name` is the name of the shader: it is used in the log messages the
/// rules that approximate something write.
pub fn normalize_named(source: &str, name: &str) -> String {
    let source = define_engine_macros(source);
    let source = rewrite_mul(&source);
    let source = strip_precision_qualifiers(&source);
    let source = rewrite_sampler_types(&source, name);
    promote_scalar_arguments(&source)
}

/// Rewrite the `mul(a, b)` calls of a source into the GLSL `a * b`.
///
/// Wallpaper Engine uses the row vector form of the product: `mul(a, b)`
/// means `a * b`, and *not* the `b * a` the DirectX `mul` would need.  The
/// calls are rewritten from the inside out, so a nested `mul` is rewritten
/// as well, and a swizzle or an index that follows the call is kept.
///
/// The body of a macro is rewritten as well: a `#define` that holds a `mul`
/// call expands to a call naga does not know.  The operands of such a call
/// are bracketed, because the parameters of the macro are substituted
/// without any precedence.
///
/// The source may be a whole shader or a single line: a call whose closing
/// parenthesis is not in the source is left alone.
pub(crate) fn rewrite_mul(text: &str) -> String {
    let mask = CodeMask::with_directives(text);
    rewrite_mul_region(text, &mask, 0, text.len()).text
}

/// A rewritten region of a source.
struct Fragment {
    /// The rewritten text of the region.
    text: String,
    /// The text holds an operator that is not enclosed in brackets, so it
    /// has to be wrapped in parentheses when it is used as an operand.
    compound: bool,
}

/// Rewrite the `mul` calls of the region `start..end` of `source`.
fn rewrite_mul_region(source: &str, mask: &CodeMask, start: usize, end: usize) -> Fragment {
    let mut text = String::new();
    let mut compound = false;
    let mut position = start;

    while let Some(call) = find_mul_call(source, mask, position, end) {
        text.push_str(&source[position..call.name_start]);
        if has_top_level_operator(source, mask, position, call.name_start) {
            compound = true;
        }

        match find_top_level_comma(source, mask, call.args_start, call.args_end) {
            Some(comma) => {
                let first = rewrite_mul_region(source, mask, call.args_start, comma);
                let second = rewrite_mul_region(source, mask, comma + 1, call.args_end);
                // The parameters of a macro are substituted without any
                // precedence, so the operands of a macro body are always
                // bracketed.
                let bracket = in_define(source, call.name_start);
                let mut product = String::new();
                push_operand(&mut product, &first, bracket);
                product.push_str(" * ");
                push_operand(&mut product, &second, bracket);
                // A swizzle or an index that follows the call needs the
                // product to be bracketed.
                if follows_member(source, mask, call.close + 1, end) {
                    product = format!("({})", product);
                }
                text.push_str(&product);
                compound = true;
            }
            None => {
                // Not a call with two arguments: keep it as it is.
                text.push_str(&source[call.name_start..call.close + 1]);
            }
        }

        position = call.close + 1;
    }

    text.push_str(&source[position..end]);
    if has_top_level_operator(source, mask, position, end) {
        compound = true;
    }

    Fragment { text, compound }
}

/// Append an operand to a product, bracketing it when it needs it.
fn push_operand(product: &mut String, operand: &Fragment, bracket: bool) {
    if bracket || operand.compound {
        product.push('(');
        product.push_str(operand.text.trim());
        product.push(')');
    } else {
        product.push_str(operand.text.trim());
    }
}

/// Whether the position is inside the body of a `#define`.
///
/// The parameters of a macro are substituted without any precedence, so the
/// operands of a `mul` call in a macro body have to be bracketed.
fn in_define(source: &str, index: usize) -> bool {
    let line_start = source[..index]
        .rfind('\n')
        .map(|position| position + 1)
        .unwrap_or(0);
    let line = source[line_start..].trim_start();
    let Some(rest) = line.strip_prefix('#') else {
        return false;
    };
    rest.trim_start().starts_with("define")
}

/// A `mul` call in a source.
struct MulCall {
    /// The start of the `mul` identifier.
    name_start: usize,
    /// The start of the arguments: the byte after the opening parenthesis.
    args_start: usize,
    /// The end of the arguments: the byte before the closing parenthesis.
    args_end: usize,
    /// The closing parenthesis of the call.
    close: usize,
}

/// The first `mul` call of the region `start..end` of `source`.
fn find_mul_call(source: &str, mask: &CodeMask, start: usize, end: usize) -> Option<MulCall> {
    let bytes = source.as_bytes();
    let mut index = start;

    while index < end {
        while index < end && !(bytes[index].is_ascii_alphabetic() || bytes[index] == b'_') {
            index += 1;
        }
        if index >= end {
            break;
        }

        let name_start = index;
        while index < end && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_') {
            index += 1;
        }

        if index - name_start != 3 || &source[name_start..index] != "mul" {
            continue;
        }
        if !mask.is_code(name_start) {
            continue;
        }

        let mut open = index;
        while open < end && bytes[open].is_ascii_whitespace() {
            open += 1;
        }
        if open >= end || bytes[open] != b'(' {
            continue;
        }

        let Some(close) = matching_paren(source, mask, open) else {
            continue;
        };
        if close >= end {
            continue;
        }

        return Some(MulCall {
            name_start,
            args_start: open + 1,
            args_end: close,
            close,
        });
    }

    None
}

/// Define the engine macros a shader uses.
fn define_engine_macros(source: &str) -> String {
    let mask = CodeMask::new(source);
    let tokens = tokenize(source, &mask);
    let defined = defined_macros(source);

    let mut wanted: Vec<(&str, &str)> = Vec::new();
    for (name, value) in ENGINE_MACROS {
        if defined.iter().any(|defined| defined.as_str() == *name) {
            continue;
        }
        let used = tokens
            .iter()
            .any(|token| token.kind == TokenKind::Word && token.text == *name);
        if used {
            wanted.push((*name, *value));
        }
    }

    if wanted.is_empty() {
        return source.to_string();
    }

    let mut block = String::new();
    for (name, value) in wanted {
        block.push_str(&format!("#ifndef {name}\n#define {name} {value}\n#endif\n"));
    }

    let insertion = macro_insertion_point(source);
    let mut result = String::with_capacity(source.len() + block.len());
    result.push_str(&source[..insertion]);
    result.push_str(&block);
    result.push_str(&source[insertion..]);
    result
}

/// The names the source defines itself.
fn defined_macros(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("#define ") else {
            continue;
        };
        let name: String = rest
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            names.push(name);
        }
    }
    names
}

/// The place the engine macros are inserted at.
///
/// The definitions have to come after the `#version` and the `#extension`
/// directives: the version of the shader has to be the first thing in the
/// file.
fn macro_insertion_point(source: &str) -> usize {
    let mut insertion = 0;
    let mut offset = 0;

    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#version") || trimmed.starts_with("#extension") {
            insertion = offset + line.len();
        }
        offset += line.len();
    }

    insertion
}

/// Remove the precision qualifiers from a source.
///
/// `lowp`, `mediump` and `highp` are legal GLSL, but naga's front end does
/// not accept them in the uniform blocks the pipeline emits.  A `precision`
/// statement keeps its qualifier: it sets the default precision and has to
/// stay as it is.
fn strip_precision_qualifiers(source: &str) -> String {
    let mask = CodeMask::new(source);
    let tokens = tokenize(source, &mask);

    let mut text = String::with_capacity(source.len());
    let mut position = 0;

    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Word {
            continue;
        }
        if !matches!(token.text, "lowp" | "mediump" | "highp") {
            continue;
        }
        // A `precision mediump float;` statement is not a qualifier.
        if index > 0
            && tokens[index - 1].kind == TokenKind::Word
            && tokens[index - 1].text == "precision"
        {
            continue;
        }

        let mut end = token.end;
        // The space that follows the qualifier goes with it.
        if source[end..].starts_with(' ') {
            end += 1;
        }
        text.push_str(&source[position..token.start]);
        position = end;
    }

    text.push_str(&source[position..]);
    text
}

/// Rewrite the sampler types the engine has no binding for.
fn rewrite_sampler_types(source: &str, name: &str) -> String {
    let mask = CodeMask::new(source);
    let tokens = tokenize(source, &mask);

    let mut edits: Vec<Edit> = Vec::new();
    let mut reported: Vec<&str> = Vec::new();

    for token in &tokens {
        if token.kind != TokenKind::Word {
            continue;
        }
        let Some((_, replacement)) = SAMPLER_TYPES.iter().find(|(ty, _)| *ty == token.text) else {
            continue;
        };
        if !reported.contains(&token.text) {
            reported.push(token.text);
            log::warn!(
                "shader '{}': the sampler type '{}' is not supported, '{}' is used instead",
                name,
                token.text,
                replacement
            );
        }
        edits.push(Edit {
            start: token.start,
            end: token.end,
            text: (*replacement).to_string(),
        });
    }

    apply_edits(source, edits)
}

/// Promote the scalar arguments of the built-in calls to vectors.
fn promote_scalar_arguments(source: &str) -> String {
    let mask = CodeMask::new(source);
    let symbols = collect_symbols(source, &mask);
    let tokens = tokenize(source, &mask);

    let mut edits: Vec<Edit> = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Word {
            continue;
        }
        let shapes = SIGNATURES
            .iter()
            .find(|(name, _)| *name == token.text)
            .map(|(_, shapes)| *shapes);
        let Some(shapes) = shapes else {
            continue;
        };
        // The name has to be the one of the call.
        let Some(open) = punct_after(&tokens, index, "(") else {
            continue;
        };
        let Some(close) = matching_paren(source, &mask, open) else {
            continue;
        };

        let arguments = argument_ranges(source, &mask, open + 1, close);
        if arguments.is_empty() {
            continue;
        }
        let types: Vec<Ty> = arguments
            .iter()
            .map(|argument| estimate_type(source[argument.0..argument.1].trim(), &symbols))
            .collect();

        // The shape that fits the arguments with the fewest promotions wins:
        // a call that matches an overload as it is must not be touched.
        let mut best: Option<Vec<Option<u8>>> = None;
        for shape in shapes {
            let Some(promotions) = fit_shape(shape, &types) else {
                continue;
            };
            let count = promotions
                .iter()
                .filter(|promotion| promotion.is_some())
                .count();
            let better = match &best {
                None => true,
                Some(current) => {
                    count
                        < current
                            .iter()
                            .filter(|promotion| promotion.is_some())
                            .count()
                }
            };
            if better {
                best = Some(promotions);
            }
        }

        let Some(promotions) = best else {
            continue;
        };
        if !promotions.iter().any(|promotion| promotion.is_some()) {
            continue;
        }

        for (argument, promotion) in arguments.iter().zip(promotions.iter()) {
            let Some(size) = promotion else {
                continue;
            };
            let text = &source[argument.0..argument.1];
            let trimmed = text.trim();
            // The space in front of the argument belongs to it.
            let space = &text[..text.len() - text.trim_start().len()];
            edits.push(Edit {
                start: argument.0,
                end: argument.1,
                text: format!("{}vec{}({})", space, size, trimmed),
            });
        }
    }

    apply_edits(source, edits)
}

/// The promotions a shape needs for the given argument types.
///
/// Every entry is the size of the vector the argument has to be promoted
/// to, or `None` when the argument stays as it is.  The shape does not fit
/// when one of the arguments cannot be used at all.
fn fit_shape(shape: &[Param], types: &[Ty]) -> Option<Vec<Option<u8>>> {
    if shape.len() != types.len() {
        return None;
    }

    // The size of the vector arguments: all the vector parameters of a
    // shape have the same size.
    let mut size: Option<u8> = None;
    for (param, ty) in shape.iter().zip(types.iter()) {
        if !matches!(param, Param::Vector) {
            continue;
        }
        let Ty::Vector(size_of) = ty else {
            continue;
        };
        match size {
            None => size = Some(*size_of),
            Some(current) if current == *size_of => {}
            Some(_) => return None,
        }
    }

    let mut promotions: Vec<Option<u8>> = Vec::with_capacity(types.len());
    for (param, ty) in shape.iter().zip(types.iter()) {
        match (param, ty) {
            (Param::Scalar, Ty::Scalar) => promotions.push(None),
            (Param::Scalar, _) => return None,
            (Param::Vector3, Ty::Vector(3)) => promotions.push(None),
            (Param::Vector3, Ty::Scalar) => promotions.push(Some(3)),
            (Param::Vector3, _) => return None,
            (Param::Vector, Ty::Vector(size_of)) => {
                if size != Some(*size_of) {
                    return None;
                }
                promotions.push(None);
            }
            (Param::Vector, Ty::Scalar) => {
                // The scalar is promoted to the size of the vector
                // arguments of the call, if there is one.
                promotions.push(size);
            }
            (Param::Vector, _) => return None,
        }
    }

    Some(promotions)
}

/// The types of the variables a shader declares.
fn collect_symbols(source: &str, mask: &CodeMask) -> Symbols {
    let tokens = tokenize(source, mask);
    let mut symbols = Symbols::default();

    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Word {
            continue;
        }
        let Some(ty) = keyword_type(token.text) else {
            continue;
        };

        // The name of the variable follows its type.
        let Some(name) = tokens.get(index + 1) else {
            continue;
        };
        if name.kind != TokenKind::Word {
            continue;
        }
        // A type followed by a name and a parenthesis is a function.
        if let Some(next) = tokens.get(index + 2)
            && next.kind == TokenKind::Punct
            && next.text == "("
        {
            continue;
        }

        // An array: `vec4 g_Bones[4]`.
        let mut ty = ty;
        if let Some(next) = tokens.get(index + 2)
            && next.kind == TokenKind::Punct
            && next.text == "["
        {
            ty = Ty::Array;
        }

        symbols.types.entry(name.text.to_string()).or_insert(ty);
    }

    symbols
}

/// The type a type keyword stands for.
fn keyword_type(word: &str) -> Option<Ty> {
    Some(match word {
        "bool" | "double" | "float" | "int" | "uint" => Ty::Scalar,
        "ivec2" | "uvec2" | "bvec2" | "vec2" | "CAST2" => Ty::Vector(2),
        "ivec3" | "uvec3" | "bvec3" | "vec3" | "CAST3" => Ty::Vector(3),
        "ivec4" | "uvec4" | "bvec4" | "vec4" | "CAST4" => Ty::Vector(4),
        "mat2" | "mat2x2" | "mat2x3" | "mat2x4" | "mat3" | "mat3x2" | "mat3x3" | "mat3x4"
        | "mat4" | "mat4x2" | "mat4x3" | "mat4x4" | "CAST3X3" => Ty::Matrix,
        "sampler"
        | "sampler2D"
        | "sampler2DComparison"
        | "sampler3D"
        | "texture2D"
        | "texture3D" => Ty::Opaque,
        _ => return None,
    })
}

/// The built-ins that return the type of their first argument.
fn preserves_its_argument(name: &str) -> bool {
    matches!(
        name,
        "abs"
            | "acos"
            | "asin"
            | "atan"
            | "ceil"
            | "cos"
            | "degrees"
            | "dFdx"
            | "dFdy"
            | "exp"
            | "exp2"
            | "floor"
            | "fract"
            | "fwidth"
            | "inversesqrt"
            | "log"
            | "log2"
            | "normalize"
            | "radians"
            | "round"
            | "roundEven"
            | "saturate"
            | "sign"
            | "sin"
            | "sqrt"
            | "tan"
            | "trunc"
    )
}

/// The type of an expression.
fn estimate_type(text: &str, symbols: &Symbols) -> Ty {
    let text = text.trim();
    if text.is_empty() {
        return Ty::Unknown;
    }

    // A number or a boolean.
    let first = text.as_bytes()[0];
    if first.is_ascii_digit()
        || (first == b'.'
            && text
                .as_bytes()
                .get(1)
                .is_some_and(|byte| byte.is_ascii_digit()))
        || text == "true"
        || text == "false"
    {
        return Ty::Scalar;
    }

    // A sign.
    if first == b'+' || first == b'-' {
        return estimate_type(&text[1..], symbols);
    }

    // A bracketed expression.
    if text.starts_with('(') && text.ends_with(')') && is_bracketed(text) {
        return estimate_type(&text[1..text.len() - 1], symbols);
    }

    // A call: a constructor or a built-in.
    if text.ends_with(')')
        && let Some(open) = matching_open(text, b'(', b')')
    {
        let name = text[..open].trim();
        if let Some(ty) = keyword_type(name) {
            return ty;
        }
        if preserves_its_argument(name) {
            let arguments = split_arguments(&text[open + 1..text.len() - 1]);
            let first = arguments.first().copied().unwrap_or("");
            return estimate_type(first, symbols);
        }
        return Ty::Unknown;
    }

    // An index.
    if text.ends_with(']')
        && let Some(open) = matching_open(text, b'[', b']')
    {
        return match estimate_type(&text[..open], symbols) {
            Ty::Array | Ty::Matrix | Ty::Vector(_) => Ty::Scalar,
            _ => Ty::Unknown,
        };
    }

    // A binary expression: the type of its operands.
    if let Some(ty) = binary_type(text, symbols) {
        return ty;
    }

    // A swizzle.
    if let Some(dot) = text.rfind('.') {
        let base = estimate_type(&text[..dot], symbols);
        let swizzle = &text[dot + 1..];
        if !is_swizzle(swizzle) {
            return Ty::Unknown;
        }
        return match base {
            Ty::Vector(_) => match swizzle.len() {
                1 => Ty::Scalar,
                2 => Ty::Vector(2),
                3 => Ty::Vector(3),
                4 => Ty::Vector(4),
                _ => Ty::Unknown,
            },
            _ => Ty::Unknown,
        };
    }

    // A name.
    symbols.types.get(text).copied().unwrap_or(Ty::Unknown)
}

/// Whether a word is a swizzle.
fn is_swizzle(word: &str) -> bool {
    !word.is_empty() && word.len() <= 4 && word.chars().all(|c| "xyzwrgbastpq".contains(c))
}

/// Whether the brackets of a text enclose the whole text.
fn is_bracketed(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 && index + 1 != bytes.len() {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

/// The type of a binary expression.
///
/// A vector and a scalar make a vector, two vectors make a vector and a
/// comparison makes a boolean.
fn binary_type(text: &str, symbols: &Symbols) -> Option<Ty> {
    let (left, right, comparison) = split_binary(text)?;
    if comparison {
        return Some(Ty::Scalar);
    }
    let left = estimate_type(left, symbols);
    let right = estimate_type(right, symbols);
    Some(match (left, right) {
        (Ty::Vector(size), Ty::Scalar) | (Ty::Scalar, Ty::Vector(size)) => Ty::Vector(size),
        (Ty::Vector(size), Ty::Vector(_)) => Ty::Vector(size),
        (Ty::Scalar, Ty::Scalar) => Ty::Scalar,
        _ => Ty::Unknown,
    })
}

/// The two sides of a binary expression.
///
/// The third value tells whether the expression is a comparison: a
/// comparison makes a boolean and not a number.
fn split_binary(text: &str) -> Option<(&str, &str, bool)> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut split: Option<usize> = None;
    let mut comparison = false;
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'+' | b'-' | b'*' | b'/' | b'%' => {
                if depth == 0 && !is_unary_sign(bytes, index) {
                    split = split.or(Some(index));
                }
            }
            b'<' | b'>' | b'=' | b'!' | b'&' | b'|' | b'^' if depth == 0 => {
                comparison = true;
            }
            _ => {}
        }
        index += 1;
    }

    let split = split?;
    Some((&text[..split], &text[split + 1..], comparison))
}

/// Whether the sign at `index` is the sign of a number and not the operator
/// of a subtraction.
fn is_unary_sign(bytes: &[u8], index: usize) -> bool {
    let mut cursor = index;
    while cursor > 0 {
        cursor -= 1;
        let byte = bytes[cursor];
        if byte.is_ascii_whitespace() {
            continue;
        }
        return matches!(
            byte,
            b'(' | b','
                | b'+'
                | b'-'
                | b'*'
                | b'/'
                | b'%'
                | b'='
                | b'<'
                | b'>'
                | b'&'
                | b'|'
                | b'!'
                | b'^'
        );
    }
    true
}

/// The position of the bracket that opens the one that closes the text.
fn matching_open(text: &str, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0i32;
    for (index, byte) in text.as_bytes().iter().enumerate().rev() {
        if *byte == close {
            depth += 1;
        } else if *byte == open {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
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

/// The ranges of the arguments of a call.
fn argument_ranges(source: &str, mask: &CodeMask, start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut depth = 0i32;
    let mut argument_start = start;

    for (offset, byte) in source.as_bytes()[start..end].iter().enumerate() {
        let index = start + offset;
        if !mask.is_code(index) {
            continue;
        }
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                ranges.push((argument_start, index));
                argument_start = index + 1;
            }
            _ => {}
        }
    }
    ranges.push((argument_start, end));

    ranges
}

/// The punctuation that follows a token.
fn punct_after(tokens: &[Token<'_>], index: usize, punct: &str) -> Option<usize> {
    let token = tokens.get(index + 1)?;
    if token.kind == TokenKind::Punct && token.text == punct {
        Some(token.start)
    } else {
        None
    }
}

/// The position of the parenthesis that closes the one at `open`.
fn matching_paren(source: &str, mask: &CodeMask, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0i32;
    let mut index = open;

    while index < bytes.len() {
        if mask.is_code(index) {
            match bytes[index] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }

    None
}

/// The position of the comma that separates the arguments of a call.
fn find_top_level_comma(source: &str, mask: &CodeMask, start: usize, end: usize) -> Option<usize> {
    let mut depth = 0i32;

    for (offset, byte) in source.as_bytes()[start..end].iter().enumerate() {
        let index = start + offset;
        if !mask.is_code(index) {
            continue;
        }
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => return Some(index),
            _ => {}
        }
    }

    None
}

/// Whether the region holds an operator that is not enclosed in brackets.
fn has_top_level_operator(source: &str, mask: &CodeMask, start: usize, end: usize) -> bool {
    let bytes = source.as_bytes();
    let mut depth = 0i32;
    let mut index = start;

    while index < end {
        if !mask.is_code(index) {
            index += 1;
            continue;
        }
        let byte = bytes[index];
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'0'..=b'9' => {
                // Skip the number, so that the sign of an exponent is not
                // taken for an operator.
                index = skip_number(source, mask, index);
                continue;
            }
            b'+' | b'-' | b'*' | b'/' | b'%' | b'<' | b'>' | b'=' | b'!' | b'&' | b'|' | b'^'
            | b'?' | b':'
                if depth == 0 =>
            {
                return true;
            }
            _ => {}
        }
        index += 1;
    }

    false
}

/// The position after the number that starts at `index`.
fn skip_number(source: &str, mask: &CodeMask, index: usize) -> usize {
    let bytes = source.as_bytes();
    let mut cursor = index;
    while cursor < bytes.len()
        && mask.is_code(cursor)
        && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'.')
    {
        cursor += 1;
    }
    cursor
}

/// Whether a member access follows the position `index`.
fn follows_member(source: &str, mask: &CodeMask, index: usize, end: usize) -> bool {
    let bytes = source.as_bytes();
    let mut cursor = index;

    while cursor < end {
        if !mask.is_code(cursor) {
            cursor += 1;
            continue;
        }
        if bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
            continue;
        }
        return matches!(bytes[cursor], b'.' | b'[');
    }

    false
}

/// A replacement of a range of a source.
struct Edit {
    /// The range that is replaced.
    start: usize,
    end: usize,
    /// The text that replaces it.
    text: String,
}

/// Replace the ranges of the source the edits cover.
fn apply_edits(source: &str, mut edits: Vec<Edit>) -> String {
    if edits.is_empty() {
        return source.to_string();
    }
    edits.sort_by_key(|edit| edit.start);

    let mut result = String::with_capacity(source.len());
    let mut position = 0;
    for edit in edits {
        if edit.start < position {
            // Overlapping edits cannot happen, but skipping one is safer
            // than producing a broken shader.
            continue;
        }
        result.push_str(&source[position..edit.start]);
        result.push_str(&edit.text);
        position = edit.end;
    }
    result.push_str(&source[position..]);
    result
}

/// The kind of a token.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TokenKind {
    /// An identifier or a keyword.
    Word,
    /// A number.
    Number,
    /// Punctuation: a single character.
    Punct,
}

/// A token of a source.
#[derive(Clone, Copy, Debug)]
struct Token<'a> {
    /// The kind of the token.
    kind: TokenKind,
    /// The start of the token.
    start: usize,
    /// The end of the token.
    end: usize,
    /// The text of the token.
    text: &'a str,
}

/// The tokens of the code of a source.
///
/// The comments, the string literals and the preprocessor directives are
/// not code: the rules must not touch them.
fn tokenize<'a>(source: &'a str, mask: &CodeMask) -> Vec<Token<'a>> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        if !mask.is_code(index) {
            index += 1;
            continue;
        }

        let byte = bytes[index];
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = index;
            while index < bytes.len()
                && mask.is_code(index)
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Word,
                start,
                end: index,
                text: &source[start..index],
            });
            continue;
        }

        if byte.is_ascii_digit()
            || (byte == b'.'
                && bytes
                    .get(index + 1)
                    .is_some_and(|next| next.is_ascii_digit()))
        {
            let start = index;
            index += 1;
            while index < bytes.len()
                && mask.is_code(index)
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'.')
            {
                index += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Number,
                start,
                end: index,
                text: &source[start..index],
            });
            continue;
        }

        if byte >= 0x80 {
            // A multi byte character: skip it whole.
            let character = source[index..].chars().next().unwrap_or('\u{fffd}');
            index += character.len_utf8();
            continue;
        }

        if !byte.is_ascii_whitespace() {
            tokens.push(Token {
                kind: TokenKind::Punct,
                start: index,
                end: index + 1,
                text: &source[index..index + 1],
            });
        }
        index += 1;
    }

    tokens
}

/// The bytes of a source that hold code.
///
/// The comments, the string literals and the preprocessor directives are
/// not code: the rules must not touch them.
struct CodeMask {
    /// One flag per byte of the source.
    code: Vec<bool>,
}

impl CodeMask {
    /// The mask of a source.
    ///
    /// The comments, the string literals and the preprocessor directives are
    /// not code.
    fn new(source: &str) -> Self {
        Self::build(source, false)
    }

    /// The mask of a source whose preprocessor directives count as code.
    ///
    /// The body of a macro is code: the `mul` call of a `#define` has to be
    /// rewritten as well, or the macro expands to a call naga does not know.
    fn with_directives(source: &str) -> Self {
        Self::build(source, true)
    }

    /// Build the mask of a source.
    fn build(source: &str, keep_directives: bool) -> Self {
        let bytes = source.as_bytes();
        let mut code = vec![true; bytes.len()];
        let mut index = 0;
        let mut in_block_comment = false;
        let mut in_line_comment = false;
        let mut at_line_start = true;

        while index < bytes.len() {
            let byte = bytes[index];

            if in_block_comment {
                code[index] = false;
                if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    code[index + 1] = false;
                    in_block_comment = false;
                    index += 2;
                } else {
                    index += 1;
                }
                continue;
            }

            if in_line_comment {
                code[index] = false;
                if byte == b'\n' {
                    in_line_comment = false;
                    at_line_start = true;
                }
                index += 1;
                continue;
            }

            if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
                code[index] = false;
                code[index + 1] = false;
                in_line_comment = true;
                index += 2;
                continue;
            }

            if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
                code[index] = false;
                code[index + 1] = false;
                in_block_comment = true;
                index += 2;
                continue;
            }

            if byte == b'"' || byte == b'\'' {
                // A string or a character literal.
                let quote = byte;
                code[index] = false;
                index += 1;
                while index < bytes.len() {
                    code[index] = false;
                    if bytes[index] == b'\\' {
                        index += 1;
                        if index < bytes.len() {
                            code[index] = false;
                            index += 1;
                        }
                        continue;
                    }
                    let done = bytes[index] == quote || bytes[index] == b'\n';
                    index += 1;
                    if done {
                        break;
                    }
                }
                at_line_start = false;
                continue;
            }

            if at_line_start && byte == b'#' {
                // A preprocessor directive: the whole line is not code.
                while index < bytes.len() && bytes[index] != b'\n' {
                    if !keep_directives {
                        code[index] = false;
                    }
                    index += 1;
                }
                continue;
            }

            if byte == b'\n' {
                at_line_start = true;
                index += 1;
                continue;
            }

            if !byte.is_ascii_whitespace() {
                at_line_start = false;
            }
            index += 1;
        }

        Self { code }
    }

    /// Whether the byte at `index` is code.
    fn is_code(&self, index: usize) -> bool {
        self.code.get(index).copied().unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shaders of the engine the fixtures are taken from, with the
    /// stage they are compiled for.
    const FIXTURES: &[(&str, &str, naga::ShaderStage)] = &[
        (
            "nested_mul.vert",
            include_str!("tests/nested_mul.vert"),
            naga::ShaderStage::Vertex,
        ),
        (
            "pow_scalar.frag",
            include_str!("tests/pow_scalar.frag"),
            naga::ShaderStage::Fragment,
        ),
        (
            "precision.frag",
            include_str!("tests/precision.frag"),
            naga::ShaderStage::Fragment,
        ),
        (
            "shadow_sampler.frag",
            include_str!("tests/shadow_sampler.frag"),
            naga::ShaderStage::Fragment,
        ),
        (
            "engine_macros.vert",
            include_str!("tests/engine_macros.vert"),
            naga::ShaderStage::Vertex,
        ),
    ];

    /// The error naga reports for a shader, if it reports one.
    fn compile_error(source: &str, stage: naga::ShaderStage) -> Option<String> {
        let options = naga::front::glsl::Options {
            stage,
            defines: Default::default(),
        };
        match naga::front::glsl::Frontend::default().parse(&options, source) {
            Ok(module) => {
                let mut validator = naga::valid::Validator::new(
                    naga::valid::ValidationFlags::all(),
                    naga::valid::Capabilities::all(),
                );
                validator
                    .validate(&module)
                    .err()
                    .map(|error| error.emit_to_string(source))
            }
            Err(errors) => Some(errors.emit_to_string(source)),
        }
    }

    /// The `mul` call keeps the order of its operands: Wallpaper Engine uses
    /// the row vector form of the product.
    #[test]
    fn mul_keeps_the_row_vector_order() {
        let source = "gl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix);";
        assert_eq!(
            rewrite_mul(source),
            "gl_Position = vec4(a_Position, 1.0) * g_ModelViewProjectionMatrix;"
        );
    }

    /// A nested `mul` is rewritten from the inside out.
    #[test]
    fn a_nested_mul_is_rewritten() {
        let source = "gl_Position = mul(mul(vec4(a_Position, 1.0), g_ModelMatrix), g_ViewMatrix);";
        assert_eq!(
            rewrite_mul(source),
            "gl_Position = (vec4(a_Position, 1.0) * g_ModelMatrix) * g_ViewMatrix;"
        );
    }

    /// A swizzle that follows the call is kept.
    #[test]
    fn a_swizzle_after_a_mul_is_kept() {
        let source = "vec3 v = mul(a, b).xyz;";
        assert_eq!(rewrite_mul(source), "vec3 v = (a * b).xyz;");
    }

    /// An operand that is an expression of its own is bracketed.
    #[test]
    fn an_operand_that_is_an_expression_is_bracketed() {
        let source = "vec3 v = mul(a - b, c);";
        assert_eq!(rewrite_mul(source), "vec3 v = (a - b) * c;");
    }

    /// A name that only starts with `mul` is not a call.
    #[test]
    fn a_name_that_is_not_a_call_is_left_alone() {
        let source = "float m = multiply(a, b) + mul_count;";
        assert_eq!(rewrite_mul(source), source);
    }

    /// A `mul` whose closing parenthesis is not in the source is left alone:
    /// the call is not complete.
    #[test]
    fn an_incomplete_mul_is_left_alone() {
        let source = "mat3 m = mul(mul(";
        assert_eq!(rewrite_mul(source), source);
    }

    /// A `mul` that spans several lines is rewritten as well: the dialect
    /// stage works on the whole source, the compat stage only on a line.
    #[test]
    fn a_mul_that_spans_several_lines_is_rewritten() {
        let source = "mat3 m = mul(mul(\n\tmat3(1, 0, 0),\n\tmat3(0, 1, 0)),\n\tmat3(0, 0, 1));\n";
        let normalized = rewrite_mul(source);
        assert!(normalized.contains("(mat3(1, 0, 0) * mat3(0, 1, 0)) * mat3(0, 0, 1)"));
    }

    /// Legal GLSL is not touched at all.
    #[test]
    fn legal_glsl_is_not_changed() {
        let source = "#version 450\n\
             layout(location=0) in vec3 v_Color;\n\
             layout(binding=0, std140) uniform EffectParams {\n\
             \tmat4 g_Model;\n\
             \tvec2 g_Scale;\n\
             };\n\
             out vec4 _fragColor;\n\
             void main() {\n\
             \tvec3 a = v_Color * mat3(1.0);\n\
             \tvec4 b = mat4(1.0) * vec4(a, 1.0);\n\
             \tvec3 c = a * 2.0;\n\
             \tvec3 d = normalize(a);\n\
             \tfloat e = dot(a, c);\n\
             \t_fragColor = vec4(b) + vec4(c * e, d.x);\n\
             }\n";
        assert_eq!(normalize(source), source);
    }

    /// The precision qualifiers are removed.
    #[test]
    fn the_precision_qualifiers_are_stripped() {
        let source = "layout(binding=0, std140) uniform EffectParams {\n\
             \tmediump float g_Alpha;\n\
             \tlowp vec3 g_Color;\n\
             };\n";
        let normalized = normalize(source);
        assert!(normalized.contains("float g_Alpha;"));
        assert!(normalized.contains("vec3 g_Color;"));
        assert!(!normalized.contains("mediump"));
        assert!(!normalized.contains("lowp"));
    }

    /// A `precision` statement sets the default precision: it stays.
    #[test]
    fn a_precision_statement_is_kept() {
        let source = "precision mediump float;\n";
        assert_eq!(normalize(source), source);
    }

    /// The sampler types the engine has no binding for are approximated.
    #[test]
    fn the_sampler_types_are_approximated() {
        let source = include_str!("tests/sampler_types.frag");
        let normalized = normalize(source);
        assert!(normalized.contains("uniform sampler2D g_Texture6;"));
        assert!(normalized.contains("uniform sampler2D g_Texture1;"));
        assert!(!normalized.contains("sampler2DComparison"));
        assert!(!normalized.contains("sampler3D"));
    }

    /// The engine macros a shader uses are defined.
    #[test]
    fn the_engine_macros_are_defined() {
        let source = "uniform mat4x3 [BONECOUNT] g_Bones;\n";
        let normalized = normalize(source);
        assert!(normalized.contains("#ifndef BONECOUNT"));
        assert!(normalized.contains("#define BONECOUNT 1"));
    }

    /// A macro the shader defines itself is not defined again.
    #[test]
    fn a_macro_the_source_defines_is_not_defined_again() {
        let source = "#define BONECOUNT 4\nuniform mat4x3 [BONECOUNT] g_Bones;\n";
        let normalized = normalize(source);
        assert!(!normalized.contains("#ifndef BONECOUNT"));
        assert_eq!(normalized, source);
    }

    /// A macro the shader does not use is not defined.
    #[test]
    fn a_macro_the_shader_does_not_use_is_not_defined() {
        let source = "void main() { }\n";
        assert_eq!(normalize(source), source);
    }

    /// A scalar argument of a vector parameter is promoted.
    #[test]
    fn a_scalar_argument_is_promoted() {
        let source = "vec3 albedo = v_Color;\nfloat a = pow(albedo, 2.2);\n";
        let normalized = normalize(source);
        assert!(normalized.contains("pow(albedo, vec3(2.2))"));
    }

    /// A call naga accepts is not touched: `min` has an overload that takes
    /// a scalar and a vector.
    #[test]
    fn a_call_naga_accepts_is_not_changed() {
        let source = "vec3 albedo = v_Color;\nfloat a = min(albedo, 0.0);\n";
        assert_eq!(normalize(source), source);
    }

    /// A call with two scalars is not touched either.
    #[test]
    fn a_call_with_two_scalars_is_not_changed() {
        let source = "float a = pow(2.0, 3.0);\n";
        assert_eq!(normalize(source), source);
    }

    /// A constructor is a vector as well.
    #[test]
    fn a_constructor_argument_is_promoted() {
        let source = "float a = pow(vec3(1.0), 2.0);\n";
        let normalized = normalize(source);
        assert!(normalized.contains("pow(vec3(1.0), vec3(2.0))"));
    }

    /// An expression that holds a vector is a vector as well.
    #[test]
    fn an_expression_argument_is_promoted() {
        let source = "vec3 srgb(vec3 v) {\n\
             \treturn pow((v + 0.055) / 1.055, 2.4);\n\
             }\n";
        let normalized = normalize(source);
        assert!(normalized.contains("pow((v + 0.055) / 1.055, vec3(2.4))"));
    }

    /// The `mul` call of a macro body is rewritten as well: the macro would
    /// expand to a call naga does not know.
    #[test]
    fn the_mul_of_a_macro_body_is_rewritten() {
        let source = "#define M_MVP mul(g_ModelMatrix, g_ViewProjectionMatrix)\n";
        let normalized = normalize(source);
        assert!(normalized.contains("#define M_MVP (g_ModelMatrix) * (g_ViewProjectionMatrix)"));
    }

    /// The definition of an engine macro does not clash with the value the
    /// pipeline hands to naga: the definition is wrapped in an `#ifndef`.
    #[test]
    fn the_engine_macro_does_not_clash_with_the_value_of_the_pipeline() {
        let source = "#version 450\n\
             layout(binding=0, std140) uniform EffectParams {\n\
             \tmat4x3 [BONECOUNT] g_Bones;\n\
             };\n\
             out vec4 _fragColor;\n\
             void main() { _fragColor = vec4(0.0); }\n";
        let normalized = normalize(source);

        let mut options = naga::front::glsl::Options {
            stage: naga::ShaderStage::Fragment,
            defines: Default::default(),
        };
        // The material of the effect says how many bones there are.
        options
            .defines
            .insert("BONECOUNT".to_string(), "4".to_string());

        let module = naga::front::glsl::Frontend::default()
            .parse(&options, &normalized)
            .expect("the shader compiles");
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator.validate(&module).expect("the shader validates");
    }

    /// The shaders of the engine compile once they are normalised.
    #[test]
    fn the_real_shaders_compile() {
        for (name, source, stage) in FIXTURES {
            let normalized = normalize(source);
            if let Some(error) = compile_error(&normalized, *stage) {
                panic!("the shader '{name}' does not compile:\n{error}\n---\n{normalized}");
            }
        }
    }

    /// The shaders of the engine do not compile before they are normalised:
    /// the dialect is what makes them work.
    #[test]
    fn the_real_shaders_do_not_compile_before_the_normalisation() {
        for (name, source, stage) in FIXTURES {
            if *name == "shadow_sampler.frag" {
                // The pipeline binds the shadow map as an ordinary texture,
                // so this one compiles as it is.
                continue;
            }
            assert!(
                compile_error(source, *stage).is_some(),
                "the shader '{name}' compiles without the dialect"
            );
        }
    }
}
