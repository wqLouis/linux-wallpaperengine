// Fullscreen-triangle mip-down shader.
//
// Renders a fullscreen triangle and samples the source mip level with
// linear filtering. Each output pixel naturally averages a 2x2 (or
// similar) block of source texels via hardware filtering, which is
// what we want for a mipmap downsample.
//
// Used by `MipChainGenerator` in `mip_loader.rs` to fill in the
// missing mip levels of a `wgpu::Texture` whose payload was uploaded
// by the parser.

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    // Single fullscreen triangle covering NDC [-1, 3] x [-1, 3] clipped
    // to the render target. This trick avoids a vertex buffer entirely.
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    let p = positions[vi];

    var out: VsOut;
    out.position = vec4<f32>(p, 0.0, 1.0);
    // NDC -> UV. Y is flipped so the destination mip is not upside down
    // relative to how the source was uploaded.
    out.tex_coords = vec2<f32>(p.x * 0.5 + 0.5, 1.0 - (p.y * 0.5 + 0.5));
    return out;
}

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;

@fragment
fn fs_main(vs: VsOut) -> @location(0) vec4<f32> {
    return textureSample(src_tex, src_sampler, vs.tex_coords);
}
