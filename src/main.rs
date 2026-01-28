use std::{collections::HashMap, path::Path};

use depkg::pkg_parser::{parser::Pkg, tex_parser::Tex};

fn main() {
    const PATH: &str = "./test/scene.pkg";
    let pkg = Pkg::new(Path::new(PATH));
    let mut texs: HashMap<(String, String), Vec<u8>> = HashMap::new();
    let mut jsons: HashMap<String, String> = HashMap::new();

    for (path, payload) in pkg.files.iter() {
        let path = Path::new(path);
        match path.extension().unwrap().to_str().unwrap() {
            "tex" => {
                let tex = Tex::new(payload).unwrap();
                texs.insert(
                    (path.to_str().unwrap().to_string(), tex.extension.clone()),
                    tex.parse_to_rgba().unwrap(),
                );
            }
            "json" => {
                jsons.insert(
                    path.to_str().unwrap().to_string(),
                    String::from_utf8_lossy(payload).to_string(),
                );
            }
            _ => {}
        }
    }
}
