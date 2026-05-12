use std::collections::BTreeMap;

use crate::scene::loader::assets_loader::MiscBucket;

pub const WM_SAMPLER_BINDING: u32 = 1;

const HEADER_NAMES: &[&str] = &[
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

/// Load all shader header files from the wallpaper engine assets bucket.
///
/// Headers are stored under `shaders/common.h`, `shaders/common_fragment.h`, etc.
/// in the same `.pkg` (or assets directory) as the shader `.frag`/`.vert` files.
/// Returns a map of bare filename → file content.
pub fn get_headers(misc: &MiscBucket) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();

    for name in HEADER_NAMES {
        let key = format!("shaders/{}", name);
        match misc.get(&key) {
            Some(bytes) => match String::from_utf8(bytes) {
                Ok(content) => {
                    map.insert(name.to_string(), content);
                }
                Err(e) => {
                    eprintln!(
                        "Warning: shader header '{}' is not valid UTF-8: {}",
                        key, e
                    );
                }
            },
            None => {
                eprintln!("Warning: shader header '{}' not found in assets", key);
            }
        }
    }

    map
}
