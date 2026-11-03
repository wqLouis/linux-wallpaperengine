// A real Wallpaper Engine shader: the fragment stage of the engine's built-in
// `fade` effect (src/scene/renderer/post_processor/shader_headers/fade.frag
// at 5844d5e^).  The engine writes the precision qualifiers the GLSL
// compiler of naga does not accept in a uniform block.
#version 450

layout(location=0) in mediump vec2 v_TexCoord;

layout(binding=0, std140) uniform EffectParams {
	mediump float g_Alpha;
};

out vec4 _fragColor;

void main() {
	_fragColor = vec4(v_TexCoord, g_Alpha, 1.0);
}
