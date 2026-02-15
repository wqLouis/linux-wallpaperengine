use std::{collections::HashMap, path::Path, sync::Arc};

use depkg::pkg_parser::tex_parser::Tex;
use serde_json::{Value, json};

use crate::scene::{Object, Vectors};

pub struct ObjectParameters {
    pub origin: Vec<f32>,
    pub angles: Vec<f32>,
    pub scale: Vec<f32>,
    pub size: Vec<f32>,
    pub alpha: f32,
    pub tex: Arc<Tex>,
}

pub fn load_from_json(
    object: &Object,
    jsons: &HashMap<String, String>,
    texs: &HashMap<String, Arc<Tex>>,
) -> Option<ObjectParameters> {
    if object.visible.is_some() {
        let visible = object.visible.clone().unwrap();
        if visible.is_boolean() && visible.as_bool().unwrap() == false {
            return None;
        }
        if visible.is_object() {
            let visible = visible.as_object().unwrap();
            if visible
                .get("value")
                .unwrap_or(&Value::Bool(true))
                .is_boolean()
                | visible
                    .get("value")
                    .unwrap_or(&Value::Bool(true))
                    .as_bool()
                    .unwrap_or(true)
                == false
            {
                return None;
            }
        }
    }

    let origin = object
        .origin
        .clone()
        .unwrap_or(Vectors::Vectors("0.0 0.0 0.0".to_string()))
        .parse()
        .unwrap()
        .iter()
        .map(|val| val.to_owned() as f32)
        .collect::<Vec<f32>>();
    let angles = object
        .angles
        .clone()
        .unwrap_or(Vectors::Vectors("0.0 0.0 0.0".to_string()))
        .parse()
        .unwrap()
        .iter()
        .map(|val| val.to_owned() as f32)
        .collect::<Vec<f32>>();
    let scale = object
        .scale
        .clone()
        .unwrap_or(Vectors::Vectors("1.0 1.0 1.0".to_string()))
        .parse()
        .unwrap()
        .iter()
        .map(|val| val.to_owned() as f32)
        .collect::<Vec<f32>>();
    let size = object
        .size
        .clone()
        .unwrap_or(Vectors::Vectors("0.0 0.0 0.0".to_string()))
        .parse()
        .unwrap()
        .iter()
        .map(|val| val.to_owned() as f32)
        .collect::<Vec<f32>>();

    let mut alpha_val = 1.0;
    let alpha_default = json!(1.0);
    let alpha = object.alpha.as_ref().unwrap_or(&alpha_default);

    if alpha.is_f64() {
        alpha_val = alpha.as_f64().unwrap_or(1.0);
    }
    if alpha.is_object() {
        let default_alpha = json!(1.0);
        let alpha = alpha
            .as_object()
            .unwrap()
            .get("value")
            .unwrap_or(&default_alpha);
        if alpha.is_f64() {
            alpha_val = alpha.as_f64().unwrap_or(1.0);
        }
    }

    let Some(model_path) = object.image.clone() else {
        return None;
    };
    let Some(model_string) = jsons.get(&model_path) else {
        return None;
    };
    let model: crate::scene::models::Root = serde_json::from_str(model_string).unwrap();
    let mut tex_path = Path::new(&model.material).to_path_buf();
    tex_path.set_extension("tex");
    let tex = texs.get(&tex_path.to_str().unwrap().to_string())?;

    if tex.payload.len() != (tex.dimension[0] * tex.dimension[1] * 4) as usize {
        println!("Broken texture: {:?}", tex_path);
        println!(
            "format: {:?}    dimensions: {:?}",
            tex.extension, tex.dimension
        );
        println!(
            "size: {:?}    actual_size: {:?}",
            (tex.dimension[0] * tex.dimension[1] * 4),
            tex.payload.len()
        );
        println!();
        return None;
    }

    Some(ObjectParameters {
        origin,
        angles,
        scale,
        size,
        alpha: alpha_val as f32,
        tex: Arc::clone(tex),
    })
}
