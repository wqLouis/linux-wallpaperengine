use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use glam::{Vec2, Vec3};

use crate::scene::loader::scene::{Object, Vectors};

#[derive(Default)]
pub struct TextureObject {
    pub origin: Vec3,
    pub angles: Vec3,
    pub size: Vec2,
    pub scale: Vec3,
    pub parent: Option<i64>,
    pub model: String,
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
    pub fn new(objects: &Vec<Object>) -> Self {
        let mut render_sequence: Vec<i64> = vec![];

        let mut texture_map: BTreeMap<i64, Rc<RefCell<TextureObject>>> = BTreeMap::new();
        let mut audio_vec: Vec<AudioObject> = Vec::new();
        let mut node_map: BTreeMap<i64, Node> = BTreeMap::new();

        for object in objects {
            let Some(loaded_object) = load_object(object) else {
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

fn load_object(object: &Object) -> Option<ObjectType> {
    if object.image.is_some() {
        // Texture

        if object.visible.is_some() {
            let visible = object.visible.clone().unwrap().value().unwrap_or(true);
            if !visible {
                return None;
            }
        }

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

        let model = object.image.clone().unwrap_or_default();

        return Some(ObjectType::Texture(TextureObject {
            origin,
            angles,
            size,
            scale,
            parent: object.parent,
            model,
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

    let origin = object
        .origin
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
    let angles = object
        .angles
        .as_ref()
        .unwrap_or(&Vectors::default())
        .parse()
        .unwrap_or_default();

    Some(ObjectType::Node(Node {
        origin,
        angles,
        scale,
        parent: object.parent,
    }))
}
