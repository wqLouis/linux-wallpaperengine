use wgpu::naga::ShaderStage;

pub use super::shader_header::WM_SAMPLER_BINDING;
pub use super::shader_layout::EffectLayout;
pub use super::shader_transform::preprocess_with_layout;

use super::shader_layout::collect_layout;

pub fn preprocess_pair(vert: &str, frag: &str) -> (String, String, EffectLayout) {
    let layout = collect_layout(vert, frag);
    let vert_out = preprocess_with_layout(vert, ShaderStage::Vertex, &layout);
    let frag_out = preprocess_with_layout(frag, ShaderStage::Fragment, &layout);
    (vert_out, frag_out, layout)
}

pub fn preprocess(source: &str, stage: ShaderStage) -> String {
    let layout = collect_layout(source, "");
    preprocess_with_layout(source, stage, &layout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_declarations() {
        let vert = r#"
uniform mat4 g_ModelViewProjectionMatrix;
uniform float g_Time;
uniform sampler2D g_Texture0;
uniform sampler2D g_Texture1;
"#;
        let frag = r#"
uniform sampler2D g_Texture0;
uniform sampler2D g_Texture2;
uniform float g_Speed;
"#;
        let layout = collect_layout(vert, frag);
        assert_eq!(layout.sampler_names.len(), 3);
        assert!(layout.sampler_names.contains(&"g_Texture0".to_string()));
        assert!(layout.sampler_names.contains(&"g_Texture1".to_string()));
        assert!(layout.sampler_names.contains(&"g_Texture2".to_string()));
        assert_eq!(layout.uniform_decls.len(), 3);
    }

    #[test]
    fn test_preprocess_waterwaves() {
        let vert = include_str!("../../../../test/yurucamp/shaders/effects/waterwaves.vert");
        let frag = include_str!("../../../../test/yurucamp/shaders/effects/waterwaves.frag");
        let (vert_out, frag_out, layout) = preprocess_pair(vert, frag);

        assert_eq!(layout.sampler_names.len(), 3);
        assert!(layout.sampler_names.contains(&"g_Texture0".to_string()));
        assert!(layout.sampler_names.contains(&"g_Texture1".to_string()));
        assert!(layout.sampler_names.contains(&"g_Texture2".to_string()));

        assert!(vert_out.contains("layout(location=0) in vec3 a_Position;"));
        assert!(vert_out.contains("layout(location=1) in vec2 a_TexCoord;"));
        assert!(vert_out.contains("layout(location=2) out vec4 v_TexCoord;"));
        assert!(vert_out.contains("layout(binding=0) uniform texture2D g_Texture0;"));
        assert!(vert_out.contains("layout(binding=1) uniform sampler _wm_sampler;"));
        assert!(vert_out.contains("g_ModelViewProjectionMatrix * vec4(a_Position, 1.0)"));
        assert!(vert_out.contains("uniform EffectParams"));

        assert!(frag_out.contains("layout(location=2) in vec4 v_TexCoord;"));
        assert!(frag_out.contains("layout(binding=0) uniform texture2D g_Texture0;"));
        assert!(frag_out.contains("layout(binding=2) uniform texture2D g_Texture1;"));
        assert!(frag_out.contains("layout(binding=4) uniform texture2D g_Texture2;"));
        assert!(frag_out.contains("sampler2D(g_Texture0, _wm_sampler)"));
        assert!(frag_out.contains("rotateVec2"));
    }

    #[test]
    fn test_preprocess_cloudmotion() {
        let vert = include_str!("../../../../test/yurucamp/shaders/effects/cloudmotion.vert");
        let frag = include_str!("../../../../test/yurucamp/shaders/effects/cloudmotion.frag");
        let (vert_out, _frag_out, layout) = preprocess_pair(vert, frag);

        assert!(layout.varying_types.contains_key("v_NoiseCoord"));
        assert_eq!(layout.varying_types.get("v_NoiseCoord").unwrap(), "vec2");
        assert_eq!(layout.varying_types.get("v_TexCoord").unwrap(), "vec4");

        assert!(
            vert_out.contains("v_NoiseCoord = v_TexCoord.xy;"),
            "Expected truncation fix, got:\n{}",
            vert_out
        );
    }

    #[test]
    fn test_preprocess_pair_basic() {
        let vert = r#"
uniform mat4 g_ModelViewProjectionMatrix;
uniform float g_Time;
uniform sampler2D g_Texture0;
attribute vec3 a_Position;
attribute vec2 a_TexCoord;
varying vec4 v_TexCoord;

void main() {
    gl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix);
    v_TexCoord = a_TexCoord.xyxy;
}
"#;
        let frag = r#"
#include "common.h"
uniform sampler2D g_Texture0;
uniform float g_Time;
varying vec4 v_TexCoord;

void main() {
    gl_FragColor = texSample2D(g_Texture0, v_TexCoord.xy);
}
"#;
        let (vert_out, frag_out, layout) = preprocess_pair(vert, frag);

        assert!(vert_out.contains("layout(location=0) in vec3 a_Position;"));
        assert!(vert_out.contains("layout(location=0) out vec4 v_TexCoord;"));
        assert!(vert_out.contains("layout(binding=0) uniform texture2D g_Texture0;"));
        assert!(vert_out.contains("g_ModelViewProjectionMatrix * vec4(a_Position, 1.0)"));

        assert!(frag_out.contains("layout(location=0) in vec4 v_TexCoord;"));
        assert!(frag_out.contains("sampler2D(g_Texture0, _wm_sampler)"));

        assert_eq!(layout.sampler_names, vec!["g_Texture0"]);
        assert_eq!(layout.sampler_bindings, vec![0]);
    }
}
