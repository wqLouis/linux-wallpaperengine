struct VertexOutput {
    @builtin(position) clip_pos: vec4f,
    @location(0) uv: vec2f,
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var tex_sampler: sampler;
@group(0) @binding(2) var<uniform> projection_matrix: mat4x4f;

@vertex
fn vs_main(@location(0) pos: vec3f, @location(1) uv: vec2f) -> VertexOutput {
    var output: VertexOutput;
    output.clip_pos = projection_matrix * vec4f(pos, 1);
    output.uv = uv;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4f {
    return textureSample(tex, tex_sampler, input.uv);
}
