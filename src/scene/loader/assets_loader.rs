//! Lazy-loading bucket wrappers that fall back to the Wallpaper Engine
//! assets directory on disk when a requested asset is not found in memory.
//!
//! Usage:
//!
//! 1. Load a `.pkg` file normally via `Scene::new()` (populates buckets
//!    from the package).
//! 2. Call `scene.set_assets_path()` to point to the Wallpaper Engine
//!    `assets/` directory.
//! 3. When code calls `scene.textures.get(key)`, `scene.jsons.get(key)`,
//!    etc., each wrapper first checks the in-memory map.  If the key is
//!    missing, it reads the file from `{assets_path}/{key}`, parses it,
//!    caches it in the map, and returns it.
//!
//! Expected assets directory layout:
//!
//! ```text
//! assets/
//! ├── effects/     # Effect JSON definitions
//! ├── fonts/       # Font files
//! ├── materials/   # .tex textures + material JSONs
//! ├── models/      # .mdl puppet model files
//! ├── particles/   # Particle system definitions
//! ├── presets/     # Preset configurations
//! ├── scenes/      # Scene configurations
//! ├── scripts/     # JavaScript scripts
//! ├── shaders/     # GLSL shader source files (.frag, .vert)
//! └── zcompat/     # Compatibility layer files
//! ```

use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    path::PathBuf,
    rc::Rc,
};

use pkg_parser::pkg_parser::{mdl_parser::MdlFile, tex_parser::Tex};

/// Generic helper: check the in-memory cache, then lazy-load from disk.
fn load_cached<T: Clone>(
    map: &RefCell<BTreeMap<String, T>>,
    assets_path: &Option<PathBuf>,
    key: &str,
    load: impl Fn(&[u8]) -> Option<T>,
) -> Option<T> {
    if let Some(val) = map.borrow().get(key) {
        return Some(val.clone());
    }
    let file_path = assets_path.as_ref()?.join(key);
    let bytes = fs::read(&file_path).ok()?;
    let val = load(&bytes)?;
    map.borrow_mut().insert(key.to_string(), val.clone());
    Some(val)
}

// ---------------------------------------------------------------------------
// Texture bucket
// ---------------------------------------------------------------------------

/// Lazily-loaded bucket of `.tex` textures.
pub struct TextureBucket {
    pub(crate) map: RefCell<BTreeMap<String, Rc<Tex>>>,
    assets_path: Option<PathBuf>,
}

impl TextureBucket {
    pub fn new(map: BTreeMap<String, Rc<Tex>>, assets_path: Option<PathBuf>) -> Self {
        Self { map: RefCell::new(map), assets_path }
    }

    pub fn set_assets_path(&mut self, path: Option<PathBuf>) {
        self.assets_path = path;
    }

    pub fn get(&self, key: &str) -> Option<Rc<Tex>> {
        load_cached(&self.map, &self.assets_path, key, |bytes| {
            let mut tex = Tex::new(bytes)?;
            tex.parse_to_rgba()?;
            log::debug!("assets: loaded tex '{}' ({}x{})", key, tex.dimension[0], tex.dimension[1]);
            Some(Rc::new(tex))
        })
    }
}

// ---------------------------------------------------------------------------
// MDL bucket
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub struct MdlBucket {
    pub(crate) map: RefCell<BTreeMap<String, Rc<MdlFile>>>,
    assets_path: Option<PathBuf>,
}

#[allow(dead_code)]
impl MdlBucket {
    pub fn new(map: BTreeMap<String, Rc<MdlFile>>, assets_path: Option<PathBuf>) -> Self {
        Self { map: RefCell::new(map), assets_path }
    }

    pub fn set_assets_path(&mut self, path: Option<PathBuf>) {
        self.assets_path = path;
    }

    pub fn get(&self, key: &str) -> Option<Rc<MdlFile>> {
        load_cached(&self.map, &self.assets_path, key, |bytes| {
            let mdl = MdlFile::new(bytes)?;
            log::debug!("assets: loaded mdl '{}'", key);
            Some(Rc::new(mdl))
        })
    }
}

// ---------------------------------------------------------------------------
// JSON bucket
// ---------------------------------------------------------------------------

pub struct JsonBucket {
    pub(crate) map: RefCell<BTreeMap<String, Rc<String>>>,
    assets_path: Option<PathBuf>,
}

impl JsonBucket {
    pub fn new(map: BTreeMap<String, String>, assets_path: Option<PathBuf>) -> Self {
        let map = map.into_iter().map(|(k, v)| (k, Rc::new(v))).collect();
        Self { map: RefCell::new(map), assets_path }
    }

    pub fn set_assets_path(&mut self, path: Option<PathBuf>) {
        self.assets_path = path;
    }

    pub fn get(&self, key: &str) -> Option<Rc<String>> {
        load_cached(&self.map, &self.assets_path, key, |bytes| {
            let text = String::from_utf8_lossy(bytes).into_owned();
            log::debug!("assets: loaded json '{}' ({} bytes)", key, bytes.len());
            Some(Rc::new(text))
        })
    }
}

// ---------------------------------------------------------------------------
// Misc bucket (binary files: shaders, audio, fonts, …)
// ---------------------------------------------------------------------------

pub struct MiscBucket {
    pub(crate) map: RefCell<BTreeMap<String, Vec<u8>>>,
    assets_path: Option<PathBuf>,
}

impl MiscBucket {
    pub fn new(map: BTreeMap<String, Vec<u8>>, assets_path: Option<PathBuf>) -> Self {
        Self { map: RefCell::new(map), assets_path }
    }

    pub fn set_assets_path(&mut self, path: Option<PathBuf>) {
        self.assets_path = path;
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        load_cached(&self.map, &self.assets_path, key, |bytes| {
            log::debug!("assets: loaded misc '{}' ({} bytes)", key, bytes.len());
            Some(bytes.to_vec())
        })
    }

    /// Remove a file from the bucket (used for audio consumption).
    pub fn remove(&self, key: &str) -> Option<Vec<u8>> {
        // Try in-memory map first
        {
            let mut map = self.map.borrow_mut();
            if let Some(val) = map.remove(key) {
                log::debug!("misc bucket: removed '{}' from cache (audio)", key);
                return Some(val);
            }
        }

        // Lazy-load from disk, return without caching (caller takes ownership)
        let assets_path = match self.assets_path.as_ref() {
            Some(p) => p,
            None => {
                log::trace!("misc bucket: '{}' not found for remove (no assets path)", key);
                return None;
            }
        };
        let file_path = assets_path.join(key);
        log::debug!("assets: loading misc (remove) '{}' from {}", key, file_path.display());

        match fs::read(&file_path) {
            Ok(bytes) => {
                log::debug!("assets: loaded (remove) '{}' ({} bytes)", key, bytes.len());
                Some(bytes)
            }
            Err(e) => {
                log::warn!("assets: misc '{}' not found for remove at {}: {}", key, file_path.display(), e);
                None
            }
        }
    }
}
