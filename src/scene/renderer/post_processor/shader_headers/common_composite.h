// Wallpaper Engine composite blending.
// COMPOSITE combo values:
//   0 = Normal (replace), 1 = Blend (alpha composite),
//   2 = Under, 3 = Cutout

vec2 ApplyCompositeOffset(vec2 coord, vec2 resolution) {
    // OpenGL samples at pixel centers, no offset needed.
    // Direct3D requires a half-texel offset which is handled
    // by the HLSL_SM30 ifdef in the shader.
    return coord;
}

vec4 ApplyComposite(vec4 original, vec4 composite) {
#if COMPOSITE == 0
    // Normal: composite replaces original
    return composite;
#elif COMPOSITE == 1
    // Blend: alpha composite on top
    vec3 rgb = mix(original.rgb, composite.rgb, composite.a);
    float a = original.a + composite.a * (1.0 - original.a);
    return vec4(rgb, a);
#elif COMPOSITE == 2
    // Under: original on top of composite
    vec3 rgb = mix(composite.rgb, original.rgb, original.a);
    float a = original.a + composite.a * (1.0 - original.a);
    return vec4(rgb, a);
#elif COMPOSITE == 3
    // Cutout: composite alpha masks original
    float a = original.a * composite.a;
    return vec4(original.rgb * composite.a, a);
#else
    return composite;
#endif
}
