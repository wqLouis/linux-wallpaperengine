//! Typed asset store for Wallpaper Engine scenes.
//!
//! Every file in a `.pkg` (or the Wallpaper Engine `assets/` fallback
//! directory) is classified into an [`AssetType`] and stored as a parsed
//! [`Asset`]. Shaders are pre-parsed into [`ShaderProgram`]s when the scene is
//! prepared, so pipeline compilation does not have to re-read or re-scan the
//! sources.
//!
//! Callers use the typed accessors ([`AssetStore::texture`],
//! [`AssetStore::json`], [`AssetStore::model`], [`AssetStore::video`],
//! [`AssetStore::shader`], …) rather than a catch-all byte bucket.

use std::{cell::RefCell, fs, path::PathBuf, rc::Rc};

use pkg_parser::pkg_parser::{
    mdl_parser::MdlFile,
    tex_parser::Tex,
    video_parser::{VideoFrame, VideoInfo, VideoPlayer, decode_all, detect_format},
};

use super::project::{Project, parse_project};
use super::shader::{self, ShaderProgram};
use crate::collections::{Map, sorted_keys};

/// The built-in vertex stage of an effect.
///
/// An effect of Wallpaper Engine that has no vertex shader of its own
/// draws a full screen quad: this is the vertex stage of the reference
/// engine.  It is written in the dialect of the engine, so the
/// preprocessing of the pipeline handles it like any other vertex shader.
const BUILTIN_VERTEX_SHADER: &str = include_str!("shader/builtin.vert");

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

/// A video (or animated image) asset: the raw bytes, the probed metadata
/// and — when the clip fits into the memory budget of the parser — the
/// decoded frames.
///
/// The container bytes are kept behind an [`Rc`], so the asset can be
/// shared between the loader and the renderer without copying the video
/// around.  The clip is decoded once, when the asset is parsed: the
/// renderer plays the frames back by index and never decodes a frame
/// again.
#[derive(Clone, Debug)]
pub struct VideoAsset {
    /// The raw container bytes (mp4/webm/gif).
    pub bytes: Rc<[u8]>,
    /// Container metadata, probed once when the asset is parsed.
    pub info: VideoInfo,
    /// The decoded frames, when the clip was decoded eagerly.
    ///
    /// The frames are `None` when the clip does not fit into the memory
    /// budget of the parser: the renderer then streams the clip with a
    /// [`VideoPlayer`], exactly like it did before.
    pub frames: Option<Rc<[VideoFrame]>>,
}

impl VideoAsset {
    /// Build a video asset from the raw bytes of a container.
    ///
    /// The clip is decoded eagerly: the frames are kept in memory and the
    /// renderer plays them back by index, without decoding anything again.
    /// The metadata comes from that decoding, so no throw-away player has
    /// to be opened to probe the clip.
    ///
    /// A clip that does not fit into the memory budget of the parser is
    /// not decoded: the asset only keeps the metadata then, and the
    /// renderer streams the clip.
    pub fn from_bytes(bytes: Rc<[u8]>) -> Self {
        match decode_all(&bytes) {
            Ok(clip) => {
                log::debug!(
                    "decoded {} frames of a {}x{} video",
                    clip.frames.len(),
                    clip.info.width,
                    clip.info.height
                );
                Self {
                    bytes,
                    info: clip.info,
                    frames: Some(Rc::from(clip.frames)),
                }
            }
            Err(error) => {
                // The clip does not fit into the memory budget (or cannot
                // be decoded at all): the renderer streams it.
                log::debug!("the video is not decoded eagerly: {}", error);
                let info = match VideoPlayer::open(&bytes) {
                    Ok(player) => player.info().clone(),
                    Err(error) => {
                        log::warn!("cannot probe the video container: {}", error);
                        VideoInfo::default()
                    }
                };
                Self {
                    bytes,
                    info,
                    frames: None,
                }
            }
        }
    }

    /// Build a video asset from a `.tex` that carries a video container.
    ///
    /// Wallpaper Engine stores video textures as a `.tex` file whose
    /// payload is the video itself, so the payload is moved (not copied)
    /// into the asset.
    pub fn from_tex(tex: Tex) -> Self {
        Self::from_bytes(Rc::from(tex.payload))
    }

    /// The size of the video in pixels.
    pub fn size(&self) -> [u32; 2] {
        [self.info.width, self.info.height]
    }
}

/// Whether the bytes are a video container (mp4/webm/gif).
///
/// Wallpaper Engine stores video textures as a `.tex` file whose payload is
/// the video itself, so the texture parser has to recognise them before it
/// tries to decode them as pixels.
pub fn is_video_payload(bytes: &[u8]) -> bool {
    detect_format(bytes).is_some()
}

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
    /// A video (or animated image): the container bytes and its metadata.
    Video(Rc<VideoAsset>),
    /// Unclassified binary payload.
    Raw(Vec<u8>),
}

impl Asset {
    /// Parse a raw byte payload according to its path.
    pub fn parse(key: &str, bytes: &[u8]) -> Option<Self> {
        match AssetType::from_path(key) {
            AssetType::Texture => {
                let mut tex = Tex::new(bytes)?;
                // Wallpaper Engine stores video textures as a `.tex` file
                // whose payload is the video container: those are handed
                // over to the video backend instead of being decoded as
                // pixels.
                if is_video_payload(&tex.payload) {
                    return Some(Self::Video(Rc::new(VideoAsset::from_tex(tex))));
                }
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
            AssetType::Video => {
                let video = VideoAsset::from_bytes(Rc::from(bytes));
                Some(Self::Video(Rc::new(video)))
            }
            AssetType::Other => Some(Self::Raw(bytes.to_vec())),
            // Scene, effect, material, particle, preset and generic JSON.
            _ => Some(Self::Json(Rc::new(
                String::from_utf8_lossy(bytes).into_owned(),
            ))),
        }
    }

    fn as_bytes(&self) -> Option<Vec<u8>> {
        match self {
            Self::Raw(b) | Self::Sound(b) | Self::Font(b) => Some(b.clone()),
            Self::Text(t) | Self::Json(t) | Self::Script(t) => Some(t.as_bytes().to_vec()),
            // The video container is not a byte blob the callers want to
            // copy around: it is decoded through its `VideoAsset`.
            Self::Video(_) => None,
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Asset store
// ---------------------------------------------------------------------------

/// Keyed store of parsed assets with lazy disk fallback and shader parsing.
pub struct AssetStore {
    map: RefCell<Map<String, Asset>>,
    headers: RefCell<Option<Rc<Map<String, String>>>>,
    assets_path: Option<PathBuf>,
}

impl AssetStore {
    pub fn new(map: Map<String, Asset>, assets_path: Option<PathBuf>) -> Self {
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

    /// The parsed video asset, if the store has one under `key`.
    ///
    /// A video is stored under its own name (`materials/foo.mp4`) or
    /// inside a `.tex` file (Wallpaper Engine's way of storing video
    /// textures); both end up as an [`Asset::Video`].
    pub fn video(&self, key: &str) -> Option<Rc<VideoAsset>> {
        match self.get(key)? {
            Asset::Video(video) => Some(video),
            _ => None,
        }
    }

    /// The first video asset of the store, together with its key.
    ///
    /// The assets are visited in a stable (key-sorted) order, so the
    /// result does not depend on the hash table's layout.
    pub fn first_video(&self) -> Option<(String, Rc<VideoAsset>)> {
        let map = self.map.borrow();
        for key in sorted_keys(&map) {
            if let Asset::Video(video) = &map[key] {
                return Some((key.clone(), Rc::clone(video)));
            }
        }
        None
    }

    /// The videos of the store whose container could not be probed.
    ///
    /// A video that cannot be probed has no size, so the renderer cannot
    /// draw it: the loader reports it as a problem of the wallpaper.
    pub fn broken_videos(&self) -> Vec<String> {
        let map = self.map.borrow();
        sorted_keys(&map)
            .into_iter()
            .filter_map(|key| match &map[key] {
                Asset::Video(video) if video.info.width == 0 || video.info.height == 0 => {
                    Some(key.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// Insert (or replace) a parsed asset.
    pub fn insert(&self, key: &str, asset: Asset) {
        self.map.borrow_mut().insert(key.to_string(), asset);
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

    /// The wallpaper's `project.json`, if the package has one.
    ///
    /// Returns `None` when the archive has no `project.json` or when it
    /// cannot be parsed. The lookup accepts the file at the archive root and,
    /// as a fallback, any archived file named "project.json".
    pub fn project(&self) -> Option<Project> {
        if let Some(raw) = self.json("project.json") {
            return parse_project(&raw);
        }

        // Fallback: the manifest is not at the archive root.
        let key = self
            .map
            .borrow()
            .keys()
            .find(|path| {
                path.rsplit('/')
                    .next()
                    .is_some_and(|name| name == "project.json")
            })
            .cloned()?;

        let raw = self.json(&key)?;
        parse_project(&raw)
    }

    /// Consume an audio asset, loading it from disk without caching.
    pub fn take_sound(&self, key: &str) -> Option<Vec<u8>> {
        if let Some(asset) = self.map.borrow_mut().remove(key)
            && let Some(bytes) = asset.as_bytes()
        {
            return Some(bytes);
        }
        self.read_disk(key)
    }

    // -- shaders -----------------------------------------------------------

    /// Return the pre-parsed shader program for a `.frag`/`.vert` pair.
    ///
    /// The program is parsed on first use (and eagerly by
    /// [`AssetStore::preparse_shaders`]).
    ///
    /// A shader that has no vertex stage of its own gets the built-in
    /// fullscreen vertex shader: without one the shader compiler has no
    /// entry point for the vertex stage and rejects the whole program.
    pub fn shader(&self, frag_path: &str, vert_path: &str) -> Option<Rc<ShaderProgram>> {
        let base = frag_path.trim_end_matches(".frag");
        if let Some(asset) = self.map.borrow().get(base)
            && let Asset::Shader(program) = asset
        {
            return Some(Rc::clone(program));
        }

        let vertex = self.source_text(vert_path).unwrap_or_default();
        let fragment = self.source_text(frag_path).unwrap_or_default();
        if vertex.trim().is_empty() && fragment.trim().is_empty() {
            return None;
        }

        // A shader without a vertex stage draws a full screen quad: the
        // built-in vertex stage is the one of the reference engine.
        let vertex = if vertex.trim().is_empty() {
            log::warn!(
                "the shader `{}` has no vertex stage: the built-in fullscreen vertex shader is used instead",
                base
            );
            BUILTIN_VERTEX_SHADER.to_string()
        } else {
            vertex
        };

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
        let keys: Vec<String> = {
            let map = self.map.borrow();
            sorted_keys(&map).into_iter().cloned().collect()
        };
        let mut bases: Vec<String> = Vec::new();
        for key in keys {
            if let Some(base) = key.strip_suffix(".frag")
                && !bases.iter().any(|b| b == base)
            {
                bases.push(base.to_string());
            }
        }
        for base in bases {
            self.shader(&format!("{base}.frag"), &format!("{base}.vert"));
        }
    }

    /// Built-in shader headers, loaded once and shared across programs.
    fn shared_headers(&self) -> Rc<Map<String, String>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::new_map;

    /// The fragment shader of an effect that has no vertex stage.
    const FRAGMENT_ONLY: &str = "\
varying vec2 v_TexCoord;

uniform sampler2D g_Texture0;

void main() {
    gl_FragColor = texSample2D(g_Texture0, v_TexCoord);
}
";

    /// A shader that has no vertex stage gets the built-in one.
    ///
    /// Without a vertex stage the shader compiler has no entry point for
    /// the vertex stage and rejects the whole program.
    #[test]
    fn a_shader_without_a_vertex_stage_gets_the_built_in_one() {
        let mut map = new_map();
        map.insert(
            "shaders/test.frag".to_string(),
            Asset::Text(Rc::new(FRAGMENT_ONLY.to_string())),
        );
        let store = AssetStore::new(map, None);

        let program = store
            .shader("shaders/test.frag", "shaders/test.vert")
            .expect("the program of the shader");
        assert!(
            program.vertex.contains("void main()"),
            "the built-in vertex stage has no entry point:\n{}",
            program.vertex
        );

        // The built-in vertex stage is preprocessed like any other one.
        let defines = program.default_defines.clone();
        let (vertex, fragment, _) = shader::preprocess_pair_with_layout(
            &program.vertex,
            &program.fragment,
            &program.headers,
            &defines,
            &program.layout,
            false,
            0,
        );
        assert!(
            vertex.contains("void main()"),
            "the preprocessed vertex stage has no entry point:\n{vertex}"
        );
        assert!(
            fragment.contains("void main()"),
            "the preprocessed fragment stage has no entry point:\n{fragment}"
        );
    }
}
