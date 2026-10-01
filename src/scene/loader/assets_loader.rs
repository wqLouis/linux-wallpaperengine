//! Typed asset store for Wallpaper Engine scenes.
//!
//! Every file in a `.pkg` (or the Wallpaper Engine `assets/` fallback
//! directory) is classified into an [`AssetType`] and stored as a parsed
//! [`Asset`]. Shaders are pre-parsed into [`ShaderProgram`]s when the scene is
//! prepared, so pipeline compilation does not have to re-read or re-scan the
//! sources.
//!
//! Callers use the typed accessors ([`AssetStore::texture`],
//! [`AssetStore::json`], [`AssetStore::model`], [`AssetStore::shader`], …)
//! rather than a catch-all byte bucket.

use std::{cell::RefCell, collections::BTreeMap, fs, path::PathBuf, rc::Rc};

use pkg_parser::pkg_parser::{mdl_parser::MdlFile, tex_parser::Tex};

use super::shader::{self, ShaderProgram};

// ---------------------------------------------------------------------------
// Asset classification
// ---------------------------------------------------------------------------

/// Every kind of asset a Wallpaper Engine scene can contain.
///
/// Classification is driven by the file extension, with JSON documents split
/// out by their directory so callers can tell effects, materials, models,
/// particles and presets apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssetType {
    /// `scene.json`
    Scene,
    /// Effects definitions under `effects/`
    Effect,
    /// Material definitions under `materials/`
    Material,
    /// Puppet model binaries (`.mdl`)
    Model,
    /// Particle systems under `particles/`
    Particle,
    /// Presets under `presets/`
    Preset,
    /// GLSL shader sources and headers (`.frag`, `.vert`, `.h`, …)
    Shader,
    /// `.tex` textures
    Texture,
    /// Audio files (`.ogg`, `.mp3`, `.wav`, `.flac`)
    Sound,
    /// Font files (`.ttf`, `.otf`)
    Font,
    /// Video textures (`.mp4`, `.webm`, `.gif`, …)
    Video,
    /// JavaScript scripts (`.js`)
    Script,
    /// Any other JSON document
    Json,
    /// Anything else
    Other,
}

impl AssetType {
    /// Classify an asset by its package-relative path.
    pub fn from_path(path: &str) -> Self {
        let ext = path
            .rsplit_once('.')
            .map(|(_, e)| e)
            .unwrap_or("")
            .to_ascii_lowercase();

        match ext.as_str() {
            "tex" => Self::Texture,
            "mdl" => Self::Model,
            "frag" | "vert" | "h" | "glsl" | "wgsl" => Self::Shader,
            "json" => Self::classify_json(path),
            "ogg" | "mp3" | "wav" | "flac" => Self::Sound,
            "ttf" | "otf" => Self::Font,
            "mp4" | "webm" | "gif" | "avi" => Self::Video,
            "js" => Self::Script,
            _ => Self::Other,
        }
    }

    fn classify_json(path: &str) -> Self {
        if path.ends_with("scene.json") {
            Self::Scene
        } else if path.starts_with("effects/") {
            Self::Effect
        } else if path.starts_with("materials/") {
            Self::Material
        } else if path.starts_with("particles/") {
            Self::Particle
        } else if path.starts_with("presets/") {
            Self::Preset
        } else {
            Self::Json
        }
    }
}

// ---------------------------------------------------------------------------
// Parsed assets
// ---------------------------------------------------------------------------

/// A single parsed asset.
#[derive(Clone)]
pub enum Asset {
    Texture(Rc<Tex>),
    Model(Rc<MdlFile>),
    /// JSON-backed documents (scene, effects, materials, …).
    Json(Rc<String>),
    /// JavaScript sources.
    Script(Rc<String>),
    /// A pre-parsed shader program, keyed by its base path (no extension).
    Shader(Rc<ShaderProgram>),
    /// Shader source or header text that has not been paired into a program yet.
    Text(Rc<String>),
    Sound(Vec<u8>),
    Font(Vec<u8>),
    Video(Vec<u8>),
    /// Unclassified binary payload.
    Raw(Vec<u8>),
}

impl Asset {
    /// Parse a raw byte payload according to its path.
    pub fn parse(key: &str, bytes: &[u8]) -> Option<Self> {
        match AssetType::from_path(key) {
            AssetType::Texture => {
                let mut tex = Tex::new(bytes)?;
                tex.parse_to_rgba()?;
                tex.build_mip_chain();
                Some(Self::Texture(Rc::new(tex)))
            }
            AssetType::Model => Some(Self::Model(Rc::new(MdlFile::new(bytes)?))),
            AssetType::Shader => Some(Self::Text(Rc::new(
                String::from_utf8_lossy(bytes).into_owned(),
            ))),
            AssetType::Script => Some(Self::Script(Rc::new(
                String::from_utf8_lossy(bytes).into_owned(),
            ))),
            AssetType::Sound => Some(Self::Sound(bytes.to_vec())),
            AssetType::Font => Some(Self::Font(bytes.to_vec())),
            AssetType::Video => Some(Self::Video(bytes.to_vec())),
            AssetType::Other => Some(Self::Raw(bytes.to_vec())),
            // Scene, effect, material, particle, preset and generic JSON.
            _ => Some(Self::Json(Rc::new(
                String::from_utf8_lossy(bytes).into_owned(),
            ))),
        }
    }

    fn as_bytes(&self) -> Option<Vec<u8>> {
        match self {
            Self::Raw(b) | Self::Sound(b) | Self::Font(b) | Self::Video(b) => Some(b.clone()),
            Self::Text(t) | Self::Json(t) | Self::Script(t) => Some(t.as_bytes().to_vec()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Asset store
// ---------------------------------------------------------------------------

/// Keyed store of parsed assets with lazy disk fallback and shader parsing.
pub struct AssetStore {
    map: RefCell<BTreeMap<String, Asset>>,
    headers: RefCell<Option<Rc<BTreeMap<String, String>>>>,
    assets_path: Option<PathBuf>,
}

impl AssetStore {
    pub fn new(map: BTreeMap<String, Asset>, assets_path: Option<PathBuf>) -> Self {
        Self {
            map: RefCell::new(map),
            headers: RefCell::new(None),
            assets_path,
        }
    }

    pub fn set_assets_path(&mut self, path: Option<PathBuf>) {
        self.assets_path = path;
        *self.headers.borrow_mut() = None;
    }

    // -- general lookup ----------------------------------------------------

    /// Look up an asset, lazily parsing it from the assets directory if needed.
    pub fn get(&self, key: &str) -> Option<Asset> {
        if let Some(asset) = self.map.borrow().get(key) {
            return Some(asset.clone());
        }
        let bytes = self.read_disk(key)?;
        let asset = Asset::parse(key, &bytes)?;
        self.map.borrow_mut().insert(key.to_string(), asset.clone());
        Some(asset)
    }

    // -- typed accessors ---------------------------------------------------

    pub fn texture(&self, key: &str) -> Option<Rc<Tex>> {
        match self.get(key)? {
            Asset::Texture(t) => Some(t),
            _ => None,
        }
    }

    pub fn model(&self, key: &str) -> Option<Rc<MdlFile>> {
        match self.get(key)? {
            Asset::Model(m) => Some(m),
            _ => None,
        }
    }

    /// JSON-backed document (scene, effect, material, …).
    pub fn json(&self, key: &str) -> Option<Rc<String>> {
        match self.get(key)? {
            Asset::Json(t) => Some(t),
            // Shader/script text can still be requested as JSON by mistake;
            // fall back to the text representation.
            Asset::Text(t) | Asset::Script(t) => Some(t),
            _ => None,
        }
    }

    /// Consume an audio asset, loading it from disk without caching.
    pub fn take_sound(&self, key: &str) -> Option<Vec<u8>> {
        if let Some(asset) = self.map.borrow_mut().remove(key)
            && let Some(bytes) = asset.as_bytes() {
                return Some(bytes);
            }
        self.read_disk(key)
    }

    // -- shaders -----------------------------------------------------------

    /// Return the pre-parsed shader program for a `.frag`/`.vert` pair.
    ///
    /// The program is parsed on first use (and eagerly by
    /// [`AssetStore::preparse_shaders`]).
    pub fn shader(&self, frag_path: &str, vert_path: &str) -> Option<Rc<ShaderProgram>> {
        let base = frag_path.trim_end_matches(".frag");
        if let Some(asset) = self.map.borrow().get(base)
            && let Asset::Shader(program) = asset {
                return Some(Rc::clone(program));
            }

        let vertex = self.source_text(vert_path).unwrap_or_default();
        let fragment = self.source_text(frag_path).unwrap_or_default();
        if vertex.is_empty() && fragment.is_empty() {
            return None;
        }

        let program = Rc::new(ShaderProgram::parse(
            vertex,
            fragment,
            self.shared_headers(),
        ));
        self.map
            .borrow_mut()
            .insert(base.to_string(), Asset::Shader(Rc::clone(&program)));
        Some(program)
    }

    /// Eagerly parse every shader pair present in the store.
    pub fn preparse_shaders(&self) {
        let keys: Vec<String> = self.map.borrow().keys().cloned().collect();
        let mut bases: Vec<String> = Vec::new();
        for key in keys {
            if let Some(base) = key.strip_suffix(".frag")
                && !bases.iter().any(|b| b == base) {
                    bases.push(base.to_string());
                }
        }
        for base in bases {
            self.shader(&format!("{base}.frag"), &format!("{base}.vert"));
        }
    }

    /// Built-in shader headers, loaded once and shared across programs.
    fn shared_headers(&self) -> Rc<BTreeMap<String, String>> {
        {
            let cached = self.headers.borrow();
            if let Some(headers) = cached.as_ref() {
                return Rc::clone(headers);
            }
        }
        let headers = Rc::new(shader::get_headers(|key| self.read_raw(key)));
        *self.headers.borrow_mut() = Some(Rc::clone(&headers));
        headers
    }

    // -- internal helpers --------------------------------------------------

    /// Read text for a shader source/header, caching it as [`Asset::Text`].
    fn source_text(&self, key: &str) -> Option<String> {
        if let Some(asset) = self.map.borrow().get(key) {
            match asset {
                Asset::Text(t) | Asset::Json(t) | Asset::Script(t) => return Some(t.to_string()),
                Asset::Raw(b) => return Some(String::from_utf8_lossy(b).into_owned()),
                _ => {}
            }
        }
        let bytes = self.read_disk(key)?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        self.map
            .borrow_mut()
            .insert(key.to_string(), Asset::Text(Rc::new(text.clone())));
        Some(text)
    }

    /// Raw bytes for a known key, checking parsed text before disk.
    fn read_raw(&self, key: &str) -> Option<Vec<u8>> {
        if let Some(bytes) = self.map.borrow().get(key).and_then(Asset::as_bytes) {
            return Some(bytes);
        }
        self.read_disk(key)
    }

    fn read_disk(&self, key: &str) -> Option<Vec<u8>> {
        let path = self.assets_path.as_ref()?.join(key);
        fs::read(path).ok()
    }
}

