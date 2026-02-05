mod scene;

use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

use depkg::pkg_parser::{parser::Pkg, tex_parser::Tex};
use indicatif::ProgressBar;

fn main() {
    const PATH: &str = "./test/scene.pkg";

    let pkg = Pkg::new(Path::new(PATH));
    let texs: Arc<Mutex<BTreeMap<(String, String), Vec<u8>>>> =
        Arc::new(Mutex::new(BTreeMap::new()));
    let mut jsons: BTreeMap<String, String> = BTreeMap::new();

    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    let pb = ProgressBar::new(pkg.files.len() as u64);

    for (path_str, payload) in pkg.files.iter() {
        let path = Path::new(path_str);
        match path.extension().unwrap().to_str().unwrap() {
            "tex" => {
                let payload = payload.to_vec();
                let path = path_str.clone();

                let texs_ptr = Arc::clone(&texs);

                let tex_handle = thread::spawn(move || {
                    let tex = Tex::new(&payload).unwrap();
                    let mut texs = texs_ptr.lock().unwrap();

                    texs.insert((path, tex.extension.clone()), tex.parse_to_rgba().unwrap());
                });

                handles.push(tex_handle);
            }
            "json" => {
                pb.inc(1);
                jsons.insert(
                    path.to_str().unwrap().to_string(),
                    String::from_utf8_lossy(payload).to_string(),
                );
            }
            _ => {
                pb.inc(1);
            }
        }
    }

    for handle in handles {
        handle.join().unwrap();
        pb.inc(1);
    }

    let scene: scene::Root = serde_json::from_str(jsons.get("scene.json").unwrap()).unwrap();

    scene::render::create_window();
}
