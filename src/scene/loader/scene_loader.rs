use indicatif::ProgressBar;
use pkg_parser::pkg_parser::{parser::Pkg, tex_parser::Tex};
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
    pub fn new(path: String) -> Self {
        let path = Path::new(&path);
        let pkg = Pkg::new(path);

        let texs: Arc<Mutex<BTreeMap<String, Tex>>> = Arc::new(Mutex::new(BTreeMap::new()));
        let mut jsons: BTreeMap<String, String> = BTreeMap::new();
        let mut misc: BTreeMap<String, Vec<u8>> = BTreeMap::new();

        let mut handles: Vec<JoinHandle<()>> = Vec::new();
        let pb = ProgressBar::new(pkg.files.len() as u64);

        for (key, val) in pkg.files.into_iter() {
            let file_path = Path::new(&key);
            match file_path.extension().unwrap().to_str().unwrap() {
                "tex" => {
                    let key_clone = key.clone();
                    let texs = Arc::clone(&texs);

                    let handle = thread::spawn(move || {
                        let mut tex = Tex::new(&val).unwrap();

                        match tex.parse_to_rgba() {
                            Some(_) => {}
                            None => return,
                        };

                        texs.lock().unwrap().insert(key_clone, tex);
                    });

                    log::debug!("pkg: enqueued tex: {}", key);
                    handles.push(handle);
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
            handle.join().unwrap();
            pb.inc(1);
        }

        pb.finish_and_clear();

        let scene_string = jsons.get("scene.json").unwrap();
        let root: crate::scene::loader::scene::Root =
            serde_json::from_str(scene_string).expect("Unsupported scene.json");
        let mut texs_locked = texs.lock().unwrap();
        let texs = std::mem::take(&mut *texs_locked)
            .into_iter()
            .map(|(k, v)| (k, Rc::new(v)))
            .collect::<BTreeMap<String, Rc<Tex>>>();

        Self {
            root,
            textures: TextureBucket::new(texs, None),
            mdls: MdlBucket::new(BTreeMap::new(), None),
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
