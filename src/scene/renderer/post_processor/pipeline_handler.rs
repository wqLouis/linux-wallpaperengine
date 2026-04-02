use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::OnceLock,
};
use wgpu::*;

use crate::scene::loader::scene_loader::Scene;

struct Dependencies {
    vert: String,
    frag: String,
    mat: String,
}

/// Get pipeline if pipeline is created, if not then create the pipeline
pub fn get_or_create_pipeline(
    effect_path: String,
    pipelines: &mut BTreeMap<String, Rc<RenderPipeline>>,
    scene: &Scene,
) -> Option<Rc<RenderPipeline>> {
    let mut pipeline = pipelines.get(&effect_path);

    if pipeline.is_some() {
        let pipeline = Rc::clone(pipeline.unwrap());
        return Some(pipeline);
    }

    let shader_dep = get_shader_from_effect(scene, effect_path)?;

    println!("{:?}", shader_dep.vert);
    println!("{:?}", shader_dep.frag);

    None
}

/// This fn get the shaders in string.
/// Only accept 1 vert 1 frag and 1 material for dependencies
fn get_shader_from_effect(scene: &Scene, effect_path: String) -> Option<Dependencies> {
    let effect_json = scene.jsons.get(&effect_path)?;

    let effects: BTreeMap<String, Value> = serde_json::from_str(effect_json).unwrap();
    let dependencies = effects.get("dependencies")?;

    let dependencies = dependencies.as_array()?;

    let dependencies = dependencies
        .into_iter()
        .filter_map(|dependency| dependency.as_str())
        .map(|dependency| Path::new(dependency).to_path_buf())
        .collect::<Vec<PathBuf>>();

    let mat: OnceLock<String> = OnceLock::new(); // wont handle at this stage
    let vert: OnceLock<String> = OnceLock::new();
    let frag: OnceLock<String> = OnceLock::new();

    for path in dependencies {
        println!("{:?}", path);

        let ancestors: Vec<&Path> = path
            .ancestors()
            .collect::<Vec<&Path>>()
            .into_iter()
            .rev()
            .collect();

        let Some(top_level) = ancestors.get(1) else {
            continue;
        };

        let Some(top_level) = top_level.to_str() else {
            continue;
        };

        match top_level {
            "materials" => {
                let Some(_) = mat
                    .set(
                        scene
                            .jsons
                            .get(path.to_str().unwrap())
                            .unwrap_or(&"".to_string())
                            .to_owned(),
                    )
                    .ok()
                else {
                    break;
                };
            }
            "shaders" => {
                // match shader types here
                match path
                    .extension()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or_default()
                {
                    "vert" => {
                        let Some(vert_bytes) = scene.misc.get(path.to_str().unwrap()) else {
                            break;
                        };

                        let Some(_) = vert
                            .set(String::from_utf8_lossy(vert_bytes).to_string())
                            .ok()
                        else {
                            break;
                        };
                    }
                    "frag" => {
                        let Some(frag_bytes) = scene.misc.get(path.to_str().unwrap()) else {
                            break;
                        };

                        let Some(_) = frag
                            .set(String::from_utf8_lossy(frag_bytes).to_string())
                            .ok()
                        else {
                            break;
                        };
                    }
                    _ => {}
                };
            }
            _ => {}
        }
    }

    Some(Dependencies {
        vert: vert.into_inner()?,
        frag: frag.into_inner()?,
        mat: mat.into_inner()?,
    })
}
