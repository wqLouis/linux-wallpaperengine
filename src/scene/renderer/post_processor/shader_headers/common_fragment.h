// Wallpaper Engine fragment shader utilities.

// Decompress a DXT5n normal map.
// DXT5n stores X in alpha channel, Y in green channel.
vec3 DecompressNormal(vec4 compressed) {
    vec3 normal;
    normal.xy = vec2(compressed.a, compressed.g) * 2.0 - 1.0;
    normal.z = sqrt(max(1.0 - dot(normal.xy, normal.xy), 0.0));
    return normalize(normal);
}

// Decompress a DXT5n normal map with a low-precision alpha mask.
// Returns vec4(rgb=normal, a=mask).
vec4 DecompressNormalWithMask(vec4 compressed) {
    vec4 result;
    result.xyz = DecompressNormal(compressed);
    result.w = compressed.r; // Alpha mask stored in red channel
    return result;
}

// Sample a single-channel 8-bit texture.
// D3D9 stores data in alpha, D3D10+/OpenGL store in red.
// Returns the maximum of both channels for compatibility.
float ConvertSampleR8(vec4 sample) {
    return max(sample.r, sample.a);
}

// Convert texture sample based on format bound to sampler 0.
// RG88: greyscale with alpha. R8: white with alpha. RGBA8888: pass-through.
vec4 ConvertTexture0Format(vec4 sample) {
    // Default: assume RGBA8888 (pass-through)
    // Shaders using this should define the format via a combo or uniform.
#if defined(TEXTURE0_FORMAT_RG88)
    return vec4(vec3(greyscale(sample.rg)), sample.a);
#elif defined(TEXTURE0_FORMAT_R8)
    return vec4(1.0, 1.0, 1.0, ConvertSampleR8(sample));
#else
    return sample;
#endif
}
