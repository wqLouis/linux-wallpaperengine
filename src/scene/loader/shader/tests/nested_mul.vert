// A real Wallpaper Engine shader: the vertex stage of the engine's built-in
// `volumetricsfront` effect (src/scene/renderer/post_processor/shader_headers/
// volumetricsfront.vert at 5844d5e^).  The declarations are the ones the
// preprocessing pipeline emits, the `mul` calls are the ones the engine
// writes.
#version 450

layout(location=0) in vec3 a_Position;

layout(location=0) out vec4 v_ScreenPos;

layout(binding=0, std140) uniform EffectParams {
	mat4 g_AltViewProjectionMatrix;
	mat4 g_ViewProjectionMatrix;
};

void main() {
	gl_Position = mul(mul(vec4(a_Position, 1.0), g_AltViewProjectionMatrix), g_ViewProjectionMatrix);
	v_ScreenPos = gl_Position;
}
