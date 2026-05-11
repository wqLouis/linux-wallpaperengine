use std::collections::BTreeMap;

macro_rules! header {
    ($name:expr) => {
        include_str!(concat!("shader_headers/", $name))
    };
}

pub const WM_SAMPLER_BINDING: u32 = 1;

pub fn get_headers() -> BTreeMap<&'static str, &'static str> {
    let mut map = BTreeMap::new();
    map.insert("common.h", header!("common.h"));
    map.insert("common_perspective.h", header!("common_perspective.h"));
    map.insert("common_blending.h", header!("common_blending.h"));
    map.insert("common_composite.h", header!("common_composite.h"));
    map.insert("common_blur.h", header!("common_blur.h"));
    map.insert("common_fragment.h", header!("common_fragment.h"));
    map.insert("common_vertex.h", header!("common_vertex.h"));
    map.insert("common_fog.h", header!("common_fog.h"));
    map.insert("common_foliage.h", header!("common_foliage.h"));
    map.insert("common_particles.h", header!("common_particles.h"));
    map.insert("common_pbr.h", header!("common_pbr.h"));
    map.insert("common_pbr_2.h", header!("common_pbr_2.h"));
    map
}
