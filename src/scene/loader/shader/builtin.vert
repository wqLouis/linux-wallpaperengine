// The built-in vertex stage of an effect.
//
// An effect of Wallpaper Engine that has no vertex shader of its own draws
// a full screen quad: this is the vertex stage of the reference engine.
// It is written in the dialect of the engine, so the preprocessing of the
// pipeline handles it like any other vertex shader of a wallpaper.

attribute vec3 a_Position;
attribute vec2 a_TexCoord;

varying vec2 v_TexCoord;

void main() {
    gl_Position = vec4(a_Position, 1.0);
    v_TexCoord = a_TexCoord;
}
