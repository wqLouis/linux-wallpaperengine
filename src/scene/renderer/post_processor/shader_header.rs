use std::collections::BTreeMap;

const COMMON_H: &str = r#"#define M_PI 3.14159265359
#define M_PI_HALF 1.57079632679
#define M_PI_2 6.28318530718
#define SQRT_2 1.41421356237
#define SQRT_3 1.73205080756

vec3 hsv2rgb(vec3 c) {
    vec4 K = vec4(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    vec3 p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, 0.0, 1.0), c.y);
}

vec3 rgb2hsv(vec3 c) {
    vec4 K = vec4(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    vec4 p = mix(vec4(c.bg, K.wz), vec4(c.gb, K.xy), step(c.b, c.g));
    vec4 q = mix(vec4(p.xyw, c.r), vec4(c.r, p.yzx), step(p.x, c.r));
    float d = q.x - min(q.w, q.y);
    float e = 1.0e-10;
    return vec3(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

vec2 rotateVec2(vec2 v, float r) {
    return vec2(v.x * cos(r) - v.y * sin(r), v.x * sin(r) + v.y * cos(r));
}

float greyscale(vec3 c) {
    return dot(c, vec3(0.299, 0.587, 0.114));
}
"#;

const COMMON_PERSPECTIVE_H: &str = r#"
mat3 squareToQuad(vec2 p0, vec2 p1, vec2 p2, vec2 p3) {
    float dx1 = p1.x - p2.x;
    float dy1 = p1.y - p2.y;
    float dx2 = p3.x - p2.x;
    float dy2 = p3.y - p2.y;
    float dx3 = p0.x - p1.x + p2.x - p3.x;
    float dy3 = p0.y - p1.y + p2.y - p3.y;

    float det = dx1 * dy2 - dy1 * dx2;
    if (abs(det) < 1e-10) {
        return mat3(1.0);
    }

    float g = (dx3 * dy2 - dy3 * dx2) / det;
    float h = (dx1 * dy3 - dy1 * dx3) / det;

    return mat3(
        p1.x - p0.x + g * p1.x,
        p3.x - p0.x + h * p3.x,
        p0.x,
        p1.y - p0.y + g * p1.y,
        p3.y - p0.y + h * p3.y,
        p0.y,
        g,
        h,
        1.0
    );
}
"#;

pub const WM_SAMPLER_BINDING: u32 = 1;

pub fn get_headers() -> BTreeMap<&'static str, &'static str> {
    let mut map = BTreeMap::new();
    map.insert("common.h", COMMON_H);
    map.insert("common_perspective.h", COMMON_PERSPECTIVE_H);
    map
}
