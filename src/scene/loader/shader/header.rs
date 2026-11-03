use crate::collections::{Map, new_map};
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
    "model_vertex_v1.h",
    "model_fragment_v1.h",
];

/// The directories a header can be stored in.
///
/// The shaders of the engine include a header with a path
/// (`#include "base/model_vertex_v1.h"`), and the headers themselves include
/// each other with a bare name.  A header is therefore looked up in the
/// directory of the shaders first and in the directory of the model headers
/// afterwards.
const HEADER_DIRECTORIES: &[&str] = &["shaders/", "shaders/base/"];

/// Load all built-in shader headers through the supplied byte loader.
///
/// Headers are stored under `shaders/common.h`, `shaders/common_fragment.h`,
/// etc. in the scene package (or the Wallpaper Engine assets directory).
/// The headers of the models are stored under `shaders/base/`.
/// Returns a map of bare filename → file content. Any header reported missing
/// by `load` is skipped with a warning.
pub fn get_headers(load: impl Fn(&str) -> Option<Vec<u8>>) -> Map<String, String> {
    let mut map = new_map();

    for name in HEADER_NAMES {
        let key = format!("{}{}", HEADER_DIRECTORIES[0], name);
        let mut content = None;
        for directory in HEADER_DIRECTORIES {
            let path = format!("{}{}", directory, name);
            let Some(bytes) = load(&path) else {
                continue;
            };
            match String::from_utf8(bytes) {
                Ok(text) => {
                    content = Some(text);
                    break;
                }
                Err(e) => {
                    eprintln!(
                        "Warning: shader header '{}' is not valid UTF-8: {}",
                        path, e
                    );
                    break;
                }
            }
        }

        match content {
            Some(text) => {
                map.insert(name.to_string(), text);
            }
            None => {
                eprintln!("Warning: shader header '{}' not found in assets", key);
            }
        }
    }

    map
}
