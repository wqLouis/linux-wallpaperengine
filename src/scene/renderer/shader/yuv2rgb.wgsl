// The conversion of the YUV 4:2:0 planes of a video frame into RGBA.
//
// The three planes are sampled with the nearest neighbour (every 2x2 block
// of luma pixels shares one chroma sample) and combined with the matrix
// of the colour space of the frame.
//
// The result is the *gamma encoded* (display) RGB, exactly like the
// `swscale` conversion the parser used to do on the CPU.
//
// The texture the pass writes into is an sRGB one: the hardware encodes
// the value of the fragment again when it stores it.  The shader has to
// decode the gamma encoded value first, otherwise the wallpaper would
// receive the pixels twice decoded (and the picture would be much too
// bright).  The stored bytes are then the ones the `swscale` conversion
// produced, and the sampler of the wallpaper decodes them exactly like it
// decodes them today.

struct YuvParams {
    /// The scale of the luma (255/219 for the limited range).
    y_scale: f32,
    /// The offset of the luma (16 for the limited range, 0 for the full one).
    y_offset: f32,
    /// The offset of the U plane (always 128).
    u_offset: f32,
    /// The offset of the V plane (always 128).
    v_offset: f32,
    /// The coefficient of the V plane in the red channel.
    crv: f32,
    /// The coefficient of the U plane in the green channel.
    cgu: f32,
    /// The coefficient of the V plane in the green channel.
    cgv: f32,
    /// The coefficient of the U plane in the blue channel.
    cbu: f32,
};

@group(0) @binding(0) var y_plane: texture_2d<f32>;
@group(0) @binding(1) var u_plane: texture_2d<f32>;
@group(0) @binding(2) var v_plane: texture_2d<f32>;
@group(0) @binding(3) var<uniform> params: YuvParams;

struct VertexOutput {
    /// The position of the fragment in the target, in pixels.
    @builtin(position) position: vec4f,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    // A fullscreen triangle: the three vertices cover the whole target.
    var positions = array<vec2f, 3>(
        vec2f(-1.0, -1.0),
        vec2f(3.0, -1.0),
        vec2f(-1.0, 3.0),
    );

    var output: VertexOutput;
    output.position = vec4f(positions[index], 0.0, 1.0);
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4f {
    // The position of the fragment is the pixel of the frame it covers.
    let pixel = vec2i(input.position.xy);

    let luma = textureLoad(y_plane, pixel, 0).r * 255.0;

    // The chroma planes are half the width and half the height of the
    // frame: the sample of the block is the nearest one.
    let chroma = pixel / 2;
    let blue = textureLoad(u_plane, chroma, 0).r * 255.0;
    let red = textureLoad(v_plane, chroma, 0).r * 255.0;

    let scaled = params.y_scale * (luma - params.y_offset);
    let rgb = vec3f(
        scaled + params.crv * (red - params.v_offset),
        scaled + params.cgu * (blue - params.u_offset) + params.cgv * (red - params.v_offset),
        scaled + params.cbu * (blue - params.u_offset),
    );

    // The values are gamma encoded: they are decoded here, because the
    // sRGB target encodes them again when it stores them.
    let linear = srgb_to_linear(clamp(rgb, vec3f(0.0), vec3f(255.0)) / 255.0);

    return vec4f(linear, 1.0);
}

/// The sRGB transfer function: the gamma encoded value into the linear
/// one.
fn srgb_to_linear(color: vec3f) -> vec3f {
    let cutoff = vec3f(0.04045);
    let low = color / 12.92;
    let high = pow((color + vec3f(0.055)) / 1.055, vec3f(2.4));
    return select(high, low, color <= cutoff);
}
