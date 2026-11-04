// A real Wallpaper Engine shader: the vertex stage of the engine's built-in
// `shadowcaster` effect (assets/shaders/shadowcaster.vert).  The engine
// writes the index of the instance as `gl_InstanceID` and declares it as
// an attribute, because it is a built-in of HLSL.  GLSL has
// `gl_InstanceIndex`, which is a built-in of the language and not an
// attribute of the shader: the declaration has to go, or it collides with
// the first attribute of the shader.
#version 450

in uint gl_InstanceID;

layout(location=0) in vec3 a_Position;

layout(location=0) out uint v_ViewportIndex;

void main() {
	gl_Position = vec4(a_Position, 1.0);
	v_ViewportIndex = gl_InstanceID;
}
