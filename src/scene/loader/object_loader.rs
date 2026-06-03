use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use glam::{Vec2, Vec3};
use pkg_parser::pkg_parser::tex_parser::Tex;
use serde_json::Value;

use crate::scene::loader::{
    mdl::{self, PuppetMesh},
    model::Model,
    scene::{Effect, Object, Vectors},
    scene_loader::Scene,
};
use crate::scene::renderer::transform::{
    build_model_matrix, compose, Alignment, Transform,
};

#[derive(Debug, Clone)]
pub struct TextureObject {
    pub texture: Rc<Tex>,
    /// Local transform (position, rotation, scale, pivot, alignment).
    pub transform: Transform,
    /// World-space model matrix, computed by composing with the parent
    /// chain.  This is the matrix the renderer should use.
    pub model: glam::Mat4,
    pub size: Vec2,
    pub parent: Option<i64>,
    pub effects: Vec<Effect>,
    pub visible: bool,
    /// Optional puppet mesh extracted from a `.mdl` file.
    pub mesh: Option<PuppetMesh>,
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
    transform: Transform,
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
    pub fn with_clear_color(objects: &Vec<Object>, scene: &Scene, clear_color: Vec3, no_mdl: bool) -> Self {
        let mut render_sequence: Vec<i64> = vec![];

        let mut texture_map: BTreeMap<i64, Rc<RefCell<TextureObject>>> = BTreeMap::new();
        let mut audio_vec: Vec<AudioObject> = Vec::new();
        let mut node_map: BTreeMap<i64, Node> = BTreeMap::new();

        for object in objects {
            let Some(loaded_object) = Self::load_object(object, &scene, clear_color, no_mdl) else {
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

        // Propagate parent transforms in topological order (parents before
        // children) so each immediate parent already contains its ancestors'
        // transforms.  We sort by chain depth so we never need to traverse
        // more than one level up.
        let mut ids: Vec<i64> = texture_map.keys().copied().collect();
        ids.sort_by_key(|id| {
            // Compute chain depth (number of ancestors)
            let mut depth = 0u32;
            let mut cur = texture_map.get(id).and_then(|t| t.borrow().parent);
            while let Some(pid) = cur {
                depth += 1;
                cur = texture_map.get(&pid)
                    .and_then(|t| t.borrow().parent);
                if cur.is_none() {
                    cur = node_map.get(&pid).and_then(|n| n.parent);
                }
                if depth > 32 { break; }
            }
            depth
        });

        for id in ids {
            let Some(texture_rc) = texture_map.get(&id) else {
                continue;
            };

            let mut texture = texture_rc.borrow_mut();

            let Some(parent_id) = texture.parent else {
                continue;
            };

            // Look up the parent's world model matrix.  Since the parent
            // has already been processed (topological order), its `model`
            // already includes all ancestor transforms.
            let parent_model: Option<glam::Mat4> = if let Some(parent_rc) =
                texture_map.get(&parent_id)
            {
                let parent = parent_rc.borrow();
                if !parent.visible {
                    texture.visible = false;
                }
                Some(parent.model)
            } else if let Some(parent) = node_map.get(&parent_id) {
                Some(build_model_matrix(&parent.transform, Vec2::ZERO))
            } else {
                None
            };

            if let Some(pm) = parent_model {
                // M_child_world = M_parent_world * M_child_local
                texture.model = compose(pm, texture.model);
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
    /// Try to resolve a renderable texture for an object.
    ///
    /// Returns `None` when the object should be skipped (model not found,
    /// texture unresolvable, or a composelayer that cannot be rendered
    /// stand-alone).
    fn resolve_texture(
        scene: &Scene,
        model: &Model,
        object: &Object,
    ) -> Option<Rc<Tex>> {
        // ── composelayer (passthrough) ────────────────────────────
        // Uses a runtime framebuffer (`_rt_FullFrameBuffer`) that only
        // exists during effect compositing.  Skip when rendering
        // stand-alone.
        if model.passthrough == Some(true) {
            log::debug!(
                "object '{}' (id {}): composelayer (passthrough) — skipping",
                object.name, object.id,
            );
            return None;
        }

        // ── load material JSON ────────────────────────────────────
        let material_raw = scene.jsons.get(&model.material)?;
        let material_json: Value =
            serde_json::from_str(&material_raw[..]).ok()?;
        let passes = material_json["passes"].as_array()?;
        let first_pass = passes.first()?;

        // ── solidlayer: no textures, flat shader ──────────────────
        let has_textures = first_pass
            .get("textures")
            .and_then(|t| t.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);

        let is_solidlayer = model.solidlayer == Some(true) || !has_textures;

        if is_solidlayer {
            let color_vec = object
                .color
                .as_ref()
                .and_then(|c| c.parse())
                .unwrap_or(glam::Vec3::ONE);
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
                "solidlayer '{}': 1x1 rgba({},{},{},{})",
                object.name, r, g, b, a,
            );
            return Some(Rc::new(Tex {
                texv: String::new(),
                texi: String::new(),
                texb: String::new(),
                size: 4,
                actual_mip_count: 1,
                dimension: [1, 1],
                image_count: 1,
                mipmap_count: 1,
                lz4: false,
                decompressed_size: 4,
                extension: "solid".into(),
                payload: vec![r, g, b, a],
                mip_levels: Vec::new(),
            }));
        }

        // ── normal texture lookup ─────────────────────────────────
        let textures = first_pass.get("textures")?.as_array()?;
        let tex_name = textures.first()?.as_str()?;

        // Runtime framebuffer references are only valid during effect
        // compositing.
        if tex_name.starts_with("_rt_") {
            log::debug!(
                "object '{}' (id {}): runtime texture '{}' — skipping",
                object.name, object.id, tex_name,
            );
            return None;
        }

        let tex_key = format!("materials/{}.tex", tex_name);
        if let Some(tex) = scene.textures.get(&tex_key) {
            return Some(tex);
        }

        // ── unresolvable ──────────────────────────────────────────
        log::warn!(
            "object '{}' (id {}): tex '{}' not found (material '{}')",
            object.name, object.id, tex_key, model.material,
        );
        None
    }

    fn load_object(object: &Object, scene: &Scene, _clear_color: Vec3, no_mdl: bool) -> Option<ObjectType> {
        // Common transform properties shared by texture and node objects
        let position = object
            .origin
            .as_ref()
            .unwrap_or(&Vectors::default())
            .parse()
            .unwrap_or_default();
        let rotation = object
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
        let pivot = object
            .pivot
            .as_ref()
            .and_then(|v| v.parse())
            .unwrap_or_default();
        let alignment = parse_alignment(object.alignment.as_deref());

        let transform = Transform::new(position, rotation, scale)
            .with_pivot(pivot)
            .with_alignment(alignment);

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

            // -----------------------------------------------------------
            // Resolve the texture:
            //   model JSON → material JSON → texture reference (tex file)
            // -----------------------------------------------------------

            // Parse the model JSON once so we can use both `material` (for
            // the texture) and `puppet` (for the MDL mesh).
            let model_json: Option<Model> = scene
                .jsons
                .get(&model_path)
                .and_then(|raw| serde_json::from_str::<Model>(&raw[..]).ok());

            let Some(ref model) = model_json else {
                log::warn!(
                    "object '{}' (id {}): model JSON '{}' not found or invalid — skipping",
                    object.name, object.id, model_path,
                );
                return None;
            };

            let Some(texture) = Self::resolve_texture(scene, model, object) else {
                log::warn!(
                    "object '{}' (id {}): texture not found for material '{}' — skipping",
                    object.name, object.id, model.material,
                );
                return None;
            };

            // Load puppet mesh if the model references a .mdl file
            // (unless --no-mdl was passed).
            let obj_dims = [size.x, size.y];
            let mesh = if no_mdl {
                None
            } else {
                model
                    .puppet
                    .as_ref()
                    .and_then(|puppet_path| {
                        log::debug!("loading puppet mesh '{}' for '{}'", puppet_path, object.name);
                        scene.mdls.get(puppet_path)
                    })
                    .and_then(|mdl_rc| mdl::extract_mesh(&mdl_rc, obj_dims))
            };

            if mesh.is_some() {
                log::info!("loaded puppet mesh for '{}': {} verts, {} indices",
                    object.name,
                    mesh.as_ref().unwrap().vertices.len(),
                    mesh.as_ref().unwrap().indices.len(),
                );
            }

            return Some(ObjectType::Texture(TextureObject {
                transform,
                model: build_model_matrix(&transform, size),
                size,
                parent: object.parent,
                texture: Rc::clone(&texture),
                effects: object.effects.clone(),
                visible,
                mesh,
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
            transform,
            parent: object.parent,
        }))
    }
}

/// Parse a scene.json `alignment` string into a typed [`Alignment`].
///
/// Unknown / missing values fall back to [`Alignment::Center`].
fn parse_alignment(s: Option<&str>) -> Alignment {
    match s.unwrap_or("").to_ascii_lowercase().as_str() {
        "left" => Alignment::Left,
        "right" => Alignment::Right,
        "top" => Alignment::Top,
        "bottom" => Alignment::Bottom,
        "topleft" | "top-left" | "top_left" => Alignment::TopLeft,
        "topright" | "top-right" | "top_right" => Alignment::TopRight,
        "bottomleft" | "bottom-left" | "bottom_left" => Alignment::BottomLeft,
        "bottomright" | "bottom-right" | "bottom_right" => Alignment::BottomRight,
        _ => Alignment::Center,
    }
}
