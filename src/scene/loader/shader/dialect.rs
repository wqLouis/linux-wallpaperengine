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
//! 1. The built-in functions the engine provides and the assets do not
//!    ship are added (`ENGINE_BUILTINS`), so that the shaders that call
//!    them compile.
//! 2. The engine macros the pipeline does not provide are defined
//!    (`ENGINE_MACROS`), so that the shaders that use them compile.
//! 3. `mul(a, b)` becomes `a * b` (the `rewrite_mul` function).  Wallpaper
//!    Engine uses the row vector form of the product, so the operands are
//!    *not* swapped the way the DirectX `mul` would need them.  A nested
//!    call is rewritten from the inside out and a swizzle that follows the
//!    call is kept.
//! 4. The casts of the dialect become the constructors of GLSL:
//!    `CAST3(x)` becomes `vec3(x)` (the `rewrite_casts` function).
//! 5. The precision qualifiers `lowp`, `mediump` and `highp` are removed.
//!    They are legal GLSL, but naga's front end does not accept them in the
//!    uniform blocks the pipeline emits.
//! 6. The sampler types the engine has no binding for are approximated:
//!    `sampler2DComparison`, `sampler3D` and `sampler2DBackBuffer` become
//!    `sampler2D`.
//! 7. The built-in calls that want a vector are given one: a scalar
//!    argument is promoted to a vector of the size of the other arguments
//!    (`pow(v, 2.0)` becomes `pow(v, vec3(2.0))`).  The signatures of the
//!    built-ins are in the `SIGNATURES` table.
//! 8. The variables that are initialised with a scalar are given one: the
//!    initialiser of a vector is wrapped in a constructor of the type of
//!    the variable (`vec2 v = 1;` becomes `vec2 v = vec2(1);`).
//! 9. The built-in globals the engine spells differently are rewritten
//!    (`gl_InstanceID` becomes `gl_InstanceIndex`).  A declaration of such
//!    a name is dropped: it is a built-in of GLSL and not an attribute of
//!    the shader.
//! 10. The functions the engine spells differently are rewritten
//!     (`lerp(a, b, t)` becomes `mix(a, b, t)`, the `FUNCTION_ALIASES`
//!     table).  The rewrite happens before the promotion of the scalar
//!     arguments, so that the arguments of the call are promoted the way
//!     the arguments of the GLSL function are promoted.
//! 11. The functions whose parameter is an array are rewritten: naga has
//!     no array parameters, so the parameter becomes a scalar and a call
//!     that passes an array passes its first element.
//! 12. The calls of the textures the engine builds with a sampler are
//!     rewritten: the sampler of the constructor is the sampler of the
//!     pipeline and the coordinate is truncated to the number of the
//!     components the texture takes.
//! 13. The longer operand of a binary operation is truncated to the number
//!     of the components of the shorter one: the type aware stage cannot
//!     see the type of a macro, because the macros are expanded by the
//!     shader compiler, after the type aware stage has run.
//! 14. The uniforms the engine injects into an effect are declared for
//!     the shaders that use them (`ENGINE_UNIFORMS`); the declaration is a
//!     member of the block of the effect.
//!
//! Every rule reaches the body of the `#define` macros as well: a macro is
//! expanded by the shader compiler, so a construct of the dialect inside of
//! a macro body breaks the shader as well.  The parameters of a macro have
//! no precedence, so the operands of a macro body are bracketed.
//!
//! Legal GLSL is left alone: every rule only fires on the constructs listed
//! above.

use std::collections::BTreeMap;

use super::glsl::expr::Expr;
use super::glsl::types::Type;

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
///
/// The macros are `pub(crate)`: the preprocessing needs to know which
/// macros the dialect stage provides, see [`external_defines`].
pub(crate) const ENGINE_MACROS: &[(&str, &str)] = &[
    ("BONECOUNT", "1"),
    ("BLENDROWCOUNT", "1"),
    ("LIGHTS_DIRECTIONAL", "1"),
    ("LIGHTS_POINT", "1"),
    ("LIGHTS_SPOT", "1"),
    ("LIGHTS_TUBE", "1"),
    // The format of a texture is a property of the material of the
    // effect.  The pipeline does not know it, so it is the format of the
    // default texture of the engine: `FORMAT_RGBA8888`, which is the zero
    // of the format constants of the engine.
    ("TEX0FORMAT", "0"),
    ("TEX1FORMAT", "0"),
    ("TEX2FORMAT", "0"),
    ("TEX3FORMAT", "0"),
    ("TEX4FORMAT", "0"),
    ("TEX5FORMAT", "0"),
    ("TEX6FORMAT", "0"),
    ("TEX7FORMAT", "0"),
    ("TEX8FORMAT", "0"),
    ("TEX9FORMAT", "0"),
];

/// The casts of the dialect and the GLSL constructors they stand for.
///
/// The engine writes `CAST3(x)` where GLSL writes `vec3(x)`.  A cast is a
/// constructor: it is a call with one argument.
///
/// The engine defines the casts as macros of its own compiler
/// (`#define CAST3(x) ((float3)(x))`), so the shaders use them without
/// the assets shipping a definition.
const CASTS: &[(&str, &str)] = &[
    ("CAST2", "vec2"),
    ("CAST3", "vec3"),
    ("CAST4", "vec4"),
    ("CAST3X3", "mat3"),
    ("CAST4U", "uvec4"),
    ("CASTF", "float"),
    ("CASTI", "int"),
    ("CASTU", "uint"),
];

/// The functions of the engine that GLSL spells differently.
///
/// The engine writes `lerp(a, b, t)` where GLSL writes `mix(a, b, t)`.
/// The name of the function is rewritten in a call only: a shader that
/// defines a function of that name keeps it, because the shader knows what
/// its own function does.
///
/// The rewrite happens before the promotion of the scalar arguments, so
/// that the arguments of the call are promoted the way the arguments of
/// the GLSL function are promoted: `lerp(v, v, 0.5)` becomes
/// `mix(v, v, vec3(0.5))`.
const FUNCTION_ALIASES: &[(&str, &str)] = &[("lerp", "mix")];

/// The functions the engine provides and the assets do not ship.
///
/// The engine has a library of built-in functions: its shaders call them
/// without declaring them, because the compiler of the engine adds them to
/// every shader.  They live in the engine itself and not in any header of
/// the assets, so the pipeline has to provide them - or the shaders that
/// call them do not compile.
///
/// The approximation is visible: the pipeline does not bind the lights of
/// the scene yet, so the light of the shader is the light of the ambient.
/// The response of the audio is zero for the same reason: the pipeline
/// does not have the audio of the scene, so the spectrum the engine would
/// read is empty.
const ENGINE_BUILTINS: &[(&str, &str)] = &[
    (
        "PerformLighting_V1",
        "vec3 PerformLighting_V1(vec3 worldPos, vec3 color, vec3 normal, vec3 viewVector, vec3 specularTint, vec3 ambient, float roughness, float metallic)\n\
         {\n\
         \treturn vec3(0.0);\n\
         }\n",
    ),
    (
        "CreateAudioResponse",
        "float CreateAudioResponse(float bufferLeft, float bufferRight)\n\
         {\n\
         \treturn 0.0;\n\
         }\n",
    ),
];

/// The uniforms the engine injects into an effect and the shaders use.
///
/// Some effects of the workshop use a uniform that the engine hands to the
/// shader and that the shader does not declare: `res` (the resolution of
/// the screen), `kernel` (the radius of the gaussian kernel) and
/// `kernelSampleCount` (the number of the samples of the kernel).  naga
/// reports an unknown variable for a name that is used and not declared, so
/// the pipeline declares the ones the shader uses.
///
/// The declaration is a uniform of the block of the effect: the layout of
/// the effect has room for it and the material of the effect can write it.
/// A shader that declares the name itself keeps its own declaration, and a
/// shader that does not use the name does not get it: the layout would
/// carry a uniform nobody needs.
const ENGINE_UNIFORMS: &[(&str, &str)] = &[
    ("res", "vec2"),
    ("kernel", "int"),
    ("kernelSampleCount", "int"),
];

/// The number of the coordinates a texture of a sampler type takes.
///
/// The engine samples a texture with a coordinate that can be wider than
/// the texture needs: the engine truncates it, naga does not.  The number
/// of the components of the coordinate of a texture is the number of the
/// coordinates the texture takes.
const SAMPLER_COORDINATES: &[(&str, u8)] = &[
    ("sampler2D", 2),
    ("sampler2DArray", 3),
    ("sampler3D", 3),
    ("samplerCube", 3),
];

/// The functions that sample a texture, with the number of their arguments.
///
/// The number is the number of the arguments of the call without a bias: a
/// call with more arguments has a bias, and the coordinate of such a call
/// is a `vec3` that naga does not truncate.
const TEXTURE_FUNCTIONS: &[(&str, usize)] = &[("texture", 2), ("textureLod", 3)];

/// The name of the sampler the pipeline binds for every texture.
///
/// The sampler of the pipeline is a binding of its own: the textures of
/// the effect are bound as textures and the sampler of the engine is bound
/// as the sampler of all of them.
pub(crate) const PIPELINE_SAMPLER: &str = "_wm_sampler";

/// The built-in globals the engine spells differently.
///
/// The engine writes the index of the instance as `gl_InstanceID`, GLSL
/// has `gl_InstanceIndex`.  The shaders of the engine may also declare
/// the name as an attribute - in HLSL it is a built-in of the compiler -
/// so the declaration is dropped as well: the instance index is a built-in
/// of GLSL and not an attribute of the shader, and an attribute without a
/// location of its own collides with the first attribute of the shader.
const BUILTIN_GLOBALS: &[(&str, &str)] = &[("gl_InstanceID", "gl_InstanceIndex")];

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
/// The promotion happens in *every* position: the engine broadcasts a
/// scalar to the type of the other arguments, so `mix(v, v, 0.5)` becomes
/// `mix(v, v, vec3(0.5))` and not only the first argument of a call is
/// ever promoted.
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
    ("dot", &[&[Param::Vector, Param::Vector]]),
    (
        "faceforward",
        &[&[Param::Vector, Param::Vector, Param::Vector]],
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
            &[Param::Vector, Param::Vector],
        ],
    ),
    (
        "min",
        &[
            &[Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Scalar],
            &[Param::Vector, Param::Vector],
        ],
    ),
    (
        "mix",
        &[
            &[Param::Scalar, Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Vector, Param::Vector],
        ],
    ),
    (
        "mod",
        &[
            &[Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Scalar],
            &[Param::Vector, Param::Vector],
        ],
    ),
    (
        "pow",
        &[
            &[Param::Scalar, Param::Scalar],
            &[Param::Vector, Param::Vector],
        ],
    ),
    ("reflect", &[&[Param::Vector, Param::Vector]]),
    ("refract", &[&[Param::Vector, Param::Vector, Param::Scalar]]),
    (
        "smoothstep",
        &[
            &[Param::Scalar, Param::Scalar, Param::Scalar],
            &[Param::Scalar, Param::Scalar, Param::Vector],
            &[Param::Vector, Param::Vector, Param::Vector],
        ],
    ),
    (
        "step",
        &[
            &[Param::Scalar, Param::Scalar],
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
    ("sampler2DBackBuffer", "sampler2D"),
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
    /// The type of every variable the shader declares outside of a function
    /// body: the uniforms, the globals and the parameters of the
    /// functions.
    types: BTreeMap<String, Ty>,
    /// The type of every variable a function body declares.
    ///
    /// A local variable shadows the declaration of the same name outside of
    /// the body it is declared in: the parameter of a function of a header
    /// and the local of another function are not the same variable, and the
    /// engine uses the same names over and over again.
    locals: BTreeMap<String, Ty>,
}

impl Symbols {
    /// Declare a name outside of a function body.
    ///
    /// A name that is declared with more than one type is not known: the
    /// dialect must not guess which of the types is the right one.
    fn declare(&mut self, name: &str, ty: Ty) {
        let declared = self.types.entry(name.to_string()).or_insert(ty);
        if *declared != ty {
            *declared = Ty::Unknown;
        }
    }

    /// Declare a name inside of a function body.
    fn declare_local(&mut self, name: &str, ty: Ty) {
        let declared = self.locals.entry(name.to_string()).or_insert(ty);
        if *declared != ty {
            *declared = Ty::Unknown;
        }
    }

    /// The type of a name, if the shader declares it.
    ///
    /// A local variable shadows the declaration of the same name outside of
    /// the function body it is declared in.
    fn lookup(&self, name: &str) -> Option<Ty> {
        self.locals
            .get(name)
            .or_else(|| self.types.get(name))
            .copied()
    }
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
    let source = insert_engine_builtins(source, name);
    let source = define_engine_macros(&source);
    let source = rewrite_function_aliases(&source);
    let source = rewrite_mul(&source);
    let source = rewrite_casts(&source);
    let source = strip_precision_qualifiers(&source);
    let source = rewrite_sampler_types(&source, name);
    let source = promote_scalar_arguments(&source);
    let source = promote_scalar_initialisers(&source);
    let source = rewrite_array_parameters(&source);
    let source = rewrite_texture_calls(&source);
    let source = truncate_binary_operands(&source);
    rewrite_builtin_globals(&source)
}

/// Add the built-in functions the engine provides and the assets do not.
///
/// A function of the table is added when the shader calls it and does not
/// define it: a shader that has its own definition of the function keeps
/// it.  The functions are added after the `#version` and the `#extension`
/// directives, because the version of the shader has to be the first thing
/// in the file, and before the code of the shader, because a function has
/// to be declared before it is called.
///
/// The rule is idempotent: the function of the table is a definition, so a
/// source that has it already is left alone.
fn insert_engine_builtins(source: &str, name: &str) -> String {
    let mask = CodeMask::with_macro_bodies(source);
    let tokens = tokenize(source, &mask);

    let mut block = String::new();
    for (function, implementation) in ENGINE_BUILTINS {
        let called = tokens
            .iter()
            .any(|token| token.kind == TokenKind::Word && token.text == *function);
        if !called || defines_function(&tokens, function) {
            continue;
        }
        log::warn!(
            "shader '{}': the built-in function '{}' of the engine is not in any header of the assets, the pipeline provides an approximation of it",
            name,
            function
        );
        block.push_str(implementation);
    }

    if block.is_empty() {
        return source.to_string();
    }

    let insertion = macro_insertion_point(source);
    let mut result = String::with_capacity(source.len() + block.len());
    result.push_str(&source[..insertion]);
    result.push_str(&block);
    result.push_str(&source[insertion..]);
    result
}

/// Whether the source defines the function `name`.
///
/// A definition is a type followed by the name and a parenthesis:
/// `vec3 PerformLighting_V1(...)`.  A call is not a definition: the token
/// before the name of a call is an operator or a `;`.
fn defines_function(tokens: &[Token<'_>], name: &str) -> bool {
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Word || token.text != name {
            continue;
        }
        let Some(previous) = index.checked_sub(1).and_then(|index| tokens.get(index)) else {
            continue;
        };
        if previous.kind != TokenKind::Word || keyword_type(previous.text).is_none() {
            continue;
        }
        let Some(next) = tokens.get(index + 1) else {
            continue;
        };
        if next.kind == TokenKind::Punct && next.text == "(" {
            return true;
        }
    }
    false
}

/// Rewrite the calls of the functions the engine spells differently.
///
/// `lerp(a, b, t)` becomes `mix(a, b, t)`.  Only a call is rewritten: a
/// shader that defines a function of that name keeps it, because the
/// shader knows what its own function does.
///
/// The rewrite happens before the promotion of the scalar arguments, so
/// that the arguments of the call are promoted the way the arguments of
/// the GLSL function are promoted: `lerp(v, v, 0.5)` becomes
/// `mix(v, v, vec3(0.5))`.
fn rewrite_function_aliases(source: &str) -> String {
    let mask = CodeMask::with_macro_bodies(source);
    let tokens = tokenize(source, &mask);

    let mut edits: Vec<Edit> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Word {
            continue;
        }
        let Some((_, replacement)) = FUNCTION_ALIASES
            .iter()
            .find(|(name, _)| *name == token.text)
        else {
            continue;
        };
        // Only a call is rewritten: the name is followed by a parenthesis.
        if punct_after(&tokens, index, "(").is_none() {
            continue;
        }
        // A definition of the function keeps its name.
        if defines_function(&tokens, token.text) {
            continue;
        }
        edits.push(Edit {
            start: token.start,
            end: token.end,
            text: (*replacement).to_string(),
        });
    }

    apply_edits(source, edits)
}

/// A function of the source whose parameter is an array.
struct ArrayFunction {
    /// The name of the function.
    name: String,
    /// The parameters of the function that are an array: the name of the
    /// parameter and the range of its array specifier (`[16]`).
    parameters: Vec<(String, usize, usize)>,
    /// The range of the body of the function.
    body: (usize, usize),
}

/// Truncate the longer operand of a binary operation.
///
/// The engine truncates the longer operand of a binary operation to the
/// number of the components of the shorter one; GLSL does not, and naga
/// rejects the operation ("Operation Multiply can't work with
/// Vector{Quad} and Vector{Bi}").
///
/// The type aware stage has that rule already, but it cannot see the type
/// of a macro: the macros are expanded by the shader compiler, after the
/// type aware stage has run.  This rule is the same rule, with the types
/// of the macros of the source added.
fn truncate_binary_operands(source: &str) -> String {
    let macros = defined_macro_bodies(source);
    if macros.is_empty() {
        // Without a macro there is nothing the type aware stage could not
        // have seen.
        return source.to_string();
    }

    let unit = super::glsl::parser::parse(source);
    let symbols = binary_symbols(source, &unit, &macros);

    let mut edits: Vec<Edit> = Vec::new();
    visit_expressions(&unit, &mut |expr| {
        let Expr::Binary { lhs, rhs, .. } = expr else {
            return;
        };
        let lhs_type = super::glsl::infer::infer(lhs, &symbols);
        let rhs_type = super::glsl::infer::infer(rhs, &symbols);
        let (Type::Vector(_, lhs_width), Type::Vector(_, rhs_width)) = (lhs_type, rhs_type) else {
            return;
        };
        if lhs_width == rhs_width {
            return;
        }

        // The longer operand is truncated to the number of the components
        // of the shorter one.
        let (operand, width) = if lhs_width > rhs_width {
            (lhs, rhs_width)
        } else {
            (rhs, lhs_width)
        };
        let span = operand.span();
        let rewritten = super::glsl::fixups::Rewritten {
            text: span.text(source).to_string(),
            ty: super::glsl::infer::infer(operand, &symbols),
            needs_parens: needs_parens(operand),
            swizzle: swizzle_of(operand, source),
        };
        let Some((truncated, _)) = super::glsl::fixups::truncate_text(&rewritten, width) else {
            return;
        };
        edits.push(Edit {
            start: span.start,
            end: span.end,
            text: truncated,
        });
    });

    apply_edits(source, edits)
}

/// The symbols the binary rule needs: the globals of the shader and the
/// macros it defines.
fn binary_symbols(
    source: &str,
    unit: &super::glsl::parser::TranslationUnit,
    macros: &BTreeMap<String, String>,
) -> super::glsl::symbols::SymbolTable {
    let mut symbols = super::glsl::symbols::SymbolTable::new();

    // The declarations of the source: the varyings and the uniforms that
    // the shader declares with a qualifier the parser does not know.
    let own = collect_symbols(source, &CodeMask::new(source));
    for (name, ty) in own.types.iter().chain(own.locals.iter()) {
        symbols.declare_global(
            name,
            glsl_type(*ty),
            super::glsl::symbols::SymbolKind::Uniform,
            false,
        );
    }

    for global in &unit.globals {
        match global {
            super::glsl::parser::GlobalDecl::Variable { ty, name, is_array } => {
                symbols.declare_global(
                    name,
                    *ty,
                    super::glsl::symbols::SymbolKind::Attribute,
                    *is_array,
                );
            }
            super::glsl::parser::GlobalDecl::InterfaceBlock { members } => {
                for member in members {
                    symbols.declare_global(
                        &member.name,
                        member.ty,
                        super::glsl::symbols::SymbolKind::Uniform,
                        member.is_array,
                    );
                }
            }
            super::glsl::parser::GlobalDecl::Opaque => {}
        }
    }

    // The macros of the source: their type is the type of their body.
    for (name, body) in macros {
        let ty = estimate_type(body, &own);
        symbols.declare_global(
            name,
            glsl_type(ty),
            super::glsl::symbols::SymbolKind::Uniform,
            false,
        );
    }

    symbols
}

/// The type of the dialect stage as the type of the type aware stage.
fn glsl_type(ty: Ty) -> super::glsl::types::Type {
    match ty {
        Ty::Scalar => super::glsl::types::Type::Scalar(super::glsl::types::Scalar::Float),
        Ty::Vector(size) => {
            super::glsl::types::Type::Vector(super::glsl::types::Scalar::Float, size)
        }
        Ty::Matrix | Ty::Array | Ty::Opaque => super::glsl::types::Type::Opaque,
        Ty::Unknown => super::glsl::types::Type::Unknown,
    }
}

/// Whether a swizzle can be appended to an expression or not.
///
/// A swizzle binds tighter than an operator, so the expression has to be
/// parenthesised first.
fn needs_parens(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Unary { .. } | Expr::Binary { .. } | Expr::Ternary { .. }
    )
}

/// The swizzle an expression is, if it is one.
fn swizzle_of(expr: &Expr, source: &str) -> Option<super::glsl::fixups::SwizzleInfo> {
    let Expr::Swizzle { base, comps, .. } = expr else {
        return None;
    };
    Some(super::glsl::fixups::SwizzleInfo {
        base: base.span().text(source).to_string(),
        comps: comps.clone(),
    })
}

/// The macros the source defines, with the body of the macro.
///
/// A function like macro is not one of them: its parameters are substituted
/// without any precedence, and the body of the macro is not a value.
fn defined_macro_bodies(source: &str) -> BTreeMap<String, String> {
    let mut macros = BTreeMap::new();

    for line in source.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("#define ") else {
            continue;
        };
        let rest = rest.trim_start();
        let name: String = rest
            .chars()
            .take_while(|character| character.is_alphanumeric() || *character == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        let body = rest[name.len()..].trim_start();
        // A function like macro: the parenthesis follows the name of the
        // macro without a space between them.
        if rest[name.len()..].starts_with('(') {
            continue;
        }
        macros.insert(name, body.to_string());
    }

    macros
}

/// Visit every expression of a translation unit.
fn visit_expressions(unit: &super::glsl::parser::TranslationUnit, visit: &mut impl FnMut(&Expr)) {
    for function in &unit.functions {
        visit_block(&function.body, visit);
    }
}

/// Visit every expression of a block.
fn visit_block(block: &super::glsl::parser::Block, visit: &mut impl FnMut(&Expr)) {
    for statement in &block.stmts {
        visit_statement(statement, visit);
    }
}

/// Visit every expression of a statement.
fn visit_statement(statement: &super::glsl::parser::Stmt, visit: &mut impl FnMut(&Expr)) {
    use super::glsl::parser::Stmt;

    match statement {
        Stmt::Decl { init, .. } => {
            if let Some(init) = init {
                visit_expression(init, visit);
            }
        }
        Stmt::Assign { target, value, .. } => {
            visit_expression(target, visit);
            visit_expression(value, visit);
        }
        Stmt::Expr(expr) => visit_expression(expr, visit),
        Stmt::If {
            cond,
            then_block,
            else_block,
        } => {
            visit_expression(cond, visit);
            visit_block(then_block, visit);
            if let Some(else_block) = else_block {
                visit_block(else_block, visit);
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(init) = init {
                visit_statement(init, visit);
            }
            if let Some(cond) = cond {
                visit_expression(cond, visit);
            }
            if let Some(step) = step {
                visit_expression(step, visit);
            }
            visit_block(body, visit);
        }
        Stmt::While { cond, body } => {
            visit_expression(cond, visit);
            visit_block(body, visit);
        }
        Stmt::DoWhile { cond, body } => {
            visit_expression(cond, visit);
            visit_block(body, visit);
        }
        Stmt::Return(expr) => {
            if let Some(expr) = expr {
                visit_expression(expr, visit);
            }
        }
        Stmt::Block(block) => visit_block(block, visit),
        Stmt::Jump | Stmt::Opaque => {}
    }
}

/// Visit every expression of an expression.
fn visit_expression(expr: &Expr, visit: &mut impl FnMut(&Expr)) {
    visit(expr);

    match expr {
        Expr::Literal { .. } | Expr::Ident { .. } | Expr::Error { .. } => {}
        Expr::Swizzle { base, .. } => visit_expression(base, visit),
        Expr::Call { args, .. } => {
            for argument in args {
                visit_expression(argument, visit);
            }
        }
        Expr::Unary { operand, .. } => visit_expression(operand, visit),
        Expr::Binary { lhs, rhs, .. } => {
            visit_expression(lhs, visit);
            visit_expression(rhs, visit);
        }
        Expr::Ternary {
            cond,
            then_expr,
            other,
            ..
        } => {
            visit_expression(cond, visit);
            visit_expression(then_expr, visit);
            visit_expression(other, visit);
        }
        Expr::Paren { inner, .. } => visit_expression(inner, visit),
        Expr::Index { base, index, .. } => {
            visit_expression(base, visit);
            visit_expression(index, visit);
        }
    }
}

/// Rewrite the functions whose parameter is an array.
///
/// GLSL has array parameters, but naga does not: a call that passes an
/// array to a function is an "unknown function" for naga.  The parameter
/// becomes a scalar - the value of the array is not known at the call site
/// either - and the accesses to it in the body of the function become the
/// scalar.  A call that passes an array passes its first element.
fn rewrite_array_parameters(source: &str) -> String {
    let mask = CodeMask::with_macro_bodies(source);
    let tokens = tokenize(source, &mask);

    let functions = array_functions(&tokens);
    if functions.is_empty() {
        return source.to_string();
    }
    let symbols = collect_symbols(source, &CodeMask::new(source));

    let mut edits: Vec<Edit> = Vec::new();
    for function in &functions {
        // The array specifier of the parameter is dropped: the parameter
        // is a scalar now.
        for (_, start, end) in &function.parameters {
            edits.push(Edit {
                start: *start,
                end: *end,
                text: String::new(),
            });
        }
        // The accesses to the parameter in the body of the function become
        // the scalar itself.
        for (name, _, _) in &function.parameters {
            edits.extend(array_accesses(&tokens, function.body, name));
        }
        // A call that passes an array to the function passes its first
        // element.
        for (index, token) in tokens.iter().enumerate() {
            if token.kind != TokenKind::Word || token.text != function.name {
                continue;
            }
            // A definition of the function is not a call.
            if is_type_preceded(&tokens, index) {
                continue;
            }
            let Some(open) = punct_after(&tokens, index, "(") else {
                continue;
            };
            let Some(close) = matching_paren(source, &mask, open) else {
                continue;
            };
            for argument in argument_ranges(source, &mask, open + 1, close) {
                let text = &source[argument.0..argument.1];
                let trimmed = text.trim();
                if !is_name(trimmed) || symbols.lookup(trimmed) != Some(Ty::Array) {
                    continue;
                }
                // The space in front of the argument belongs to it.
                let space = &text[..text.len() - text.trim_start().len()];
                edits.push(Edit {
                    start: argument.0,
                    end: argument.1,
                    text: format!("{space}{trimmed}[0]"),
                });
            }
        }
    }

    apply_edits(source, edits)
}

/// The functions of the source whose parameter is an array.
fn array_functions(tokens: &[Token<'_>]) -> Vec<ArrayFunction> {
    let mut functions = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Word {
            continue;
        }
        // A definition of a function: a type, the name of the function and
        // a parenthesis.
        if !is_type_preceded(tokens, index) {
            continue;
        }
        let Some(open) = punct_after(tokens, index, "(") else {
            continue;
        };
        let open = tokens
            .iter()
            .position(|token| token.start == open)
            .unwrap_or(index + 1);
        let Some(close) = closing_token(tokens, open) else {
            continue;
        };

        // The parameters that are an array: a name and an array specifier.
        let mut parameters = Vec::new();
        let mut cursor = open + 1;
        while cursor < close {
            let current = tokens[cursor];
            if current.kind == TokenKind::Punct && current.text == "[" {
                let Some(name) = cursor.checked_sub(1).and_then(|index| tokens.get(index)) else {
                    break;
                };
                if name.kind == TokenKind::Word {
                    let Some(end) = closing_token(tokens, cursor) else {
                        break;
                    };
                    parameters.push((name.text.to_string(), current.start, tokens[end].end));
                    cursor = end + 1;
                    continue;
                }
            }
            cursor += 1;
        }
        if parameters.is_empty() {
            continue;
        }

        // The body of the function: the brace behind the parenthesis.
        let Some(body) = tokens.get(close + 1) else {
            continue;
        };
        if body.kind != TokenKind::Punct || body.text != "{" {
            continue;
        }
        let Some(end) = closing_token(tokens, close + 1) else {
            continue;
        };

        functions.push(ArrayFunction {
            name: token.text.to_string(),
            parameters,
            body: (body.start, tokens[end].end),
        });
    }

    functions
}

/// The accesses to an array in the body of a function.
fn array_accesses(tokens: &[Token<'_>], body: (usize, usize), name: &str) -> Vec<Edit> {
    let mut edits = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Word || token.text != name {
            continue;
        }
        if token.start < body.0 || token.end > body.1 {
            continue;
        }
        // The name is followed by a bracket: it is an access to the array.
        let Some(next) = tokens.get(index + 1) else {
            continue;
        };
        if next.kind != TokenKind::Punct || next.text != "[" {
            continue;
        }
        let Some(close) = closing_token(tokens, index + 1) else {
            continue;
        };
        edits.push(Edit {
            start: token.start,
            end: tokens[close].end,
            text: name.to_string(),
        });
    }

    edits
}

/// Whether the name of the token `index` follows the type of a declaration.
fn is_type_preceded(tokens: &[Token<'_>], index: usize) -> bool {
    let Some(previous) = index.checked_sub(1).and_then(|index| tokens.get(index)) else {
        return false;
    };
    previous.kind == TokenKind::Word && keyword_type(previous.text).is_some()
}

/// The index of the token that closes the bracket of the token `open`.
fn closing_token(tokens: &[Token<'_>], open: usize) -> Option<usize> {
    let mut depth = 0i32;

    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token.kind != TokenKind::Punct {
            continue;
        }
        match token.text {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

/// Whether a text is the name of a variable.
fn is_name(text: &str) -> bool {
    let mut characters = text.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if first.is_ascii_digit() {
        return false;
    }
    text.chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// The constructor of a sampler: the texture and the sampler it combines.
struct SamplerConstructor<'a> {
    /// The type of the sampler.
    sampler_type: &'a str,
    /// The texture of the constructor.
    texture: &'a str,
}

/// The constructor of a sampler, if the text is one.
///
/// The engine builds a sampler with a constructor:
/// `sampler2D(g_Texture0, _wm_sampler)` combines the texture and the
/// sampler into the sampler the call samples with.
fn sampler_constructor(text: &str) -> Option<SamplerConstructor<'_>> {
    let mask = CodeMask::new(text);
    let tokens = tokenize(text, &mask);

    if tokens.len() < 2 || tokens[0].kind != TokenKind::Word {
        return None;
    }
    if tokens[1].kind != TokenKind::Punct || tokens[1].text != "(" {
        return None;
    }
    let close = closing_token(&tokens, 1)?;
    let arguments = argument_ranges(text, &mask, tokens[1].end, tokens[close].start);
    if arguments.len() != 2 {
        return None;
    }

    Some(SamplerConstructor {
        sampler_type: tokens[0].text,
        texture: text[arguments[0].0..arguments[0].1].trim(),
    })
}

/// Rewrite the calls of the textures the engine builds with a sampler.
///
/// The engine writes `texture(sampler2D(g_Texture0, _wm_sampler), uv)`: the
/// sampler is a constructor that combines the texture and the sampler.
/// GLSL has no such constructor - the sampler is a separate binding - so
/// the call is rewritten into the form the pipeline uses: the texture of
/// the constructor and the sampler of the pipeline.
///
/// The coordinate of the call is truncated to the number of the components
/// the texture takes: the engine truncates it, naga does not, and a
/// coordinate that is too wide is an "unknown function" for naga.
///
/// A call whose first argument is not a constructor is not touched: the
/// plain `texture(s, uv)` of the compat stage and a legal GLSL sampler stay
/// as they are.
fn rewrite_texture_calls(source: &str) -> String {
    let mask = CodeMask::with_macro_bodies(source);
    let symbols = collect_symbols(source, &CodeMask::new(source));
    let tokens = tokenize(source, &mask);

    let mut edits: Vec<Edit> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Word {
            continue;
        }
        let Some((_, arguments_of_the_call)) = TEXTURE_FUNCTIONS
            .iter()
            .find(|(name, _)| *name == token.text)
        else {
            continue;
        };
        let Some(open) = punct_after(&tokens, index, "(") else {
            continue;
        };
        let Some(close) = matching_paren(source, &mask, open) else {
            continue;
        };
        let arguments = argument_ranges(source, &mask, open + 1, close);
        // A call with more arguments has a bias: its coordinate is a
        // `vec3` that naga does not truncate.
        if arguments.len() != *arguments_of_the_call {
            continue;
        }

        // The first argument is the combined form: a constructor that
        // makes a sampler out of a texture and a sampler.
        let first = source[arguments[0].0..arguments[0].1].trim();
        let Some(constructor) = sampler_constructor(first) else {
            continue;
        };
        let Some((_, components)) = SAMPLER_COORDINATES
            .iter()
            .find(|(name, _)| *name == constructor.sampler_type)
        else {
            continue;
        };

        // The call samples the texture of the constructor with the sampler
        // of the pipeline.
        edits.push(Edit {
            start: arguments[0].0,
            end: arguments[0].1,
            text: format!("sampler2D({}, {})", constructor.texture, PIPELINE_SAMPLER),
        });

        // The coordinate is truncated to the number of the components the
        // texture takes.
        let text = &source[arguments[1].0..arguments[1].1];
        let trimmed = text.trim();
        let Ty::Vector(size) = estimate_type(trimmed, &symbols) else {
            continue;
        };
        if size <= *components {
            continue;
        }
        let space = &text[..text.len() - text.trim_start().len()];
        let swizzle = &"xyzw"[..*components as usize];
        let replacement = if is_name(trimmed) {
            format!("{space}{trimmed}.{swizzle}")
        } else {
            format!("{space}({trimmed}).{swizzle}")
        };
        edits.push(Edit {
            start: arguments[1].0,
            end: arguments[1].1,
            text: replacement,
        });
    }

    apply_edits(source, edits)
}

/// The uniforms of the engine that a shader uses and does not declare.
///
/// naga reports an unknown variable for a name that is used and not
/// declared, so the pipeline declares the ones the shader uses.  The
/// declaration is a uniform of the block of the effect: the layout of the
/// effect has room for it and the material of the effect can write it.
///
/// A shader that declares the name itself keeps its own declaration, and a
/// shader that does not use the name does not get it: the layout would
/// carry a uniform nobody needs.
///
/// The macros of the shader are not taken into account: a shader that
/// defines the name as a macro uses the macro, but the uniform is
/// harmless - the macro wins in the body of the shader.
pub(crate) fn engine_uniforms(source: &str) -> Vec<(&'static str, &'static str)> {
    let symbols = collect_symbols(source, &CodeMask::new(source));
    let mask = CodeMask::with_macro_bodies(source);
    let tokens = tokenize(source, &mask);

    let mut uniforms = Vec::new();
    for (name, ty) in ENGINE_UNIFORMS {
        // The shader declares the name itself: it knows what it wants.
        if symbols.lookup(name).is_some() {
            continue;
        }
        // The shader does not use the name: the layout would carry a
        // uniform nobody needs.
        if !tokens
            .iter()
            .any(|token| token.kind == TokenKind::Word && token.text == *name)
        {
            continue;
        }
        uniforms.push((*name, *ty));
    }

    uniforms
}

/// Rewrite the built-in globals the engine spells differently.
///
/// A name that the shader declares as an attribute is dropped: the
/// built-in of GLSL is not an attribute of the shader.  Every other use of
/// the name is rewritten to the name GLSL uses.
fn rewrite_builtin_globals(source: &str) -> String {
    let mask = CodeMask::with_macro_bodies(source);
    let tokens = tokenize(source, &mask);

    let mut edits: Vec<Edit> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Word {
            continue;
        }
        let Some((_, replacement)) = BUILTIN_GLOBALS.iter().find(|(name, _)| *name == token.text)
        else {
            continue;
        };

        // A declaration of the name is not a use of it: the built-in of
        // GLSL is not an attribute, so the declaration is dropped.
        if let Some((start, end)) = declaration_of(&tokens, index) {
            edits.push(Edit {
                start,
                end,
                text: String::new(),
            });
            continue;
        }

        edits.push(Edit {
            start: token.start,
            end: token.end,
            text: (*replacement).to_string(),
        });
    }

    apply_edits(source, edits)
}

/// The range of the declaration whose name is the token `index`.
///
/// The range covers the whole statement, so that the qualifiers of the
/// declaration (`in`, `attribute`, a layout) are dropped with it.
fn declaration_of(tokens: &[Token<'_>], index: usize) -> Option<(usize, usize)> {
    // The type of the declaration is the token before the name.
    let type_token = index.checked_sub(1).and_then(|index| tokens.get(index))?;
    if type_token.kind != TokenKind::Word || keyword_type(type_token.text).is_none() {
        return None;
    }
    // The name is the last token of the declaration: the declaration ends
    // with a semicolon.
    let next = tokens.get(index + 1)?;
    if next.kind != TokenKind::Punct {
        return None;
    }
    if !matches!(next.text, ";" | "," | "[") {
        return None;
    }
    let end = statement_end(tokens, index + 1)?;
    // The qualifiers of the declaration are part of it.
    let start = statement_start(tokens, type_token.start);
    Some((start, end))
}

/// The end of the statement the token `index` is in.
///
/// The statement ends at the semicolon that closes it.
fn statement_end(tokens: &[Token<'_>], index: usize) -> Option<usize> {
    let mut depth = 0i32;
    for token in tokens.iter().skip(index) {
        if token.kind != TokenKind::Punct {
            continue;
        }
        match token.text {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth -= 1,
            ";" if depth == 0 => return Some(token.end),
            _ => {}
        }
    }
    None
}

/// The start of the statement the position `index` is in.
///
/// The statement starts after the last `;`, `{` or `}` before the
/// position, so that the qualifiers of a declaration are part of it.
fn statement_start(tokens: &[Token<'_>], index: usize) -> usize {
    let mut start = 0;
    for token in tokens {
        if token.start >= index {
            break;
        }
        if token.kind == TokenKind::Punct && matches!(token.text, ";" | "{" | "}") {
            start = token.end;
        }
    }
    start
}

/// Promote the initialiser of a vector that is a scalar.
///
/// `vec2 texelSize = 1;` initialises a vector with a scalar.  GLSL has no
/// implicit conversion from a scalar to a vector, so the initialiser is
/// wrapped in a constructor of the type of the variable: `vec2 texelSize =
/// vec2(1);`.
///
/// A variable that is initialised with a vector is left alone: the
/// constructor would only add noise.
fn promote_scalar_initialisers(source: &str) -> String {
    let mask = CodeMask::with_macro_bodies(source);
    // The symbols of the macro bodies are not collected: a macro body is a
    // text that is expanded later, and its parameters are not variables.
    let symbols = collect_symbols(source, &CodeMask::new(source));
    let tokens = tokenize(source, &mask);

    let mut edits: Vec<Edit> = Vec::new();
    for index in 0..tokens.len() {
        let token = &tokens[index];
        if token.kind != TokenKind::Word {
            continue;
        }
        let Some(Ty::Vector(_)) = keyword_type(token.text) else {
            continue;
        };
        // The name of the variable follows its type.
        let Some(name) = tokens.get(index + 1) else {
            continue;
        };
        if name.kind != TokenKind::Word {
            continue;
        }
        // The variable is initialised.
        let Some(assign) = tokens.get(index + 2) else {
            continue;
        };
        if assign.kind != TokenKind::Punct || assign.text != "=" {
            continue;
        }
        // The initialiser is the expression that follows the assignment.
        let Some((start, end)) = initialiser_range(&tokens, index + 3) else {
            continue;
        };
        let text = source[start..end].trim();
        if estimate_type(text, &symbols) != Ty::Scalar {
            continue;
        }
        edits.push(Edit {
            start,
            end,
            text: format!("{}({})", token.text, text),
        });
    }

    apply_edits(source, edits)
}

/// The range of the expression that starts at the token `index`.
///
/// The expression ends at the `;` or the `,` that closes it.
fn initialiser_range(tokens: &[Token<'_>], index: usize) -> Option<(usize, usize)> {
    let first = tokens.get(index)?;
    let mut depth = 0i32;
    let mut last = first;

    for token in tokens.iter().skip(index) {
        if token.kind == TokenKind::Punct {
            match token.text {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth -= 1,
                ";" | "," if depth == 0 => {
                    return Some((first.start, last.end));
                }
                _ => {}
            }
        }
        last = token;
    }

    Some((first.start, last.end))
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
    let mask = CodeMask::with_macro_bodies(text);
    rewrite_mul_region(text, &mask, 0, text.len()).text
}

/// Rewrite the casts of the dialect into the constructors of GLSL.
///
/// `CAST2(x)` becomes `vec2(x)`, `CAST3(x)` becomes `vec3(x)`, `CAST4(x)`
/// becomes `vec4(x)` and `CAST3X3(x)` becomes `mat3(x)`.
///
/// The body of a macro is rewritten as well: a `#define` that holds a cast
/// expands to a call naga does not know.  The cast is a constructor, so it
/// keeps its argument - and with it the precedence of the argument.
pub(crate) fn rewrite_casts(text: &str) -> String {
    let mask = CodeMask::with_macro_bodies(text);
    let tokens = tokenize(text, &mask);

    let mut edits: Vec<Edit> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Word {
            continue;
        }
        let Some((_, replacement)) = CASTS.iter().find(|(name, _)| *name == token.text) else {
            continue;
        };
        // A cast is a constructor: it is a call with an argument.
        let is_call = tokens
            .get(index + 1)
            .is_some_and(|next| next.kind == TokenKind::Punct && next.text == "(");
        if !is_call {
            continue;
        }
        edits.push(Edit {
            start: token.start,
            end: token.end,
            text: (*replacement).to_string(),
        });
    }

    apply_edits(text, edits)
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
    let mask = CodeMask::with_macro_bodies(source);
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
    let mask = CodeMask::with_macro_bodies(source);
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
    let mask = CodeMask::with_macro_bodies(source);
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
    let mask = CodeMask::with_macro_bodies(source);
    // The symbols of the macro bodies are not collected: a macro body is a
    // text that is expanded later, and its parameters are not variables.
    let symbols = collect_symbols(source, &CodeMask::new(source));
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
///
/// The declarations of a function body are the locals: they shadow the
/// declarations of the same name outside of the body.
fn collect_symbols(source: &str, mask: &CodeMask) -> Symbols {
    let tokens = tokenize(source, mask);
    let mut symbols = Symbols::default();
    // The depth of the braces: a declaration inside of a body is a local.
    let mut depth = 0i32;

    for (index, token) in tokens.iter().enumerate() {
        if token.kind == TokenKind::Punct {
            match token.text {
                "{" => depth += 1,
                "}" => depth -= 1,
                _ => {}
            }
            continue;
        }
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

        if depth > 0 {
            symbols.declare_local(name.text, ty);
        } else {
            symbols.declare(name.text, ty);
        }
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
    symbols.lookup(text).unwrap_or(Ty::Unknown)
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
        Self::build(source, Directives::Excluded)
    }

    /// The mask of a source whose macro bodies count as code.
    ///
    /// The body of a macro is code: a `mul` call or a cast of a `#define`
    /// has to be rewritten as well, or the macro expands to a call naga
    /// does not know.  The directive itself is not code: the name of the
    /// macro and its parameters are not code either.
    fn with_macro_bodies(source: &str) -> Self {
        Self::build(source, Directives::Bodies)
    }

    /// Build the mask of a source.
    fn build(source: &str, directives: Directives) -> Self {
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
                // A preprocessor directive: the whole line is not code, the
                // body of a macro is.
                let line_end = source[index..]
                    .find('\n')
                    .map(|offset| index + offset)
                    .unwrap_or(bytes.len());
                for byte in &mut code[index..line_end] {
                    *byte = false;
                }
                if directives == Directives::Bodies
                    && let Some((body_start, body_end)) = macro_body(source, index)
                {
                    let body_end = body_end.min(line_end);
                    for byte in &mut code[body_start..body_end] {
                        *byte = true;
                    }
                }
                index = line_end;
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

/// What the preprocessor directives of a source are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Directives {
    /// The directives are not code: the rules must not touch them.
    Excluded,
    /// The body of a `#define` is code, the rest of the directive is not.
    Bodies,
}

/// The body of the macro of a `#define` directive.
///
/// The directive starts at `start`: the position of its `#`.  The body is
/// the text that follows the name of the macro and its parameters, up to
/// the end of the line.  A directive that is not a `#define` has no body.
fn macro_body(source: &str, start: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut index = start + 1;

    // The name of the directive.
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    let name_start = index;
    while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_') {
        index += 1;
    }
    if &source[name_start..index] != "define" {
        return None;
    }

    // The name of the macro.
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    let macro_start = index;
    while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_') {
        index += 1;
    }
    if index == macro_start {
        return None;
    }

    // The parameters of the macro: a parenthesis that follows the name of
    // the macro directly makes it a function like macro.
    if bytes.get(index) == Some(&b'(') {
        let mut depth = 0i32;
        while index < bytes.len() {
            match bytes[index] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        index += 1;
                        break;
                    }
                }
                b'\n' => break,
                _ => {}
            }
            index += 1;
        }
    }

    // The body of the macro: the rest of the line.
    while index < bytes.len() && bytes[index] != b'\n' && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    let body_start = index;
    let mut body_end = body_start;
    while body_end < bytes.len() && bytes[body_end] != b'\n' {
        body_end += 1;
    }
    // A comment at the end of the line is not part of the body.
    if let Some(offset) = source[body_start..body_end].find("//") {
        body_end = body_start + offset;
    }

    Some((body_start, body_end))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture of the tests: the name of the shader, its source, the
    /// stage it is compiled for and the macros it is compiled with.
    type Fixture = (
        &'static str,
        &'static str,
        naga::ShaderStage,
        &'static [(&'static str, &'static str)],
    );

    /// The shaders of the engine the fixtures are taken from, with the
    /// stage they are compiled for and the macros they are compiled with.
    const FIXTURES: &[Fixture] = &[
        (
            "nested_mul.vert",
            include_str!("tests/nested_mul.vert"),
            naga::ShaderStage::Vertex,
            &[],
        ),
        (
            "pow_scalar.frag",
            include_str!("tests/pow_scalar.frag"),
            naga::ShaderStage::Fragment,
            &[],
        ),
        (
            "precision.frag",
            include_str!("tests/precision.frag"),
            naga::ShaderStage::Fragment,
            &[],
        ),
        (
            "shadow_sampler.frag",
            include_str!("tests/shadow_sampler.frag"),
            naga::ShaderStage::Fragment,
            &[],
        ),
        (
            "instance_index.vert",
            include_str!("tests/instance_index.vert"),
            naga::ShaderStage::Vertex,
            &[],
        ),
        (
            "scalar_initialiser.frag",
            include_str!("tests/scalar_initialiser.frag"),
            naga::ShaderStage::Fragment,
            &[],
        ),
        (
            "engine_macros.vert",
            include_str!("tests/engine_macros.vert"),
            naga::ShaderStage::Vertex,
            &[],
        ),
        // The fragment stage of the pulse effect of the engine.
        (
            "pulse_effect.frag",
            include_str!("tests/pulse_effect.frag"),
            naga::ShaderStage::Fragment,
            &[("BLENDMODE", "9")],
        ),
    ];

    /// The shaders of the engine that compile without the dialect stage.
    ///
    /// The dialect stage must not break them: the fire effect of the engine
    /// mixes two scalars with a scalar, and the dialect stage must not
    /// turn the mix into a mix of vectors.
    const COMPILES_WITHOUT_THE_DIALECT: &[Fixture] = &[(
        "fire_effect.frag",
        include_str!("tests/fire_effect.frag"),
        naga::ShaderStage::Fragment,
        &[("BLENDMODE", "0")],
    )];

    /// The error naga reports for a shader, if it reports one.
    fn compile_error(
        source: &str,
        stage: naga::ShaderStage,
        defines: &[(&str, &str)],
    ) -> Option<String> {
        let options = naga::front::glsl::Options {
            stage,
            defines: defines
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
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
        assert!(normalized.contains("uniform sampler2D g_Texture2;"));
        assert!(!normalized.contains("sampler2DComparison"));
        assert!(!normalized.contains("sampler3D"));
        assert!(!normalized.contains("sampler2DBackBuffer"));
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

    /// The scalar of a `mix` is promoted as well: the engine wants the
    /// three arguments of a `mix` to have the same type.
    #[test]
    fn the_scalar_of_a_mix_is_promoted() {
        let source = "vec3 albedo = v_Color;\nfloat a = mix(albedo, albedo, 0.5);\n";
        let normalized = normalize(source);
        assert!(normalized.contains("mix(albedo, albedo, vec3(0.5))"));
    }

    /// A `mix` of two vectors and a vector is not touched.
    #[test]
    fn a_mix_of_two_vectors_and_a_vector_is_not_changed() {
        let source = "vec3 albedo = v_Color;\nvec3 a = mix(albedo, albedo, albedo);\n";
        assert_eq!(normalize(source), source);
    }

    /// A `min` of a vector and a scalar is not touched: naga accepts it.
    #[test]
    fn a_min_of_a_vector_and_a_scalar_is_not_changed() {
        let source = "vec3 albedo = v_Color;\nfloat a = min(albedo, 0.0);\n";
        assert_eq!(normalize(source), source);
    }

    /// A `clamp` of a vector and two scalars is not touched: naga accepts
    /// it.
    #[test]
    fn a_clamp_of_a_vector_and_two_scalars_is_not_changed() {
        let source = "vec3 albedo = v_Color;\nvec3 a = clamp(albedo, 0.0, 1.0);\n";
        assert_eq!(normalize(source), source);
    }

    /// A `step` of two scalars is not touched.
    #[test]
    fn a_step_of_two_scalars_is_not_changed() {
        let source = "float a = step(0.5, 1.0);\n";
        assert_eq!(normalize(source), source);
    }

    /// A local variable shadows the declaration of the same name outside of
    /// the function body it is declared in: the parameter of a function of
    /// a header and the local of another function are not the same
    /// variable.
    #[test]
    fn a_local_shadows_the_declaration_of_the_same_name() {
        let source = "vec3 BlendHue(vec3 base, vec3 blend)\n\
             {\n\
             \treturn vec3(blend);\n\
             }\n\
             void main()\n\
             {\n\
             \tfloat blend = 0.5;\n\
             \tfloat cloud0 = 1.0;\n\
             \tfloat cloud1 = 2.0;\n\
             \tfloat noise = mix(cloud0, cloud1, blend);\n\
             }\n";
        let normalized = normalize(source);
        assert!(normalized.contains("float noise = mix(cloud0, cloud1, blend);"));
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

    /// The cast of a macro body is rewritten as well: the macro would
    /// expand to a call naga does not know.
    #[test]
    fn the_cast_of_a_macro_body_is_rewritten() {
        let source = "#define BlendAdd(base, blend) min(base + blend, CAST3(1.0))\n";
        let normalized = normalize(source);
        assert!(!normalized.contains("CAST3"));
        assert!(normalized.contains("#define BlendAdd(base, blend) min(base + blend, vec3(1.0))"));
    }

    /// A cast around an expression keeps the expression and its
    /// parentheses: the cast is a constructor.
    #[test]
    fn the_cast_of_a_macro_body_keeps_its_argument() {
        let source =
            "#define BlendTint(base, blend) (CAST3(max(base.x, max(base.y, base.z))) * blend)\n";
        let normalized = normalize(source);
        assert!(!normalized.contains("CAST3"));
        assert!(normalized.contains(
            "#define BlendTint(base, blend) (vec3(max(base.x, max(base.y, base.z))) * blend)"
        ));
    }

    /// The name of a macro is not a cast: a macro may be called anything.
    #[test]
    fn the_name_of_a_macro_is_not_a_cast() {
        let source = "#define CAST3 something\n";
        assert_eq!(normalize(source), source);
    }

    /// The parameters of a macro have no precedence: the operands of a
    /// `mul` in a macro body are bracketed.
    #[test]
    fn the_parameters_of_a_macro_keep_working() {
        let source = "#define DOUBLE(x) mul(x, 2.0)\nfloat y = DOUBLE(a + b);\n";
        let normalized = normalize(source);
        assert!(normalized.contains("#define DOUBLE(x) (x) * (2.0)"));
        assert!(normalized.contains("float y = DOUBLE(a + b);"));
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

    /// The index of the instance of the engine is the one of GLSL.
    #[test]
    fn the_instance_index_of_the_engine_is_the_one_of_glsl() {
        let source = "void main() { gl_Position = vec4(float(gl_InstanceID)); }\n";
        let normalized = normalize(source);
        assert!(normalized.contains("gl_InstanceIndex"));
        assert!(!normalized.contains("gl_InstanceID"));
    }

    /// A shader that already uses the index of the instance of GLSL is not
    /// changed.
    #[test]
    fn the_instance_index_of_glsl_is_not_changed() {
        let source = "void main() { gl_Position = vec4(float(gl_InstanceIndex)); }\n";
        assert_eq!(normalize(source), source);
    }

    /// The declaration of the index of the instance of the engine is
    /// dropped: the index is a built-in of GLSL and not an attribute of the
    /// shader, and an attribute without a location collides with the first
    /// attribute of the shader.
    #[test]
    fn the_declaration_of_the_instance_index_is_dropped() {
        let source = "in uint gl_InstanceID;\nlayout(location=0) in vec3 a_Position;\n";
        let normalized = normalize(source);
        assert!(!normalized.contains("gl_InstanceID"));
        assert!(normalized.contains("layout(location=0) in vec3 a_Position;"));
    }

    /// The declaration of the index of the instance is dropped with its
    /// qualifiers: the layout of the attribute goes with it.
    #[test]
    fn the_qualifiers_of_the_declaration_of_the_instance_index_are_dropped() {
        let source = "layout(location=3) in uint gl_InstanceID;\nvoid main() {}\n";
        let normalized = normalize(source);
        assert!(!normalized.contains("gl_InstanceID"));
        assert!(!normalized.contains("layout(location=3)"));
        assert!(normalized.contains("void main() {}"));
    }

    /// A vector that is initialised with a scalar is given one.
    #[test]
    fn a_vector_that_is_initialised_with_a_scalar_is_promoted() {
        let source = "void main() { vec2 texelSize = 1; }\n";
        let normalized = normalize(source);
        assert!(normalized.contains("vec2 texelSize = vec2(1);"));
    }

    /// A vector that is initialised with a vector is not changed.
    #[test]
    fn a_vector_that_is_initialised_with_a_vector_is_not_changed() {
        let source = "void main() { vec2 v = vec2(1.0); }\n";
        assert_eq!(normalize(source), source);
    }

    /// A scalar that is initialised with a scalar is not changed: GLSL
    /// converts it itself.
    #[test]
    fn a_scalar_that_is_initialised_with_a_scalar_is_not_changed() {
        let source = "void main() { float f = 1.0; }\n";
        assert_eq!(normalize(source), source);
    }

    /// The casts of the engine are rewritten as well: the engine defines
    /// them as macros of its own compiler.
    #[test]
    fn the_casts_of_the_engine_are_rewritten() {
        let source = "void main() { float f = CASTF(1); uint u = CASTU(2); int i = CASTI(3); vec4 v = CAST4U(4); }\n";
        let normalized = normalize(source);
        assert!(normalized.contains("float f = float(1);"));
        assert!(normalized.contains("uint u = uint(2);"));
        assert!(normalized.contains("int i = int(3);"));
        assert!(normalized.contains("vec4 v = uvec4(4);"));
    }

    /// The built-in function of the engine is provided by the pipeline: it
    /// is in none of the headers of the assets.
    #[test]
    fn the_built_in_function_of_the_engine_is_provided() {
        let source = "void main() { vec3 l = PerformLighting_V1(v_Pos, albedo, n, v, spec, f0, 0.7, 0.0); }\n";
        let normalized = normalize(source);
        assert!(normalized.contains("vec3 PerformLighting_V1("));
    }

    /// A shader that has its own definition of the built-in function of the
    /// engine keeps it.
    #[test]
    fn a_shader_that_defines_the_built_in_function_keeps_it() {
        let source = "vec3 PerformLighting_V1(vec3 a, vec3 b) { return a; }\n\
             void main() { vec3 l = PerformLighting_V1(v_Pos, albedo); }\n";
        let normalized = normalize(source);
        assert_eq!(normalized.matches("PerformLighting_V1").count(), 2);
    }

    /// The built-in function of the engine is added after the version of
    /// the shader: the version has to be the first thing in the file.
    #[test]
    fn the_built_in_function_is_added_after_the_version_of_the_shader() {
        let source = "#version 450\n\
             void main() { vec3 l = PerformLighting_V1(v_Pos, albedo, n, v, spec, f0, 0.7, 0.0); }\n";
        let normalized = normalize(source);
        assert!(normalized.starts_with("#version 450\n"));
        assert!(normalized.contains("\nvec3 PerformLighting_V1("));
    }

    /// The format of a texture is the format of the default texture of the
    /// engine: the pipeline does not know the format of the material.
    #[test]
    fn the_format_of_a_texture_is_defined() {
        let source = "void main() { int f = TEX8FORMAT; }\n";
        let normalized = normalize(source);
        assert!(normalized.contains("#define TEX8FORMAT 0"));
    }

    /// The `lerp` of the engine is the `mix` of GLSL.
    #[test]
    fn the_lerp_of_the_engine_is_a_mix() {
        let source = "vec3 albedo = v_Color;\nfloat a = lerp(albedo, albedo, 0.5);\n";
        let normalized = normalize(source);
        assert!(normalized.contains("mix(albedo, albedo, vec3(0.5))"));
        assert!(!normalized.contains("lerp"));
    }

    /// The `mix` of GLSL is not touched.
    #[test]
    fn the_mix_of_glsl_is_not_changed() {
        let source = "vec3 albedo = v_Color;\nfloat a = mix(albedo, albedo, 0.5);\n";
        let normalized = normalize(source);
        assert!(normalized.contains("mix(albedo, albedo, vec3(0.5))"));
    }

    /// A shader that defines a function of that name keeps it: the shader
    /// knows what its own function does.
    #[test]
    fn a_function_of_the_shader_keeps_its_name() {
        let source = "float lerp(float a, float b, float t) { return a; }\n\
             void main() { float x = lerp(1.0, 2.0, 0.5); }\n";
        let normalized = normalize(source);
        assert!(normalized.contains("float lerp(float a, float b, float t)"));
        assert!(!normalized.contains("mix("));
    }

    /// The call of a texture that is built with a sampler uses the texture
    /// and the sampler of the pipeline.
    #[test]
    fn the_combined_sampler_uses_the_texture_and_the_sampler_of_the_pipeline() {
        let source = "layout(binding=0) uniform texture2D g_Texture0;\n\
             layout(binding=1) uniform sampler _wm_sampler;\n\
             void main() { vec4 c = texture(sampler2D(g_Texture0, _wm_sampler), uv); }\n";
        let normalized = normalize(source);
        assert!(normalized.contains("texture(sampler2D(g_Texture0, _wm_sampler), uv)"));
    }

    /// A plain `texture(s, uv)` is not touched: the rule only fires on the
    /// combined form.
    #[test]
    fn a_plain_texture_call_is_not_changed() {
        let source = "layout(binding=0) uniform texture2D g_Texture0;\n\
             void main() { vec4 c = texture(g_Texture0, uv); }\n";
        let normalized = normalize(source);
        assert!(normalized.contains("texture(g_Texture0, uv)"));
    }

    /// A legal GLSL sampler is not changed either.
    #[test]
    fn a_legal_glsl_sampler_is_not_changed() {
        let source = "uniform sampler2D g_Texture0;\n\
             void main() { vec4 c = texture(g_Texture0, uv); }\n";
        let normalized = normalize(source);
        assert!(normalized.contains("texture(g_Texture0, uv)"));
    }

    /// The coordinate of a texture is truncated to the number of the
    /// components the texture takes: the engine truncates it, naga does not.
    #[test]
    fn the_coordinate_of_a_texture_is_truncated() {
        let source = "layout(binding=0) uniform texture2D g_Texture0;\n\
             layout(binding=1) uniform sampler _wm_sampler;\n\
             in vec4 v_TexCoord;\n\
             void main() { vec4 c = texture(sampler2D(g_Texture0, _wm_sampler), v_TexCoord); }\n";
        let normalized = normalize(source);
        assert!(normalized.contains("texture(sampler2D(g_Texture0, _wm_sampler), v_TexCoord.xy)"));
    }

    /// The longer operand of a binary operation is truncated to the
    /// number of the components of the shorter one.
    ///
    /// The type aware stage has that rule already, but it cannot see the
    /// type of a macro: the macros are expanded by the shader compiler,
    /// after the type aware stage has run.
    #[test]
    fn the_longer_operand_of_a_binary_operation_is_truncated() {
        let source = "#define scaleCenter (u_Center * 2.0 - 1.0)\n\
             uniform vec2 u_Center;\n\
             in vec4 v_TexCoord;\n\
             void main() { vec2 v = v_TexCoord * 2.0 - 1.0 - scaleCenter; }\n";
        let normalized = normalize(source);
        assert!(
            normalized.contains("(v_TexCoord * 2.0 - 1.0).xy"),
            "the operand is not truncated:\n{normalized}"
        );
    }

    /// A binary operation whose operands have the same number of
    /// components is not touched.
    #[test]
    fn a_binary_operation_of_two_vectors_is_not_changed() {
        let source = "#define scaleCenter (u_Center * 2.0 - 1.0)\n\
             uniform vec2 u_Center;\n\
             in vec2 v_TexCoord;\n\
             void main() { vec2 v = v_TexCoord * 2.0 - 1.0 - scaleCenter; }\n";
        let normalized = normalize(source);
        assert!(!normalized.contains(".xy)"));
    }

    /// The built-in function of the engine that reads the audio spectrum is
    /// provided by the pipeline: it is in none of the headers of the assets.
    #[test]
    fn the_built_in_function_of_the_audio_is_provided() {
        let source = "void main() { float a = CreateAudioResponse(left, right); }\n";
        let normalized = normalize(source);
        assert!(normalized.contains("float CreateAudioResponse("));
    }

    /// A function whose parameter is an array cannot be compiled by naga:
    /// the parameter becomes a scalar and the call passes the first element
    /// of the array.
    #[test]
    fn a_function_with_an_array_parameter_is_rewritten() {
        let source = "uniform float g_Spectrum[16];\n\
             float Response(float bufferLeft[16], float bufferRight[16])\n\
             {\n\
             \treturn bufferLeft[0] + bufferRight[0];\n\
             }\n\
             void main() { float a = Response(g_Spectrum, g_Spectrum); }\n";
        let normalized = normalize(source);
        assert!(normalized.contains("float Response(float bufferLeft, float bufferRight)"));
        assert!(normalized.contains("return bufferLeft + bufferRight;"));
        assert!(normalized.contains("Response(g_Spectrum[0], g_Spectrum[0])"));
    }

    /// The uniform of the engine that a shader uses is declared: naga
    /// reports an unknown variable for a name that is used and not
    /// declared.
    #[test]
    fn the_uniform_of_the_engine_is_declared() {
        let source = "void main() { vec2 v = res * 0.5; }\n";
        assert_eq!(engine_uniforms(source), vec![("res", "vec2")]);
    }

    /// A shader that does not use the uniform does not get it: the layout
    /// would carry a uniform nobody needs.
    #[test]
    fn a_uniform_that_is_not_used_is_not_declared() {
        let source = "void main() { vec2 v = g_TexCoord * 0.5; }\n";
        assert!(engine_uniforms(source).is_empty());
    }

    /// A shader that declares the uniform itself keeps its declaration.
    #[test]
    fn a_uniform_that_is_declared_is_not_declared_again() {
        let source = "uniform vec2 res;\nvoid main() { vec2 v = res * 0.5; }\n";
        assert!(engine_uniforms(source).is_empty());
    }

    /// The shaders of the engine compile once they are normalised.
    #[test]
    fn the_real_shaders_compile() {
        for (name, source, stage, defines) in FIXTURES.iter().chain(COMPILES_WITHOUT_THE_DIALECT) {
            let normalized = normalize(source);
            if let Some(error) = compile_error(&normalized, *stage, defines) {
                panic!("the shader '{name}' does not compile:\n{error}\n---\n{normalized}");
            }
        }
    }

    /// The shaders of the engine do not compile before they are normalised:
    /// the dialect is what makes them work.
    #[test]
    fn the_real_shaders_do_not_compile_before_the_normalisation() {
        for (name, source, stage, defines) in FIXTURES {
            if *name == "shadow_sampler.frag" {
                // The pipeline binds the shadow map as an ordinary texture,
                // so this one compiles as it is.
                continue;
            }
            assert!(
                compile_error(source, *stage, defines).is_some(),
                "the shader '{name}' compiles without the dialect"
            );
        }
    }

    /// The dialect stage does not break a shader that compiles without it.
    ///
    /// The promotion of a scalar to a vector is only allowed when it does
    /// not change the type of the expression: the fire effect of the engine
    /// mixes two scalars with a scalar.
    #[test]
    fn the_dialect_stage_does_not_break_a_shader_that_compiles_without_it() {
        for (name, source, stage, defines) in COMPILES_WITHOUT_THE_DIALECT {
            assert!(
                compile_error(source, *stage, defines).is_none(),
                "the shader '{name}' does not compile without the dialect"
            );
            let normalized = normalize(source);
            assert!(
                compile_error(&normalized, *stage, defines).is_none(),
                "the shader '{name}' does not compile with the dialect"
            );
        }
    }
}
