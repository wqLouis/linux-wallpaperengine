// A real Wallpaper Engine shader: the vertex stage of the engine's built-in
// `shadowcaster` effect (src/scene/renderer/post_processor/shader_headers/
// shadowcaster.vert at 5844d5e^).  The engine tells the shader how many
// bones it has to work with; the macro is the size of an array, so the
// shader does not compile when the pipeline does not provide it.
#version 450

layout(location=0) in vec3 a_Position;

layout(binding=0, std140) uniform EffectParams {
	mat4x3 [BONECOUNT] g_Bones;
};

void main() {
	gl_Position = vec4(a_Position, 1.0);
}
