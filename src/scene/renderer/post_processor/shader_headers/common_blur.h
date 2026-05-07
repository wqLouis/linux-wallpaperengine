// Wallpaper Engine gaussian blur kernels.
// Uses g_Texture0 as the source texture (declared by shader preprocessor).
// uv.xy = base texture coordinate, uv.zw = offset step (vec2)

vec4 blur13a(vec2 uv, vec2 step) {
    vec4 result = vec4(0.0);
    float a = 0.0;
    vec4 s;

    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2(-6.0, -6.0), 0.0); result += s * 0.006299; a += s.a * 0.006299;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2(-5.0, -5.0), 0.0); result += s * 0.017298; a += s.a * 0.017298;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2(-4.0, -4.0), 0.0); result += s * 0.039533; a += s.a * 0.039533;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2(-3.0, -3.0), 0.0); result += s * 0.075189; a += s.a * 0.075189;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2(-2.0, -2.0), 0.0); result += s * 0.119007; a += s.a * 0.119007;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2(-1.0, -1.0), 0.0); result += s * 0.156756; a += s.a * 0.156756;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv, 0.0);                              result += s * 0.171834; a += s.a * 0.171834;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2( 1.0,  1.0), 0.0); result += s * 0.156756; a += s.a * 0.156756;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2( 2.0,  2.0), 0.0); result += s * 0.119007; a += s.a * 0.119007;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2( 3.0,  3.0), 0.0); result += s * 0.075189; a += s.a * 0.075189;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2( 4.0,  4.0), 0.0); result += s * 0.039533; a += s.a * 0.039533;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2( 5.0,  5.0), 0.0); result += s * 0.017298; a += s.a * 0.017298;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2( 6.0,  6.0), 0.0); result += s * 0.006299; a += s.a * 0.006299;

    return vec4(result.rgb / max(a, 0.001), a / 13.0);
}

vec4 blur7a(vec2 uv, vec2 step) {
    vec4 result = vec4(0.0);
    float a = 0.0;
    vec4 s;

    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2(-3.0, -3.0), 0.0); result += s * 0.071303; a += s.a * 0.071303;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2(-2.0, -2.0), 0.0); result += s * 0.131514; a += s.a * 0.131514;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2(-1.0, -1.0), 0.0); result += s * 0.189879; a += s.a * 0.189879;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv, 0.0);                              result += s * 0.214607; a += s.a * 0.214607;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2( 1.0,  1.0), 0.0); result += s * 0.189879; a += s.a * 0.189879;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2( 2.0,  2.0), 0.0); result += s * 0.131514; a += s.a * 0.131514;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2( 3.0,  3.0), 0.0); result += s * 0.071303; a += s.a * 0.071303;

    return vec4(result.rgb / max(a, 0.001), a / 7.0);
}

vec4 blur3a(vec2 uv, vec2 step) {
    vec4 result = vec4(0.0);
    float a = 0.0;
    vec4 s;

    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2(-1.0, -1.0), 0.0); result += s * 0.25; a += s.a * 0.25;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv, 0.0);                              result += s * 0.50; a += s.a * 0.50;
    s = textureLod(sampler2D(g_Texture0, _wm_sampler), uv + step * vec2( 1.0,  1.0), 0.0); result += s * 0.25; a += s.a * 0.25;

    return vec4(result.rgb / max(a, 0.001), a / 3.0);
}
