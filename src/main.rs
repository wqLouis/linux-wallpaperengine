mod scene;

use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

use clap::Parser;
use depkg::pkg_parser::{parser::Pkg, tex_parser::Tex};
use indicatif::ProgressBar;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    // wallpaper .pkg file path
    #[arg(default_value = "./scene.pkg")]
    path: String,
}

fn main() {
    let args = Args::parse();

    let path = Path::new(&args.path);

    if path.exists() == false || path.extension().unwrap_or_default() != "pkg" {
        panic!("Path not exist or wrong file extension");
    }

    let pkg = Pkg::new(path);
    let texs: Arc<Mutex<HashMap<String, Tex>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut jsons: HashMap<String, String> = HashMap::new();

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
                    let mut tex = Tex::new(&payload).unwrap();
                    let mut texs = texs_ptr.lock().unwrap();
                    let parsed = tex.parse_to_rgba();
                    match parsed {
                        Some(_) => {}
                        None => return,
                    }

                    texs.insert(path, tex);
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

    pb.finish_and_clear();

    let scene: scene::Root = serde_json::from_str(jsons.get("scene.json").unwrap()).unwrap();

    let mut texs = texs.lock().unwrap();
    let texs = std::mem::take(&mut *texs);

    scene::renderer::render::start(scene, jsons, texs);
}
