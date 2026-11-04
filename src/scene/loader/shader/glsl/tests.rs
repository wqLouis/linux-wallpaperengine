//! Tests for the type aware normalisation stage.
//!
//! The most important test is [`the_preprocessed_shader_compiles`]: it runs
//! the whole preprocessor over a shader that reproduces the crash the
//! normalisation was written for and compiles the result with naga, the
//! shader compiler wgpu uses.  Without the normalisation naga rejects the
//! shader with "Operation Multiply can't work with Vector{Quad} and
//! Vector{Bi}".

use super::*;
use crate::collections::new_map;
use crate::scene::loader::shader::{collect_layout, preprocess_pair_with_layout};

/// Build a shader from its declarations and the body of `main`.
fn shader(decls: &str, body: &str) -> String {
    let mut source = String::new();
    if !decls.is_empty() {
        source.push_str(decls);
        source.push('\n');
    }
    source.push_str("void main() {\n");
    source.push_str(body);
    source.push_str("\n}\n");
    source
}

/// Normalise a shader with a layout collected from the shader itself.
fn normalise_source(source: &str) -> String {
    let layout = collect_layout(source, source, &new_map());
    normalise(source, &layout)
}

/// A normalisation case.
struct RuleCase {
    /// What the case is about.
    name: &'static str,
    /// The declarations of the shader.
    decls: &'static str,
    /// The body of `main`.
    body: &'static str,
    /// The body of `main` after the normalisation.
    expected: &'static str,
}

/// The cases of the normalisation rules.
const CASES: &[RuleCase] = &[
    RuleCase {
        name: "a vec4 multiplied by a vec2 truncates the vec4",
        decls: "uniform vec2 g_Scale;",
        body: "    vec4 a = vec4(1.0);\n    vec2 b = a * g_Scale;",
        expected: "    vec4 a = vec4(1.0);\n    vec2 b = a.xy * g_Scale;",
    },
    RuleCase {
        name: "a vec4 multiplied by a vec2 truncates the vec2 side as well",
        decls: "uniform vec4 g_Scale;",
        body: "    vec2 a = vec2(1.0);\n    vec2 b = a * g_Scale;",
        expected: "    vec2 a = vec2(1.0);\n    vec2 b = a * g_Scale.xy;",
    },
    RuleCase {
        name: "a vec4 assigned to a vec2 is truncated",
        decls: "",
        body: "    vec4 a = vec4(1.0);\n    vec2 b = a;",
        expected: "    vec4 a = vec4(1.0);\n    vec2 b = a.xy;",
    },
    RuleCase {
        name: "a vec3 multiplied by a vec2 truncates the vec3",
        decls: "uniform vec2 g_Scale;",
        body: "    vec3 a = vec3(1.0);\n    vec2 b = a * g_Scale;",
        expected: "    vec3 a = vec3(1.0);\n    vec2 b = a.xy * g_Scale;",
    },
    RuleCase {
        name: "a vec2 assigned to a vec4 is left alone",
        decls: "",
        body: "    vec2 a = vec2(1.0);\n    vec4 b = a;",
        expected: "    vec2 a = vec2(1.0);\n    vec4 b = a;",
    },
    RuleCase {
        name: "a vec4 multiplied by a float is left alone",
        decls: "",
        body: "    vec4 a = vec4(1.0);\n    vec4 b = a * 0.5;",
        expected: "    vec4 a = vec4(1.0);\n    vec4 b = a * 0.5;",
    },
    RuleCase {
        name: "a mat4 multiplied by a vec4 is left alone",
        decls: "uniform mat4 g_Matrix;",
        body: "    vec4 b = g_Matrix * vec4(1.0);",
        expected: "    vec4 b = g_Matrix * vec4(1.0);",
    },
    RuleCase {
        name: "an unknown symbol is left alone",
        decls: "",
        body: "    vec2 b = g_Unknown * vec2(1.0);",
        expected: "    vec2 b = g_Unknown * vec2(1.0);",
    },
    RuleCase {
        name: "an unknown function is left alone",
        decls: "",
        body: "    vec2 b = unknownFunction() * vec2(1.0);",
        expected: "    vec2 b = unknownFunction() * vec2(1.0);",
    },
    RuleCase {
        name: "the swizzle of a swizzle is left alone",
        decls: "",
        body: "    vec4 a = vec4(1.0);\n    vec2 b = a.xy.xy;",
        expected: "    vec4 a = vec4(1.0);\n    vec2 b = a.xy.xy;",
    },
    RuleCase {
        name: "a swizzle operand is truncated by shortening its components",
        decls: "uniform vec2 g_Scale;",
        body: "    vec4 a = vec4(1.0);\n    vec2 b = a.xyz * g_Scale;",
        expected: "    vec4 a = vec4(1.0);\n    vec2 b = a.xy * g_Scale;",
    },
    RuleCase {
        name: "a vec4 assigned to a vec3 swizzle is truncated",
        decls: "uniform vec4 g_Color;",
        body: "    vec4 albedo = vec4(0.0);\n    albedo.rgb = g_Color;",
        expected: "    vec4 albedo = vec4(0.0);\n    albedo.rgb = g_Color.xyz;",
    },
    RuleCase {
        name: "the value of a compound assignment is truncated",
        decls: "uniform vec4 g_Color;",
        body: "    vec4 albedo = vec4(0.0);\n    albedo.rgb *= g_Color * 2.0;",
        expected: "    vec4 albedo = vec4(0.0);\n    albedo.rgb *= (g_Color * 2.0).xyz;",
    },
    RuleCase {
        name: "the element of an array is not a vector component",
        decls: "varying vec2 v_TexCoord[4];",
        body: "    v_TexCoord[0] = vec2(1.0);",
        expected: "    v_TexCoord[0] = vec2(1.0);",
    },
    RuleCase {
        name: "an expression inside a call argument is truncated",
        decls: "uniform vec2 g_Scale;",
        body: "    vec4 a = vec4(1.0);\n    vec2 b = clamp(a * g_Scale, vec2(0.0), vec2(1.0));",
        expected: "    vec4 a = vec4(1.0);\n    vec2 b = clamp(a.xy * g_Scale, vec2(0.0), vec2(1.0));",
    },
    RuleCase {
        name: "a chained multiplication is truncated",
        decls: "uniform vec2 g_Scale;",
        body: "    vec4 a = vec4(1.0);\n    vec2 b = a * g_Scale * vec2(2.0);",
        expected: "    vec4 a = vec4(1.0);\n    vec2 b = a.xy * g_Scale * vec2(2.0);",
    },
    RuleCase {
        name: "the sum of two vectors of a different width is truncated",
        decls: "uniform vec3 g_Offset;",
        body: "    vec2 a = vec2(1.0);\n    vec2 b = a + g_Offset;",
        expected: "    vec2 a = vec2(1.0);\n    vec2 b = a + g_Offset.xy;",
    },
    RuleCase {
        name: "a legal shader is not touched at all",
        decls: "uniform vec2 g_Scale;\nuniform mat4 g_Matrix;",
        body: "    vec4 a = g_Matrix * vec4(1.0);\n    vec4 b = a * g_Scale.x;\n    vec2 c = b.xy;",
        expected: "    vec4 a = g_Matrix * vec4(1.0);\n    vec4 b = a * g_Scale.x;\n    vec2 c = b.xy;",
    },
];

#[test]
fn the_rules_normalise_the_shaders_they_are_meant_to() {
    for case in CASES {
        let source = shader(case.decls, case.body);
        let expected = shader(case.decls, case.expected);
        let normalised = normalise_source(&source);
        assert_eq!(normalised, expected, "case: {}", case.name);
    }
}

#[test]
fn a_legal_shader_stays_byte_identical() {
    let source = shader(
        "uniform vec2 g_Scale;\nuniform mat4 g_Matrix;\nuniform float g_Time;",
        "    vec4 position = g_Matrix * vec4(1.0, 2.0, 3.0, 1.0);\n    \
         vec2 scaled = position.xy * g_Scale;\n    \
         float phase = g_Time * 0.5;\n    \
         vec3 shaded = normalize(vec3(scaled, phase));\n    \
         gl_Position = position;",
    );
    let normalised = normalise_source(&source);
    assert_eq!(normalised, source);
}

#[test]
fn comments_and_spacing_are_preserved() {
    let source = shader(
        "uniform vec2 g_Scale;",
        "    vec4 a = vec4(1.0); // a comment\n    vec2 b = a * g_Scale;",
    );
    let expected = shader(
        "uniform vec2 g_Scale;",
        "    vec4 a = vec4(1.0); // a comment\n    vec2 b = a.xy * g_Scale;",
    );
    assert_eq!(normalise_source(&source), expected);
}

/// The vertex shader of the workshop wallpaper that crashed.
///
/// It multiplies a `vec4` with a `vec2`, which Wallpaper Engine accepts and
/// naga does not.
const CRASH_VERTEX: &str = "\
uniform mat4 g_EffectTextureProjectionMatrixInverse;
uniform vec2 g_CursorScale;
uniform float g_CursorScaleMultiplier;

varying vec2 v_TexCoord;

void main() {
    vec2 cursorPositionAdjusted = vec2(0.5, 0.5);
    vec4 transformedCursorPosition = mul(vec4(cursorPositionAdjusted, 0.0, 1.0), g_EffectTextureProjectionMatrixInverse);
    vec2 da = transformedCursorPosition * g_CursorScale * g_CursorScaleMultiplier * 0.001;
    v_TexCoord = da;
    gl_Position = vec4(da, 0.0, 1.0);
}
";

/// The fragment shader that goes with [`CRASH_VERTEX`].
const CRASH_FRAGMENT: &str = "\
uniform sampler2D g_Texture;

varying vec2 v_TexCoord;

void main() {
    gl_FragColor = texture(g_Texture, v_TexCoord);
}
";

#[test]
fn the_preprocessed_shader_compiles() {
    let headers = new_map();
    let layout = collect_layout(CRASH_VERTEX, CRASH_FRAGMENT, &headers);
    let (vertex, fragment, _) = preprocess_pair_with_layout(
        CRASH_VERTEX,
        CRASH_FRAGMENT,
        &headers,
        &new_map(),
        &layout,
        false,
        0,
    );

    // The multiplication of the vec4 with the vec2 has to be truncated.
    assert!(
        vertex.contains("transformedCursorPosition.xy * g_CursorScale"),
        "the vec4 was not truncated:\n{}",
        vertex
    );

    compile(&vertex, naga::ShaderStage::Vertex);
    compile(&fragment, naga::ShaderStage::Fragment);
}

/// Compile a GLSL source with naga and validate the result.
fn compile(source: &str, stage: naga::ShaderStage) {
    let mut frontend = naga::front::glsl::Frontend::default();
    let options = naga::front::glsl::Options {
        stage,
        defines: Default::default(),
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
