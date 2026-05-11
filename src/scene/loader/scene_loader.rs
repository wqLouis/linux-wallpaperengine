use indicatif::ProgressBar;
use pkg_parser::pkg_parser::{mdl_parser::MdlFile, parser::Pkg, tex_parser::Tex};
use std::{
    collections::BTreeMap,
    path::Path,
    rc::Rc,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

pub struct Scene {
    pub root: crate::scene::loader::scene::Root,
    pub textures: BTreeMap<String, Rc<Tex>>,
    pub mdls: BTreeMap<String, Rc<MdlFile>>,
    pub jsons: BTreeMap<String, String>,
    pub misc: BTreeMap<String, Vec<u8>>,
}

impl Scene {
    pub fn new(path: String) -> Self {
        let path = Path::new(&path);
        let pkg = Pkg::new(path);

        let texs: Arc<Mutex<BTreeMap<String, Tex>>> = Arc::new(Mutex::new(BTreeMap::new()));
        let mdls: Arc<Mutex<BTreeMap<String, MdlFile>>> = Arc::new(Mutex::new(BTreeMap::new()));
        let mut jsons: BTreeMap<String, String> = BTreeMap::new();
        let mut misc: BTreeMap<String, Vec<u8>> = BTreeMap::new();

        let mut handles: Vec<JoinHandle<()>> = Vec::new();
        let pb = ProgressBar::new(pkg.files.len() as u64);

        for (key, val) in pkg.files.into_iter() {
            let file_path = Path::new(&key);
            match file_path.extension().unwrap().to_str().unwrap() {
                "tex" => {
                    let key = key.clone();
                    let texs = Arc::clone(&texs);

                    let handle = thread::spawn(move || {
                        let mut tex = Tex::new(&val).unwrap();

                        match tex.parse_to_rgba() {
                            Some(_) => {}
                            None => return,
                        };

                        texs.lock().unwrap().insert(key, tex);
                    });

                    handles.push(handle);
                }
                "mdl" => {
                    let key = key.clone();
                    let mdls = Arc::clone(&mdls);

                    let handle = thread::spawn(move || {
                        let Some(mdl) = MdlFile::new(&val) else {
                            log::warn!("failed to parse mdl: {}", key);
                            return;
                        };
                        mdls.lock().unwrap().insert(key, mdl);
                    });

                    handles.push(handle);
                }
                "json" => {
                    pb.inc(1);
                    jsons.insert(key, String::from_utf8_lossy(&val).to_string());
                }
                _ => {
                    pb.inc(1);
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

        let mut mdls_locked = mdls.lock().unwrap();
        let mdls = std::mem::take(&mut *mdls_locked)
            .into_iter()
            .map(|(k, v)| (k, Rc::new(v)))
            .collect::<BTreeMap<String, Rc<MdlFile>>>();

        Self {
            root,
            jsons,
            textures: texs,
            mdls,
            misc,
        }
    }
}
