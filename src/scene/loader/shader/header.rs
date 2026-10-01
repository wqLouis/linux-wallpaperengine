use std::collections::BTreeMap;

pub const WM_SAMPLER_BINDING: u32 = 1;

pub const HEADER_NAMES: &[&str] = &[
    "common.h",
    "common_perspective.h",
    "common_blending.h",
    "common_composite.h",
    "common_blur.h",
    "common_fragment.h",
    "common_vertex.h",
    "common_fog.h",
    "common_foliage.h",
    "common_particles.h",
    "common_pbr.h",
    "common_pbr_2.h",
];

/// Load all built-in shader headers through the supplied byte loader.
///
/// Headers are stored under `shaders/common.h`, `shaders/common_fragment.h`,
/// etc. in the scene package (or the Wallpaper Engine assets directory).
/// Returns a map of bare filename → file content. Any header reported missing
/// by `load` is skipped with a warning.
pub fn get_headers(load: impl Fn(&str) -> Option<Vec<u8>>) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();

    for name in HEADER_NAMES {
        let key = format!("shaders/{}", name);
        match load(&key) {
            Some(bytes) => match String::from_utf8(bytes) {
                Ok(content) => {
                    map.insert(name.to_string(), content);
                }
                Err(e) => {
                    eprintln!("Warning: shader header '{}' is not valid UTF-8: {}", key, e);
                }
            },
            None => {
                eprintln!("Warning: shader header '{}' not found in assets", key);
            }
        }
    }

    map
}
