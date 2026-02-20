use depkg::pkg_parser::{parser::Pkg, tex_parser::Tex};
use indicatif::ProgressBar;
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
    pub jsons: BTreeMap<String, String>,
    pub desc: BTreeMap<String, Vec<u8>>,
}

impl Scene {
    pub fn new(path: String) -> Self {
        let path = Path::new(&path);
        let pkg = Pkg::new(path);

        let texs: Arc<Mutex<BTreeMap<String, Tex>>> = Arc::new(Mutex::new(BTreeMap::new()));
        let mut jsons: BTreeMap<String, String> = BTreeMap::new();
        let mut desc: BTreeMap<String, Vec<u8>> = BTreeMap::new();

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
                "json" => {
                    pb.inc(1);
                    jsons.insert(key, String::from_utf8_lossy(&val).to_string());
                }
                _ => {
                    pb.inc(1);
                    desc.insert(key, val);
                }
            }
        }

        for handle in handles {
            handle.join().unwrap();
            pb.inc(1);
        }

        pb.finish_and_clear();

        let scene_string = jsons.get("scene.json").unwrap();
        let root: crate::scene::loader::scene::Root = serde_json::from_str(scene_string)
            .expect(&format!("Unsupported scene.json\n{:?}", scene_string));
        let mut texs_locked = texs.lock().unwrap();
        let texs = std::mem::take(&mut *texs_locked)
            .into_iter()
            .map(|(k, v)| (k, Rc::new(v)))
            .collect::<BTreeMap<String, Rc<Tex>>>();

        Self {
            root,
            jsons,
            textures: texs,
            desc,
        }
    }
}
