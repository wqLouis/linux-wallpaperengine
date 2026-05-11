use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use glam::{Vec2, Vec3};
use pkg_parser::pkg_parser::tex_parser::Tex;
use serde_json::Value;

use crate::scene::loader::{
    model::Model,
    scene::{Effect, Object, Vectors},
    scene_loader::Scene,
};

#[derive(Debug, Clone)]
pub struct TextureObject {
    pub texture: Rc<Tex>,
    pub origin: Vec3,
    pub angles: Vec3,
    pub size: Vec2,
    pub scale: Vec3,
    pub parent: Option<i64>,
    pub effects: Vec<Effect>,
}

pub struct AudioObject {
    pub sounds: Vec<String>,
    pub playback_mode: PlaybackMode,
}

pub struct ObjectMap {
    pub texture: Vec<TextureObject>,
    pub audio: Vec<AudioObject>,
}

struct Node {
    origin: Vec3,
    angles: Vec3,
    scale: Vec3,
    parent: Option<i64>,
}

pub enum PlaybackMode {
    Loop,
    Others,
}

enum ObjectType {
    Texture(TextureObject),
    Audio(AudioObject),
    Node(Node),
}

impl ObjectMap {
    pub fn with_clear_color(objects: &Vec<Object>, scene: &Scene, clear_color: Vec3) -> Self {
        let mut render_sequence: Vec<i64> = vec![];

        let mut texture_map: BTreeMap<i64, Rc<RefCell<TextureObject>>> = BTreeMap::new();
        let mut audio_vec: Vec<AudioObject> = Vec::new();
        let mut node_map: BTreeMap<i64, Node> = BTreeMap::new();

        for object in objects {
            let Some(loaded_object) = Self::load_object(object, &scene, clear_color) else {
                continue;
            };
            match loaded_object {
                ObjectType::Audio(audio_object) => {
                    audio_vec.push(audio_object);
                }
                ObjectType::Texture(texture_object) => {
                    render_sequence.push(object.id);
                    texture_map.insert(object.id, Rc::new(RefCell::new(texture_object)));
                }
                ObjectType::Node(node) => {
                    node_map.insert(object.id, node);
                }
            }
        }

        for id in texture_map.keys().copied().collect::<Vec<i64>>() {
            let Some(texture_rc) = texture_map.get(&id) else {
                continue;
            };

            let mut texture = texture_rc.borrow_mut();

            let Some(mut parent_id) = texture.parent else {
                continue;
            };

            loop {
                let tex_parent = texture_map.get(&parent_id);
                let node_parent = node_map.get(&parent_id);

                if tex_parent.is_none() && node_parent.is_none() {
                    break;
                }

                if tex_parent.is_some() {
                    let parent = tex_parent.unwrap().borrow();
                    texture.angles += parent.angles;
                    texture.scale *= parent.scale;
                    texture.origin += parent.origin;
                    texture.origin = parent.origin + texture.origin * parent.scale;

                    match parent.parent {
                        None => break,
                        Some(id) => parent_id = id,
                    }
                }

                if node_parent.is_some() {
                    let parent = node_parent.unwrap();
                    texture.angles += parent.angles;
                    texture.scale *= parent.scale;
                    texture.origin = parent.origin + texture.origin * parent.scale;

                    match parent.parent {
                        None => break,
                        Some(id) => parent_id = id,
                    }
                }
            }
        }

        let mut texture_vec: Vec<TextureObject> = vec![];

        for id in render_sequence {
            let Some(tex_obj) = texture_map.remove(&id) else {
                continue;
            };
            texture_vec.push(Rc::into_inner(tex_obj).unwrap().into_inner());
        }

        Self {
            texture: texture_vec,
            audio: audio_vec,
        }
    }
}

impl ObjectMap {
    fn load_object(object: &Object, scene: &Scene, clear_color: Vec3) -> Option<ObjectType> {
        // Common transform properties shared by texture and node objects
        let origin = object
            .origin
            .as_ref()
            .unwrap_or(&Vectors::default())
            .parse()
            .unwrap_or_default();
        let angles = object
            .angles
            .as_ref()
            .unwrap_or(&Vectors::default())
            .parse()
            .unwrap_or_default();
        let scale = object
            .scale
            .as_ref()
            .unwrap_or(&Vectors::Scaler(1.0))
            .parse()
            .unwrap_or_default();

        if object.image.is_some() {
            // Texture
            if object.visible.is_some() {
                let visible = object.visible.clone().unwrap().value().unwrap_or(true);
                if !visible {
                    return None;
                }
            }

            let size = object
                .size
                .as_ref()
                .unwrap_or(&Vectors::default())
                .parse()
                .unwrap_or_default();
            let size = Vec2 {
                x: size.x,
                y: size.y,
            };

            let model_path = object.image.clone().unwrap_or_default();
            let model = serde_json::from_str::<Model>(&scene.jsons.get(&model_path)?[..]).ok()?;

            // Load the material JSON to find the actual texture reference.
            // If the material has a `textures` array we load that tex;
            // otherwise (shader-only like "flat") we create a 1×1 white
            // solid placeholder so the object can still be rendered.
            let material_json: Value =
                serde_json::from_str(&scene.jsons.get(&model.material)?[..]).ok()?;
            let texture: Rc<Tex> = match material_json["passes"]
                .get(0)
                .and_then(|p| p.get("textures"))
                .and_then(|t| t.get(0))
                .and_then(|t| t.as_str())
            {
                Some(tex_name) => {
                    let tex_key = format!("materials/{}.tex", tex_name);
                    match scene.textures.get(&tex_key) {
                        Some(t) => t,
                        None => {
                            log::debug!(
                                "cannot get texture '{}' for material '{}' (tex_name: {})",
                                tex_key,
                                model.material,
                                tex_name,
                            );
                            return None;
                        }
                    }
                }
                None => {
                    // Shader-only material — create a 1×1 solid texture
                    // using the object's `color` and `alpha` properties.
                    // Falls back to the scene's clear color, then white.
                    let color_vec = object
                        .color
                        .as_ref()
                        .and_then(|c| c.parse())
                        .unwrap_or(clear_color)
                        .max(Vec3::ZERO);

                    let alpha_val = object
                        .alpha
                        .as_ref()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0);
                    let r = (color_vec.x.clamp(0.0, 1.0) * 255.0) as u8;
                    let g = (color_vec.y.clamp(0.0, 1.0) * 255.0) as u8;
                    let b = (color_vec.z.clamp(0.0, 1.0) * 255.0) as u8;
                    let a = (alpha_val.clamp(0.0, 1.0) * 255.0) as u8;
                    log::trace!(
                        "material '{}' has no textures, using 1x1 solid ({},{},{},{}) for object '{}'",
                        model.material,
                        r, g, b, a,
                        object.name,
                    );
                    Rc::new(Tex {
                        texv: String::new(),
                        texi: String::new(),
                        texb: String::new(),
                        size: 4,
                        dimension: [1, 1],
                        image_count: 1,
                        mipmap_count: 1,
                        lz4: false,
                        decompressed_size: 4,
                        extension: "solid".into(),
                        payload: vec![r, g, b, a],
                    })
                }
            };

            return Some(ObjectType::Texture(TextureObject {
                origin,
                angles,
                size,
                scale,
                parent: object.parent,
                texture: Rc::clone(&texture),
                effects: object.effects.clone(),
            }));
        }

        if object.sound.len() > 0 {
            // Audio
            let playback_mode = match object.playbackmode.clone().unwrap_or_default().as_str() {
                "loop" => PlaybackMode::Loop,
                _ => PlaybackMode::Others,
            };

            return Some(ObjectType::Audio(AudioObject {
                sounds: object.sound.to_owned(),
                playback_mode: playback_mode,
            }));
        }

        Some(ObjectType::Node(Node {
            origin,
            angles,
            scale,
            parent: object.parent,
        }))
    }
}
