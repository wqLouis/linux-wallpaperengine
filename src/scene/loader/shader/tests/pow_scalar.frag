// A real Wallpaper Engine shader: the fragment stage of the engine's built-in
// `downsample_quarter_linear` effect (src/scene/renderer/post_processor/
// shader_headers/downsample_quarter_linear.frag at 5844d5e^).  The engine
// writes `pow(vector, scalar)`, which is not a GLSL overload.
#version 450

layout(location=0) in vec3 v_Color;

layout(binding=0, std140) uniform EffectParams {
	float g_Exposure;
};

out vec4 _fragColor;

void main() {
	_fragColor = vec4(pow(v_Color, 1.0 / 2.2), 1.0);
}
