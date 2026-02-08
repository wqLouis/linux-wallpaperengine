struct VertexOutput {
    @builtin(position) clip_pos: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs_main(@location(0) pos: vec3f, @location(1) uv: vec2f) -> VertexOutput {
    var output: VertexOutput;
    output.clip_pos = vec4f(pos, 1);
    output.uv = uv;
    return output;
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var tex_sampler: sampler;

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4f {
    return vec4f(1, 1, 1, 1);
}
