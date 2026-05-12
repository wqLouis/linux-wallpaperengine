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
    pub visible: bool,
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

                if let Some(parent_rc) = tex_parent {
                    let parent = parent_rc.borrow();
                    // Propagate invisibility: if parent is not visible, child is also not visible
                    if !parent.visible {
                        texture.visible = false;
                    }
                    texture.angles += parent.angles;
                    texture.scale *= parent.scale;
                    texture.origin += parent.origin;
                    texture.origin = parent.origin + texture.origin * parent.scale;

                    match parent.parent {
                        None => break,
                        Some(id) => parent_id = id,
                    }
                }

                if let Some(parent) = node_parent {
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
            let obj = Rc::into_inner(tex_obj).unwrap().into_inner();
            if !obj.visible {
                continue;
            }
            texture_vec.push(obj);
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
            let visible = object
                .visible
                .clone()
                .and_then(|v| v.value())
                .unwrap_or(true);

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

            // Helper: build a 1×1 solid-colour fallback texture using the
            // object's `color` / `alpha` properties.
            let make_solid = || -> Rc<Tex> {
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
                log::debug!(
                    "solidlayer fallback for '{}': 1x1 rgba({},{},{},{})",
                    object.name, r, g, b, a
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
            };

            // Resolve the texture:
            //   model JSON → material JSON → texture reference (tex file)
            // Falls back to a solid-colour placeholder at every step.
            let texture: Rc<Tex> = (|| -> Option<Rc<Tex>> {
                let model_raw = scene.jsons.get(&model_path)?;
                let model = serde_json::from_str::<Model>(&model_raw[..]).ok()?;
                let material_raw = scene.jsons.get(&model.material)?;
                let material_json: Value =
                    serde_json::from_str(&material_raw[..]).ok()?;
                let tex_name = material_json["passes"]
                    .get(0)?
                    .get("textures")?
                    .get(0)?
                    .as_str()?;
                let tex_key = format!("materials/{}.tex", tex_name);
                match scene.textures.get(&tex_key) {
                    Some(t) => Some(t),
                    None => {
                        log::debug!(
                            "cannot get texture '{}' for material '{}'",
                            tex_key, model.material
                        );
                        None
                    }
                }
            })()
            .unwrap_or_else(make_solid);

            return Some(ObjectType::Texture(TextureObject {
                origin,
                angles,
                size,
                scale,
                parent: object.parent,
                texture: Rc::clone(&texture),
                effects: object.effects.clone(),
                visible,
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
