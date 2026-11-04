// A real Wallpaper Engine shader: the fragment stage of the engine's
// built-in `editorpaintbrush` effect (assets/shaders/editorpaintbrush.frag).
// The engine initialises a vector with a scalar, which GLSL does not
// allow: the initialiser has to be wrapped in a constructor of the type
// of the variable.
#version 450

layout(location=0) in vec2 v_TexCoord;

out vec4 _fragColor;

void main() {
	vec2 texelSize = 1;
	_fragColor = vec4(texelSize, 0.0, 1.0);
}
