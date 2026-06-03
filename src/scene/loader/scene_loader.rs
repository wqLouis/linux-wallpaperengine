use indicatif::ProgressBar;
use pkg_parser::pkg_parser::{mdl_parser::MdlFile, parser::Pkg, tex_parser::Tex};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

use super::assets_loader::{JsonBucket, MdlBucket, MiscBucket, TextureBucket};

pub struct Scene {
    pub root: crate::scene::loader::scene::Root,
    pub textures: TextureBucket,
    pub mdls: MdlBucket,
    pub jsons: JsonBucket,
    pub misc: MiscBucket,
}

impl Scene {
    /// Load a wallpaper scene from a .pkg file.
    ///
    /// Panics if the .pkg file is unreadable or `scene.json` is missing/
    /// invalid — there is no meaningful fallback without a scene definition.
    /// Individual asset failures (textures, etc.) are logged and skipped
    /// gracefully.
    pub fn new(path: String) -> Self {
        let path = Path::new(&path);
        let pkg = Pkg::new(path).unwrap_or_else(|e| {
            panic!("Failed to load PKG file '{}': {}", path.display(), e);
        });

        let texs: Arc<Mutex<BTreeMap<String, Tex>>> = Arc::new(Mutex::new(BTreeMap::new()));
        let mut mdls_map: BTreeMap<String, Rc<MdlFile>> = BTreeMap::new();
        let mut jsons: BTreeMap<String, String> = BTreeMap::new();
        let mut misc: BTreeMap<String, Vec<u8>> = BTreeMap::new();

        let mut handles: Vec<JoinHandle<()>> = Vec::new();
        let pb = ProgressBar::new(pkg.files.len() as u64);

        for (key, val) in pkg.files.into_iter() {
            let file_path = Path::new(&key);
            let ext = file_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            match ext {
                "tex" => {
                    let key_clone = key.clone();
                    let key_for_log = key.clone();
                    let texs = Arc::clone(&texs);

                    let handle = thread::spawn(move || {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let mut tex = match Tex::new(&val) {
                                Some(t) => t,
                                None => {
                                    log::warn!("pkg: failed to parse texture: {}", key_clone);
                                    return;
                                }
                            };

                            match tex.parse_to_rgba() {
                                Some(_) => {}
                                None => {
                                    log::warn!("pkg: failed to convert texture to RGBA: {}", key_clone);
                                    return;
                                }
                            };

                            tex.build_mip_chain();

                            texs.lock().unwrap().insert(key_clone.clone(), tex);
                            log::debug!("pkg: loaded tex: {}", key_clone);
                        }));

                        if let Err(e) = result {
                            let msg = if let Some(s) = e.downcast_ref::<String>() {
                                s.clone()
                            } else if let Some(s) = e.downcast_ref::<&str>() {
                                s.to_string()
                            } else {
                                "unknown panic".to_string()
                            };
                            log::warn!("pkg: texture thread panicked for '{}': {}", key_for_log, msg);
                        }
                    });

                    log::debug!("pkg: enqueued tex: {}", key);
                    handles.push(handle);
                }

                "mdl" => {
                    pb.inc(1);
                    match MdlFile::new(&val) {
                        Some(mdl) => {
                            log::debug!("pkg: loaded mdl: {} ({} records, {} quads)",
                                key, mdl.data.records.len(), mdl.data.quads.len());
                            mdls_map.insert(key, Rc::new(mdl));
                        }
                        None => {
                            log::warn!("pkg: failed to parse mdl: {}", key);
                            // Keep raw bytes in misc as fallback
                            misc.insert(key, val);
                        }
                    }
                }
                "json" => {
                    pb.inc(1);
                    log::debug!("pkg: loaded json: {}", key);
                    jsons.insert(key, String::from_utf8_lossy(&val).to_string());
                }
                _ => {
                    pb.inc(1);
                    log::debug!("pkg: loaded misc: {}", key);
                    misc.insert(key, val);
                }
            }
        }

        for handle in handles {
            match handle.join() {
                Ok(()) => {}
                Err(e) => {
                    let msg = if let Some(s) = e.downcast_ref::<String>() {
                        s.as_str()
                    } else if let Some(s) = e.downcast_ref::<&str>() {
                        s
                    } else {
                        "unknown panic"
                    };
                    log::warn!("pkg: texture thread panicked: {}", msg);
                }
            }
            pb.inc(1);
        }

        pb.finish_and_clear();

        let scene_string = jsons.get("scene.json")
            .unwrap_or_else(|| panic!("scene.json not found in PKG archive"));
        let root: crate::scene::loader::scene::Root =
            serde_json::from_str(scene_string)
                .unwrap_or_else(|e| panic!("Failed to parse scene.json: {}", e));

        let mut texs_locked = texs.lock().unwrap();
        let texs = std::mem::take(&mut *texs_locked)
            .into_iter()
            .map(|(k, v)| (k, Rc::new(v)))
            .collect::<BTreeMap<String, Rc<Tex>>>();

        Self {
            root,
            textures: TextureBucket::new(texs, None),
            mdls: MdlBucket::new(mdls_map, None),
            jsons: JsonBucket::new(jsons, None),
            misc: MiscBucket::new(misc, None),
        }
    }

    /// Set the Wallpaper Engine assets directory for lazy-loading fallback.
    ///
    /// When a requested asset is not found in the in-memory buckets
    /// (populated from the `.pkg` file), the bucket wrappers will attempt
    /// to read it from `{assets_path}/{key}` on disk, parse it, cache it,
    /// and return it.
    pub fn set_assets_path(&mut self, assets_path: PathBuf) {
        let path = Some(assets_path);
        self.textures.set_assets_path(path.clone());
        self.mdls.set_assets_path(path.clone());
        self.jsons.set_assets_path(path.clone());
        self.misc.set_assets_path(path);
    }
}
