//! Shader programs parsed once when a scene is loaded.
//!
//! Wallpaper Engine effects reference a `.frag`/`.vert` pair by a shared base
//! name. Parsing the pair (interface layout, default combos, source macros) is
//! independent of the runtime combo values, so it is done up-front by the
//! loader instead of on every pipeline compilation.

use std::{collections::BTreeMap, rc::Rc};

use super::{EffectLayout, collect_default_defines, collect_layout, collect_source_defines};

/// A parsed vertex/fragment shader pair.
#[derive(Debug, Clone)]
pub struct ShaderProgram {
    pub vertex: String,
    pub fragment: String,
    /// Shared built-in headers, resolved during parsing.
    pub headers: Rc<BTreeMap<String, String>>,
    /// Combined shader interface layout (samplers, uniforms, varyings).
    pub layout: EffectLayout,
    /// Default values from `[COMBO]` annotations in either stage.
    pub default_defines: BTreeMap<String, String>,
    /// Plain `#define` macros collected from either stage.
    pub source_defines: BTreeMap<String, String>,
}

impl ShaderProgram {
    /// Parse a shader pair. Either stage may be empty when a shader only has
    /// one stage.
    pub fn parse(vertex: String, fragment: String, headers: Rc<BTreeMap<String, String>>) -> Self {
        let layout = collect_layout(&vertex, &fragment, &headers);
        let default_defines = collect_default_defines(&vertex, &fragment);

        // Vertex macros take priority over fragment macros, matching the
        // behaviour of `preprocess_pair_with_layout`.
        let mut source_defines = collect_source_defines(&vertex);
        for (name, value) in collect_source_defines(&fragment) {
            source_defines.entry(name).or_insert(value);
        }

        Self {
            vertex,
            fragment,
            headers,
            layout,
            default_defines,
            source_defines,
        }
    }
}
