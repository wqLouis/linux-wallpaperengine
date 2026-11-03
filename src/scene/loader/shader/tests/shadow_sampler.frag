// A real Wallpaper Engine shader: the fragment stage of the engine's built-in
// `chroma4` effect (src/scene/renderer/post_processor/shader_headers/
// chroma4.frag at 5844d5e^).  The engine declares its shadow map with a
// comparison sampler; the pipeline has no comparison sampler, so the
// shadow map is bound as an ordinary two dimensional texture - this is what
// the source looks like after the layout collector has seen the rewritten
// sampler type.
#version 450

layout(binding=0) uniform texture2D g_Texture6;
layout(binding=1) uniform sampler _wm_sampler;

layout(location=0) in vec2 v_TexCoord;

out vec4 _fragColor;

void main() {
	_fragColor = texture(sampler2D(g_Texture6, _wm_sampler), v_TexCoord);
}
