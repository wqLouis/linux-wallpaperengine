use std::{cell::RefCell, rc::Rc};

use glam::{Vec2, Vec3};
use pkg_parser::pkg_parser::tex_parser::Tex;
use serde_json::Value;

use crate::collections::{Map, new_map};
use crate::scene::loader::{
    assets_loader::VideoAsset,
    mdl::{self, PuppetAnimation, PuppetMesh},
    model::Model,
    scene::{Effect, ElementType, Object, Vectors},
    scene_loader::Scene,
};
use crate::scene::renderer::transform::{Alignment, Transform, build_model_matrix, compose};

/// The extensions of the standalone video files a material may refer to.
const VIDEO_EXTENSIONS: [&str; 4] = ["mp4", "webm", "gif", "avi"];

/// Where an object's pixels come from.
#[derive(Debug, Clone)]
pub enum TextureSource {
    /// A static texture parsed from a `.tex` file.
    Static(Rc<Tex>),
    /// A video or animated image, decoded frame by frame.
    Video(Rc<VideoAsset>),
}

impl TextureSource {
    /// The size of the source in pixels.
    ///
    /// A static texture is stored padded (to a power of two), a video is
    /// decoded at its own resolution.
    pub fn dimension(&self) -> [u32; 2] {
        match self {
            Self::Static(tex) => tex.dimension,
            Self::Video(video) => video.size(),
        }
    }

    /// The UV scale of the source: the ratio between the real (non-padded)
    /// region and the stored size.
    ///
    /// A video has no padded region, so its scale is always `(1, 1)`.
    pub fn uv_scale(&self) -> Vec2 {
        match self {
            Self::Static(tex) => Vec2::new(
                tex.real_dimension[0] as f32 / tex.dimension[0].max(1) as f32,
                tex.real_dimension[1] as f32 / tex.dimension[1].max(1) as f32,
            ),
            Self::Video(_) => Vec2::ONE,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextureObject {
    /// The scene.json object id — used to resolve parent/attachment links.
    pub id: i64,
    /// The pixels of the object: a static texture or a video.
    pub source: TextureSource,
    /// Local transform (position, rotation, scale, alignment).
    ///
    /// Kept for reference — the world matrix is pre-computed in
    /// [`TextureObject::model`].
    #[allow(dead_code)]
    pub transform: Transform,
    /// World-space model matrix, computed by composing with the parent
    /// chain.  This is the matrix the renderer should use.
    pub model: glam::Mat4,
    /// The object's own model matrix (before the parent chain is applied).
    ///
    /// Needed for objects attached to a bone socket: the socket matrix is
    /// composed with this local matrix every frame, so the animated socket
    /// can replace the bind-pose one.
    pub local_model: glam::Mat4,
    pub size: Vec2,
    /// UV scale applied to plain quads: `real_dimension / storage_dimension`.
    /// Wallpaper Engine only samples the real (non-padded) top-left region of
    /// the stored texture, so quads must clamp their UVs accordingly.  Always
    /// `(1, 1)` for puppet meshes, which carry their own UVs.
    pub uv_scale: Vec2,
    pub parent: Option<i64>,
    pub effects: Vec<Effect>,
    pub visible: bool,
    /// Optional puppet mesh extracted from a `.mdl` file.
    pub mesh: Option<PuppetMesh>,
    /// Optional animation layers selected from the mesh's MDL.
    pub animations: Vec<PuppetAnimation>,
    /// Wallpaper Engine `attachment` socket name this object is parented to.
    pub attachment: Option<String>,
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
    /// Boxed: a [`TextureObject`] is far bigger than the other variants
    /// (it carries the optional puppet mesh), so keeping it inline would
    /// make every enum value that large.
    Texture(Box<TextureObject>),
    Audio(AudioObject),
    Node(Node),
}

impl ObjectMap {
    pub fn with_clear_color(
        objects: &Vec<Object>,
        scene: &Scene,
        clear_color: Vec3,
        no_mdl: bool,
    ) -> Self {
        let mut render_sequence: Vec<i64> = vec![];

        let mut texture_map: Map<i64, Rc<RefCell<TextureObject>>> = new_map();
        let mut audio_vec: Vec<AudioObject> = Vec::new();
        let mut node_map: Map<i64, Node> = new_map();

        for object in objects {
            let Some(loaded_object) = Self::load_object(object, scene, clear_color, no_mdl) else {
                continue;
            };
            match loaded_object {
                ObjectType::Audio(audio_object) => {
                    audio_vec.push(audio_object);
                }
                ObjectType::Texture(texture_object) => {
                    render_sequence.push(object.id);
                    texture_map.insert(object.id, Rc::new(RefCell::new(*texture_object)));
                }
                ObjectType::Node(node) => {
                    node_map.insert(object.id, node);
                }
            }
        }

        // Propagate parent transforms in topological order (parents before
        // children) so each immediate parent already contains its ancestors'
        // transforms.  We sort by chain depth so we never need to traverse
        // more than one level up.  Wallpaper Engine archives form a valid
        // forest, so no cycle handling is needed.
        let mut ids: Vec<i64> = texture_map.keys().copied().collect();
        ids.sort_by_key(|id| {
            let parent_of = |id: i64| {
                texture_map
                    .get(&id)
                    .and_then(|t| t.borrow().parent)
                    .or_else(|| node_map.get(&id).and_then(|n| n.parent))
            };

            let mut depth = 0u32;
            let mut cur = parent_of(*id);
            while let Some(pid) = cur {
                depth += 1;
                cur = parent_of(pid);
            }
            // Depth first, then the object id: the tie-break keeps the
            // propagation order stable now that the map has no inherent
            // order of its own.
            (depth, *id)
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
            let mut socket: Option<glam::Mat4> = None;
            let parent_model: Option<glam::Mat4> =
                if let Some(parent_rc) = texture_map.get(&parent_id) {
                    let parent = parent_rc.borrow();
                    if !parent.visible {
                        texture.visible = false;
                    }
                    // Objects parented to a puppet attachment socket are placed
                    // at that socket (from the parent's MDL) before their own
                    // local transform is applied.
                    if let (Some(name), Some(mesh)) =
                        (texture.attachment.as_deref(), parent.mesh.as_ref())
                    {
                        socket = mesh.attachment_world(name);
                    }
                    Some(parent.model)
                } else {
                    node_map
                        .get(&parent_id)
                        .map(|parent| build_model_matrix(&parent.transform, Vec2::ZERO))
                };

            if let Some(pm) = parent_model {
                // An object attached to a bone socket is placed *relative* to
                // that socket: the socket matrix is the base and the
                // object's own origin/angles/scale are the offset from it
                // (`M_world(child) = M_world(parent) * M_socket * M_local`).
                //
                // The scene data confirms it: a puppet's face is attached to
                // the controller's "Attachment" socket (which sits at the
                // controller's origin, i.e. at the character's feet) with an
                // origin of ~750 px — the origin has to be applied on top of
                // the socket, otherwise the face would end up at the feet.
                let local = match socket {
                    Some(socket) => socket * texture.local_model,
                    None => texture.local_model,
                };
                // M_child_world = M_parent_world * M_child_local
                texture.model = compose(pm, local);
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
    ///
    /// The pixels come either from a static `.tex` file or from a video:
    ///
    /// 1. `materials/{name}.tex` whose payload is a video container —
    ///    Wallpaper Engine stores its video textures that way,
    /// 2. `materials/{name}.mp4` — a standalone video next to the material,
    /// 3. `materials/{name}.tex` — an ordinary texture.
    fn resolve_texture(scene: &Scene, model: &Model, object: &Object) -> Option<TextureSource> {
        // ── composelayer (passthrough) ────────────────────────────
        // Uses a runtime framebuffer (`_rt_FullFrameBuffer`) that only
        // exists during effect compositing.  Skip when rendering
        // stand-alone.
        if model.passthrough == Some(true) {
            log::debug!(
                "object '{}' (id {}): composelayer (passthrough) — skipping",
                object.name,
                object.id,
            );
            return None;
        }

        // ── load material JSON ────────────────────────────────────
        let material_raw = scene.assets.json(&model.material)?;
        let material_json: Value = serde_json::from_str(&material_raw[..]).ok()?;
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
                object.name,
                r,
                g,
                b,
                a,
            );
            return Some(TextureSource::Static(Rc::new(Tex {
                texv: String::new(),
                texi: String::new(),
                texb: String::new(),
                size: 4,
                actual_mip_count: 1,
                dimension: [1, 1],
                real_dimension: [1, 1],
                image_count: 1,
                mipmap_count: 1,
                lz4: false,
                decompressed_size: 4,
                extension: "solid".into(),
                payload: vec![r, g, b, a],
                mip_levels: Vec::new(),
            })));
        }

        // ── normal texture lookup ─────────────────────────────────
        let textures = first_pass.get("textures")?.as_array()?;
        let tex_name = textures.first()?.as_str()?;

        // Runtime framebuffer references are only valid during effect
        // compositing.
        if tex_name.starts_with("_rt_") {
            log::debug!(
                "object '{}' (id {}): runtime texture '{}' — skipping",
                object.name,
                object.id,
                tex_name,
            );
            return None;
        }

        let tex_key = format!("materials/{}.tex", tex_name);

        // ── a video texture ───────────────────────────────────────
        // Wallpaper Engine stores its videos as a `.tex` file whose
        // payload is the video container.
        if let Some(video) = scene.assets.video(&tex_key) {
            log::debug!(
                "object '{}' (id {}): video texture '{}' ({}x{})",
                object.name,
                object.id,
                tex_key,
                video.info.width,
                video.info.height,
            );
            return Some(TextureSource::Video(video));
        }

        // ── a standalone video next to the material ───────────────
        for extension in VIDEO_EXTENSIONS {
            let video_key = format!("materials/{}.{}", tex_name, extension);
            if let Some(video) = scene.assets.video(&video_key) {
                log::debug!(
                    "object '{}' (id {}): video texture '{}' ({}x{})",
                    object.name,
                    object.id,
                    video_key,
                    video.info.width,
                    video.info.height,
                );
                return Some(TextureSource::Video(video));
            }
        }

        // ── an ordinary texture ───────────────────────────────────
        if let Some(tex) = scene.assets.texture(&tex_key) {
            return Some(TextureSource::Static(tex));
        }

        // ── unresolvable ──────────────────────────────────────────
        log::warn!(
            "object '{}' (id {}): tex '{}' not found (material '{}')",
            object.name,
            object.id,
            tex_key,
            model.material,
        );
        None
    }

    fn load_object(
        object: &Object,
        scene: &Scene,
        _clear_color: Vec3,
        no_mdl: bool,
    ) -> Option<ObjectType> {
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
        let alignment = parse_alignment(object.alignment.as_deref());

        let transform = Transform::new(position, rotation, scale).with_alignment(alignment);

        match object.element_type() {
            ElementType::Image => {
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
                    .assets
                    .json(&model_path)
                    .and_then(|raw| serde_json::from_str::<Model>(&raw[..]).ok());

                let Some(ref model) = model_json else {
                    log::warn!(
                        "object '{}' (id {}): model JSON '{}' not found or invalid — skipping",
                        object.name,
                        object.id,
                        model_path,
                    );
                    return None;
                };

                let Some(source) = Self::resolve_texture(scene, model, object) else {
                    log::warn!(
                        "object '{}' (id {}): texture not found for material '{}' — skipping",
                        object.name,
                        object.id,
                        model.material,
                    );
                    return None;
                };

                // Load puppet mesh if the model references a .mdl file
                // (unless --no-mdl was passed).
                //
                // The mesh's UVs are relative to the texture's *real*
                // (non-padded) region, so they carry the same UV scale the
                // plain quads use.  A video has no padded region, so its
                // scale is always (1, 1).
                let uv_scale = source.uv_scale();
                let mesh = if no_mdl {
                    None
                } else {
                    model
                        .puppet
                        .as_ref()
                        .and_then(|puppet_path| {
                            log::debug!(
                                "loading puppet mesh '{}' for '{}'",
                                puppet_path,
                                object.name
                            );
                            scene.assets.model(puppet_path)
                        })
                        .and_then(|mdl_rc| mdl::extract_mesh(mdl_rc, uv_scale))
                };

                if let Some(mesh) = &mesh {
                    log::info!(
                        "loaded puppet mesh for '{}': {} verts, {} indices",
                        object.name,
                        mesh.vertices.len(),
                        mesh.indices.len(),
                    );
                }

                // Pick the animation layers this object should play.
                let animations = mesh
                    .as_ref()
                    .map(|mesh| select_animations(&mesh.mdl, object))
                    .unwrap_or_default();

                if !animations.is_empty() {
                    log::info!(
                        "animations for '{}': {} layer(s)",
                        object.name,
                        animations.len(),
                    );
                }

                let local_model = build_model_matrix(&transform, size);

                return Some(ObjectType::Texture(Box::new(TextureObject {
                    id: object.id,
                    transform,
                    model: local_model,
                    local_model,
                    size,
                    uv_scale,
                    parent: object.parent,
                    source,
                    effects: object.effects.clone(),
                    visible,
                    mesh,
                    animations,
                    attachment: object.attachment.clone(),
                })));
            }

            ElementType::Sound => {
                // Audio
                let playback_mode = match object.playbackmode.clone().unwrap_or_default().as_str() {
                    "loop" => PlaybackMode::Loop,
                    _ => PlaybackMode::Others,
                };

                return Some(ObjectType::Audio(AudioObject {
                    sounds: object.sound.to_owned(),
                    playback_mode,
                }));
            }

            // Particle systems, text, lights and cameras are not rendered by
            // this backend yet, but they still act as transform-only nodes so
            // their children keep their world transforms.
            _ => {}
        }

        Some(ObjectType::Node(Node {
            transform,
            parent: object.parent,
        }))
    }
}

/// Pick the animation layers an object should play.
///
/// Wallpaper Engine stores one layer per animation; layer order matches the
/// MDL clip order (`asuna body` → `eyes`, `breathing` ↔ `eyes`,
/// `Animation 2`).  Layer names usually match the clip name, but not always
/// (`breathing` does not), so an exact name match wins and the layer's
/// position is the fallback.  Invisible layers are skipped.
fn select_animations(
    mdl: &pkg_parser::pkg_parser::mdl_parser::MdlFile,
    object: &Object,
) -> Vec<PuppetAnimation> {
    let clips = &mdl.animation.clips;
    if clips.is_empty() {
        return Vec::new();
    }

    let mut animations = Vec::new();
    for (index, layer) in object.animationlayers.iter().enumerate() {
        if let Some(false) = layer.visible.as_bool() {
            continue;
        }
        let clip_index = clips
            .iter()
            .position(|clip| clip.name == layer.name)
            .unwrap_or(index);
        if clips.get(clip_index).is_none() {
            continue;
        }

        animations.push(PuppetAnimation {
            clip_index,
            rate: layer.rate.as_f64().unwrap_or(1.0) as f32,
            blend: layer.blend.as_f64().unwrap_or(1.0) as f32,
            additive: layer.additive,
        });
    }

    animations
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::new_map;
    use crate::scene::loader::assets_loader::{Asset, AssetStore};
    use crate::scene::loader::scene::Root;

    /// A tiny h264 clip (32×32) from the parser's test data.
    const SAMPLE_MP4: &[u8] = include_bytes!("../../pkg_parser/tests/data/sample.mp4");

    /// Build a minimal `.tex` file around a payload.
    ///
    /// The header is the one Wallpaper Engine writes (`TEXV0005` /
    /// `TEXI0002` / `TEXB0003`) with an uncompressed payload.
    fn make_tex(payload: &[u8], width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"TEXV0005");
        out.push(0);
        out.extend_from_slice(b"TEXI0002");
        out.push(0);
        out.extend_from_slice(&0u32.to_le_bytes()); // format: uncompressed
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes()); // the real width
        out.extend_from_slice(&height.to_le_bytes()); // the real height
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved
        out.extend_from_slice(b"TEXB0003");
        out.push(0);
        out.extend_from_slice(&1u32.to_le_bytes()); // image count
        out.extend_from_slice(&[0u8; 8]); // reserved
        out.extend_from_slice(&[0u8; 8]); // reserved
        out.extend_from_slice(&0u32.to_le_bytes()); // lz4
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes()); // decompressed size
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes()); // payload size
        out.extend_from_slice(payload);
        out
    }

    /// A scene whose store holds the given assets.
    fn scene_with(assets: Vec<(&str, Asset)>) -> Scene {
        let mut map = new_map();
        for (key, asset) in assets {
            map.insert(key.to_string(), asset);
        }

        Scene {
            root: Root::default(),
            assets: AssetStore::new(map, None),
            project: None,
        }
    }

    /// The material of the tests: it binds the texture "video".
    fn material() -> Asset {
        Asset::Json(Rc::new(
            r#"{"passes":[{"textures":["video"]}]}"#.to_string(),
        ))
    }

    /// The model of the tests: it points at the material.
    fn model() -> Model {
        Model {
            material: "materials/video.json".to_string(),
            ..Default::default()
        }
    }

    /// The object of the tests.
    fn object() -> Object {
        Object {
            id: 1,
            name: "video".to_string(),
            ..Default::default()
        }
    }

    /// A `.tex` whose payload is a video resolves to a video source.
    ///
    /// Wallpaper Engine stores its video textures that way: the `.tex`
    /// holds the video container as its payload.
    #[test]
    fn a_tex_with_a_video_payload_resolves_to_a_video() {
        let tex = Asset::parse("materials/video.tex", &make_tex(SAMPLE_MP4, 32, 32))
            .expect("the video texture must parse");

        let scene = scene_with(vec![
            ("materials/video.json", material()),
            ("materials/video.tex", tex),
        ]);

        let source = ObjectMap::resolve_texture(&scene, &model(), &object())
            .expect("the video texture must resolve");

        match source {
            TextureSource::Video(video) => {
                assert_eq!(video.info.width, 32);
                assert_eq!(video.info.height, 32);
                assert_eq!(video.size(), [32, 32]);
            }
            TextureSource::Static(_) => panic!("the video must not resolve to a static texture"),
        }
    }

    /// A standalone video next to the material resolves to a video source.
    ///
    /// The clip is small enough to be decoded eagerly: its frames are in
    /// memory, so the renderer never decodes a frame again.
    #[test]
    fn a_standalone_video_resolves_to_a_video() {
        let video = Asset::parse("materials/video.mp4", SAMPLE_MP4).expect("the video must parse");

        let scene = scene_with(vec![
            ("materials/video.json", material()),
            ("materials/video.mp4", video),
        ]);

        let source = ObjectMap::resolve_texture(&scene, &model(), &object())
            .expect("the video must resolve");

        match source {
            TextureSource::Video(video) => {
                assert_eq!(video.info.width, 32);

                // The four frames of the clip were decoded eagerly.
                let frames = video
                    .frames
                    .as_ref()
                    .expect("the clip must have been decoded eagerly");
                assert_eq!(frames.len(), 4);
                assert_eq!(frames[0].width, 32);
                assert_eq!(frames[0].height, 32);
                assert_eq!(frames[0].rgba.len(), 32 * 32 * 4);
            }
            TextureSource::Static(_) => panic!("the video must not resolve to a static texture"),
        }
    }

    /// An ordinary texture resolves to a static source.
    #[test]
    fn a_plain_tex_resolves_to_a_static_texture() {
        // A 2×2 opaque red texture.
        let payload = vec![255u8; 2 * 2 * 4];
        let tex = Asset::parse("materials/video.tex", &make_tex(&payload, 2, 2))
            .expect("the texture must parse");

        let scene = scene_with(vec![
            ("materials/video.json", material()),
            ("materials/video.tex", tex),
        ]);

        let source = ObjectMap::resolve_texture(&scene, &model(), &object())
            .expect("the texture must resolve");

        match source {
            TextureSource::Static(tex) => {
                assert_eq!(tex.dimension, [2, 2]);
                assert_eq!(tex.real_dimension, [2, 2]);
            }
            TextureSource::Video(_) => panic!("the texture must not resolve to a video"),
        }
    }

    /// A video has no padded region: its UV scale is always (1, 1).
    #[test]
    fn a_video_has_no_uv_scale() {
        let video = Asset::parse("materials/video.mp4", SAMPLE_MP4).expect("the video must parse");
        let scene = scene_with(vec![
            ("materials/video.json", material()),
            ("materials/video.mp4", video),
        ]);

        let source = ObjectMap::resolve_texture(&scene, &model(), &object())
            .expect("the video must resolve");

        assert_eq!(source.uv_scale(), Vec2::ONE);
        assert_eq!(source.dimension(), [32, 32]);
    }
}
