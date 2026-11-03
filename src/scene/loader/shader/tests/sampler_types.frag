// A real Wallpaper Engine shader: the fragment stage of the engine's built-in
// `chroma4` effect (src/scene/renderer/post_processor/shader_headers/
// chroma4.frag at 5844d5e^).  The engine declares its shadow map with a
// comparison sampler and its volume texture with a three dimensional
// sampler.  The pipeline binds every texture as a two dimensional texture
// and it has no comparison sampler, so both are approximated by a
// `sampler2D`.
#version 450

uniform sampler2DComparison g_Texture6;
uniform sampler3D g_Texture1;

out vec4 _fragColor;

void main() {
	_fragColor = vec4(0.0);
}
