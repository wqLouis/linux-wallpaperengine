// Wallpaper Engine vertex shader utilities.

// Build tangent space matrix from normal and tangent (with sign in w).
mat3 BuildTangentSpace(vec3 normal, vec4 signedTangent) {
    vec3 tangent = signedTangent.xyz;
    vec3 bitangent = cross(normal, tangent) * signedTangent.w;
    return mat3(tangent, bitangent, normal);
}

// Build world-space tangent space matrix.
mat3 BuildTangentSpace(mat3 modelTransform, vec3 normal, vec4 signedTangent) {
    vec3 worldNormal = normalize(modelTransform * normal);
    vec3 worldTangent = normalize(modelTransform * signedTangent.xyz);
    vec3 worldBitangent = cross(worldNormal, worldTangent) * signedTangent.w;
    return mat3(worldTangent, worldBitangent, worldNormal);
}
