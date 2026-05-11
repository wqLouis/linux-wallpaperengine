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

// ---------------------------------------------------------------------------
// Texture bucket
// ---------------------------------------------------------------------------

/// Lazily-loaded bucket of `.tex` textures.
pub struct TextureBucket {
    pub(crate) map: RefCell<BTreeMap<String, Rc<Tex>>>,
    assets_path: Option<PathBuf>,
}

impl TextureBucket {
    /// Create a bucket from an initial map and an optional assets path.
    pub fn new(map: BTreeMap<String, Rc<Tex>>, assets_path: Option<PathBuf>) -> Self {
        Self {
            map: RefCell::new(map),
            assets_path,
        }
    }

    /// Set the assets path for fallback disk loading.
    pub fn set_assets_path(&mut self, path: Option<PathBuf>) {
        self.assets_path = path;
    }

    /// Look up a texture by key.
    ///
    /// If the key is not in the in-memory map and an `assets_path` is
    /// configured, the file is read from `{assets_path}/{key}`, parsed,
    /// cached, and returned.
    pub fn get(&self, key: &str) -> Option<Rc<Tex>> {
        // Fast path: already cached
        {
            let map = self.map.borrow();
            if let Some(val) = map.get(key) {
                log::trace!("texture bucket: cache hit for '{}'", key);
                return Some(Rc::clone(val));
            }
        }

        // Lazy-load from the assets directory
        let assets_path = match self.assets_path.as_ref() {
            Some(p) => p,
            None => {
                log::trace!("texture bucket: '{}' not found (no assets path)", key);
                return None;
            }
        };
        let file_path = assets_path.join(key);
        log::debug!("assets: loading tex '{}' from {}", key, file_path.display());

        let bytes = match fs::read(&file_path) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("assets: tex '{}' not found at {}: {}", key, file_path.display(), e);
                return None;
            }
        };
        let mut tex = match Tex::new(&bytes) {
            Some(t) => t,
            None => {
                log::warn!("assets: failed to parse tex '{}' from {}", key, file_path.display());
                return None;
            }
        };
        if tex.parse_to_rgba().is_none() {
            log::warn!("assets: failed to decode tex '{}' to RGBA", key);
            return None;
        }

        let rc = Rc::new(tex);
        log::debug!("assets: cached tex '{}' ({}x{})", key, rc.dimension[0], rc.dimension[1]);
        self.map.borrow_mut().insert(key.to_string(), Rc::clone(&rc));
        Some(rc)
    }
}

// ---------------------------------------------------------------------------
// MDL bucket
// ---------------------------------------------------------------------------

/// Lazily-loaded bucket of `.mdl` puppet model files.
#[allow(dead_code)]
pub struct MdlBucket {
    pub(crate) map: RefCell<BTreeMap<String, Rc<MdlFile>>>,
    assets_path: Option<PathBuf>,
}

#[allow(dead_code)]
impl MdlBucket {
    pub fn new(map: BTreeMap<String, Rc<MdlFile>>, assets_path: Option<PathBuf>) -> Self {
        Self {
            map: RefCell::new(map),
            assets_path,
        }
    }

    /// Set the assets path for fallback disk loading.
    pub fn set_assets_path(&mut self, path: Option<PathBuf>) {
        self.assets_path = path;
    }

    /// Look up a model by key, lazy-loading from disk if needed.
    pub fn get(&self, key: &str) -> Option<Rc<MdlFile>> {
        {
            let map = self.map.borrow();
            if let Some(val) = map.get(key) {
                log::trace!("mdl bucket: cache hit for '{}'", key);
                return Some(Rc::clone(val));
            }
        }

        let assets_path = match self.assets_path.as_ref() {
            Some(p) => p,
            None => {
                log::trace!("mdl bucket: '{}' not found (no assets path)", key);
                return None;
            }
        };
        let file_path = assets_path.join(key);
        log::debug!("assets: loading mdl '{}' from {}", key, file_path.display());

        let bytes = match fs::read(&file_path) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("assets: mdl '{}' not found at {}: {}", key, file_path.display(), e);
                return None;
            }
        };
        let mdl = match MdlFile::new(&bytes) {
            Some(m) => m,
            None => {
                log::warn!("assets: failed to parse mdl '{}' from {}", key, file_path.display());
                return None;
            }
        };

        let rc = Rc::new(mdl);
        log::debug!("assets: cached mdl '{}'", key);
        self.map.borrow_mut().insert(key.to_string(), Rc::clone(&rc));
        Some(rc)
    }
}

// ---------------------------------------------------------------------------
// JSON bucket
// ---------------------------------------------------------------------------

/// Lazily-loaded bucket of `.json` files (stored as `Rc<String>`).
pub struct JsonBucket {
    pub(crate) map: RefCell<BTreeMap<String, Rc<String>>>,
    assets_path: Option<PathBuf>,
}

impl JsonBucket {
    pub fn new(map: BTreeMap<String, String>, assets_path: Option<PathBuf>) -> Self {
        let map = map
            .into_iter()
            .map(|(k, v)| (k, Rc::new(v)))
            .collect::<BTreeMap<String, Rc<String>>>();
        Self {
            map: RefCell::new(map),
            assets_path,
        }
    }

    /// Set the assets path for fallback disk loading.
    pub fn set_assets_path(&mut self, path: Option<PathBuf>) {
        self.assets_path = path;
    }

    /// Look up a JSON file by key, lazy-loading from disk if needed.
    pub fn get(&self, key: &str) -> Option<Rc<String>> {
        {
            let map = self.map.borrow();
            if let Some(val) = map.get(key) {
                log::trace!("json bucket: cache hit for '{}'", key);
                return Some(Rc::clone(val));
            }
        }

        let assets_path = match self.assets_path.as_ref() {
            Some(p) => p,
            None => {
                log::trace!("json bucket: '{}' not found (no assets path)", key);
                return None;
            }
        };
        let file_path = assets_path.join(key);
        log::debug!("assets: loading json '{}' from {}", key, file_path.display());

        let bytes = match fs::read(&file_path) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("assets: json '{}' not found at {}: {}", key, file_path.display(), e);
                return None;
            }
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();

        let rc = Rc::new(text);
        log::debug!("assets: cached json '{}' ({} bytes)", key, bytes.len());
        self.map.borrow_mut().insert(key.to_string(), Rc::clone(&rc));
        Some(rc)
    }
}

// ---------------------------------------------------------------------------
// Misc bucket (binary files: shaders, audio, fonts, …)
// ---------------------------------------------------------------------------

/// Lazily-loaded bucket of miscellaneous binary assets.
pub struct MiscBucket {
    pub(crate) map: RefCell<BTreeMap<String, Vec<u8>>>,
    assets_path: Option<PathBuf>,
}

impl MiscBucket {
    pub fn new(map: BTreeMap<String, Vec<u8>>, assets_path: Option<PathBuf>) -> Self {
        Self {
            map: RefCell::new(map),
            assets_path,
        }
    }

    /// Set the assets path for fallback disk loading.
    pub fn set_assets_path(&mut self, path: Option<PathBuf>) {
        self.assets_path = path;
    }

    /// Look up a file by key, lazy-loading from disk if needed.
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        {
            let map = self.map.borrow();
            if let Some(val) = map.get(key) {
                log::trace!("misc bucket: cache hit for '{}'", key);
                return Some(val.clone());
            }
        }

        let assets_path = match self.assets_path.as_ref() {
            Some(p) => p,
            None => {
                log::trace!("misc bucket: '{}' not found (no assets path)", key);
                return None;
            }
        };
        let file_path = assets_path.join(key);
        log::debug!("assets: loading misc '{}' from {}", key, file_path.display());

        let bytes = match fs::read(&file_path) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("assets: misc '{}' not found at {}: {}", key, file_path.display(), e);
                return None;
            }
        };

        log::debug!("assets: cached misc '{}' ({} bytes)", key, bytes.len());
        self.map
            .borrow_mut()
            .insert(key.to_string(), bytes.clone());
        Some(bytes)
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
