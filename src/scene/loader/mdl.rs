//! Puppet mesh extraction and CPU skinning from parsed MDL files.
//!
//! Converts [`MdlFile`] control points, triangles and skeleton into a
//! GPU-ready rest mesh ([`PuppetMesh`]) plus the data needed to re-skin it
//! every frame from an animation clip.
//!
//! ## Position pipeline
//!
//! 1. **Rest pose** — the control-point positions are already the mesh in
//!    the rest pose, in object-local space (centred around 0).  The bone
//!    matrices in MDLS are *parent-local* bind transforms (`world[i] =
//!    world[parent] * local[i]`), used as the reference for animation;
//!    they are not applied at rest.
//!
//! 2. **Skinning** — for each bone, `skin[i] = anim_world[i] *
//!    bind_world[i]⁻¹`.  A vertex is deformed by blending its (up to four)
//!    bone slots with their weights.  At the bind pose every `skin[i]` is
//!    identity, so the rest mesh is unchanged.
//!
//! 3. **Object-local units** — the vertices keep the MDL's own units and
//!    are handed to the renderer verbatim, exactly like Wallpaper Engine
//!    does (the engine never touches the vertex positions, it only supplies
//!    the bone matrices).  The scene.json `size` of a puppet layer only
//!    affects the *alignment anchor*, not the drawn pixels.
//!
//! ## Matrix convention
//!
//! MDLS/MDLE matrices are stored row-major with the translation in row 3
//! (row-vector convention).  `Mat4::from_cols_array` interprets the same
//! bytes column-major, which is exactly the transpose needed for glam's
//! column-vector convention; the animation keyframes reproduce that
//! matrix when built with `T * R * S`.
//!
//! Wallpaper Engine re-normalises the basis of every bone matrix when it
//! rebuilds them each frame (`col_i = normalize(row_i)`), so **any scale
//! baked into a bone matrix is dropped**.  [`bone_matrix`] does the same.

use std::rc::Rc;

use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use pkg_parser::pkg_parser::mdl_parser::{AnimationClip, BoneEntry, Keyframe, MdlFile, Track};

use crate::scene::renderer::vertex::Vertex;

/// Animation clip selected for an object, plus its playback parameters.
#[derive(Debug, Clone, Copy)]
pub struct PuppetAnimation {
    /// Index into [`MdlFile::animation::clips`].
    pub clip_index: usize,
    /// Playback speed multiplier from the scene's animation layer.
    pub rate: f32,
    /// Layer blend weight (`0.0`..`1.0`).
    pub blend: f32,
    /// `true` for an additive layer, `false` to replace the base pose.
    pub additive: bool,
}

/// Extracted GPU-ready mesh and skeleton from an MDL puppet model.
#[derive(Debug, Clone)]
pub struct PuppetMesh {
    /// Source model (control points, bones, clips).
    pub mdl: Rc<MdlFile>,
    /// Rest-pose vertices in the MDL's own object-local units (used when
    /// not animating).
    pub vertices: Vec<Vertex>,
    /// UV scale of the layer's texture.
    ///
    /// Wallpaper Engine pads some textures to a power of two and only the
    /// top-left `real_dimension` region holds the image.  The puppet's UVs
    /// are relative to the *real* image, so they have to be scaled into the
    /// stored texture's region — exactly like the quad's UVs.
    pub uv_scale: Vec2,
    pub indices: Vec<u32>,
    /// Inverse bind-pose world matrix, one per bone.
    pub bind_world_inv: Vec<Mat4>,
}

impl PuppetMesh {
    /// CPU-skin the mesh for the given animation layers at `time` (seconds).
    ///
    /// The result stays in the MDL's own object-local units — the same
    /// space the rest mesh uses.
    ///
    /// Non-additive layers replace the bind pose; additive layers then apply
    /// their `bind⁻¹ * clip` delta on top (as observed in Wallpaper Engine
    /// scenes, e.g. a `breathing` + `eyes` body rig).  Falls back to the
    /// rest mesh when no layer resolves.
    pub fn skinned_vertices(&self, animations: &[PuppetAnimation], time: f32) -> Vec<Vertex> {
        let mdl = &self.mdl;
        let bones = &mdl.bones.bones;
        if bones.is_empty() {
            return self.vertices.clone();
        }

        let locals = self.animated_locals(animations, time);
        let parents: Vec<u32> = bones.iter().map(|b| b.parent_index).collect();
        let world = compose_world(&parents, &locals);

        mdl.data
            .records
            .iter()
            .map(|cp| {
                let p = Vec3::new(cp.pos_x, cp.pos_y, cp.pos_z);
                let mut out = Vec3::ZERO;
                let mut total = 0.0f32;
                for slot in 0..4 {
                    let weight = cp.weights[slot];
                    if weight == 0.0 {
                        continue;
                    }
                    let bone = cp.bones[slot] as usize;
                    if bone >= world.len() || bone >= self.bind_world_inv.len() {
                        continue;
                    }
                    let skin = world[bone] * self.bind_world_inv[bone];
                    out += skin.transform_point3(p) * weight;
                    total += weight;
                }
                if total <= 0.0 {
                    out = p;
                }
                Vertex {
                    pos: [out.x, out.y, out.z],
                    uv: [
                        cp.tex_u * self.uv_scale.x,
                        cp.tex_v * self.uv_scale.y,
                    ],
                }
            })
            .collect()
    }

    /// Rest-pose world matrix of the named MDAT attachment socket, in the
    /// puppet's local (centred) space.  `None` when the MDL has no such
    /// socket.
    pub fn attachment_world(&self, name: &str) -> Option<Mat4> {
        let entry = self
            .mdl
            .attachments
            .entries
            .iter()
            .find(|e| e.name == name)?;
        let bones = &self.mdl.bones.bones;
        let parents: Vec<u32> = bones.iter().map(|b| b.parent_index).collect();
        let locals: Vec<Mat4> = bones.iter().map(|b| bone_matrix(&b.matrix)).collect();
        let world = compose_world(&parents, &locals);
        let bone = *world.get(entry.bone_index as usize)?;
        Some(bone * Mat4::from_cols_array(&entry.transform))
    }

    /// The attachment (bone socket) matrices of the *animated* pose.
    ///
    /// Wallpaper Engine builds the socket matrices from the runtime bone
    /// matrices, so an object attached to a bone follows the puppet's
    /// animation.  The matrices are in the puppet's local space; the
    /// caller composes them with the puppet's world matrix.
    pub fn animated_attachment_worlds(
        &self,
        animations: &[PuppetAnimation],
        time: f32,
    ) -> Vec<(String, Mat4)> {
        let bones = &self.mdl.bones.bones;
        if bones.is_empty() {
            return Vec::new();
        }

        let locals = self.animated_locals(animations, time);
        let parents: Vec<u32> = bones.iter().map(|b| b.parent_index).collect();
        let world = compose_world(&parents, &locals);

        self.mdl
            .attachments
            .entries
            .iter()
            .filter_map(|entry| {
                let bone = world.get(entry.bone_index as usize).copied()?;
                Some((entry.name.clone(), bone * Mat4::from_cols_array(&entry.transform)))
            })
            .collect()
    }

    /// The per-bone parent-local matrices of the current pose.
    ///
    /// Starts from the bind pose; non-additive animation layers replace it,
    /// additive layers accumulate their `bind⁻¹ * clip` delta on top (as
    /// observed in Wallpaper Engine scenes, e.g. a `breathing` + `eyes` body
    /// rig).
    fn animated_locals(&self, animations: &[PuppetAnimation], time: f32) -> Vec<Mat4> {
        let mdl = &self.mdl;
        let bones = &mdl.bones.bones;

        // Sample every requested layer into per-bone parent-local matrices.
        let sampled: Vec<(Vec<Mat4>, f32, bool)> = animations
            .iter()
            .filter_map(|anim| {
                let clip = mdl.animation.clips.get(anim.clip_index)?;
                let locals = clip
                    .tracks
                    .iter()
                    .map(|track| sample_track(track, clip, time * anim.rate))
                    .collect();
                Some((locals, anim.blend, anim.additive))
            })
            .collect();

        // Start from the bind pose; replace layers overwrite it, additive
        // layers accumulate on top.
        let mut locals: Vec<Mat4> = bones.iter().map(|b| bone_matrix(&b.matrix)).collect();
        for (sampled_locals, _, additive) in &sampled {
            if !*additive {
                locals = sampled_locals.clone();
            }
        }
        for (sampled_locals, blend, additive) in &sampled {
            if !*additive {
                continue;
            }
            for (bone, local) in locals.iter_mut().enumerate() {
                if let Some(sampled_local) = sampled_locals.get(bone) {
                    let bind = bone_matrix(&bones[bone].matrix);
                    let delta = scale_delta(bind.inverse() * *sampled_local, *blend);
                    *local *= delta;
                }
            }
        }

        locals
    }
}

/// Attempt to extract a renderable mesh + skeleton from a parsed MDL file.
///
/// The vertices keep the MDL's own object-local units — Wallpaper Engine
/// never rescales them either, the layer's `size` only shifts the alignment
/// anchor.
///
/// `uv_scale` is the layer's texture scale (`real_dimension / dimension`):
/// textures that are stored padded only carry the image in their top-left
/// region, so the puppet's UVs have to be scaled into it.
pub fn extract_mesh(mdl: Rc<MdlFile>, uv_scale: Vec2) -> Option<PuppetMesh> {
    let num_records = mdl.data.records.len();
    if num_records == 0 || mdl.data.triangles.is_empty() {
        log::debug!("mdl mesh: no records or triangles");
        return None;
    }

    let num_tri_total = mdl.data.triangles.len();

    // Wallpaper Engine hard-fails above 128 bones (`CMP EAX, 0x80` in the
    // MDLS chunk handler); the port copes with more, but says so.
    const MAX_BONES: usize = 128;
    if mdl.bones.bones.len() > MAX_BONES {
        log::warn!(
            "mdl mesh: {} bones — Wallpaper Engine only supports {}",
            mdl.bones.bones.len(),
            MAX_BONES,
        );
    }

    log::debug!(
        "mdl mesh: {} tris, {} verts, {} bones, {} clips",
        num_tri_total,
        num_records,
        mdl.bones.bones.len(),
        mdl.animation.clips.len(),
    );

    // The control points are already in the MDL's own object-local units
    // (centred around the origin) — keep them verbatim, including z.
    let vertices: Vec<Vertex> = mdl
        .data
        .records
        .iter()
        .map(|cp| Vertex {
            pos: [cp.pos_x, cp.pos_y, cp.pos_z],
            uv: [cp.tex_u * uv_scale.x, cp.tex_v * uv_scale.y],
        })
        .collect();

    let indices: Vec<u32> = mdl
        .data
        .triangles
        .iter()
        .flat_map(|t| [t.a as u32, t.b as u32, t.c as u32])
        .collect();

    let bind_world_inv = build_bind_world_inv(&mdl.bones.bones);

    Some(PuppetMesh {
        mdl,
        vertices,
        indices,
        bind_world_inv,
        uv_scale,
    })
}

/// Scale an additive delta matrix by a blend weight.
///
/// `delta` maps the bind pose to the animated pose; `t == 1` keeps it,
/// `t == 0` yields identity, and values in between interpolate the
/// translation, rotation and scale from identity.
fn scale_delta(delta: Mat4, t: f32) -> Mat4 {
    if t >= 1.0 {
        return delta;
    }
    if t <= 0.0 {
        return Mat4::IDENTITY;
    }
    let (scale, rotation, translation) = delta.to_scale_rotation_translation();
    Mat4::from_scale_rotation_translation(
        Vec3::ONE.lerp(scale, t),
        Quat::IDENTITY.slerp(rotation, t),
        translation * t,
    )
}

/// Build a bone matrix the way Wallpaper Engine does.
///
/// The MDL stores the matrix row-major with the translation in row 3, so
/// [`Mat4::from_cols_array`] (which reads the bytes as column-major) yields
/// exactly the transpose Wallpaper Engine performs: column *i* is the
/// stored row *i*.
///
/// The engine then **re-normalises the three basis vectors** when it
/// rebuilds the bone matrices every frame (`col_i = normalize(row_i)`),
/// i.e. any scale baked into the bone matrix is dropped.  The port does
/// the same, otherwise the puppet's limbs would keep a stretch that
/// Wallpaper Engine removes.
pub fn bone_matrix(raw: &[f32; 16]) -> Mat4 {
    normalise_basis(Mat4::from_cols_array(raw))
}

/// Re-normalise the three basis vectors of a matrix, dropping any scale.
///
/// Wallpaper Engine rebuilds every bone matrix each frame as
/// `col_i = normalize(row_i)`.
fn normalise_basis(m: Mat4) -> Mat4 {
    let mut cols = m.to_cols_array();
    for i in 0..3 {
        let (x, y, z) = (cols[i * 4], cols[i * 4 + 1], cols[i * 4 + 2]);
        let len = (x * x + y * y + z * z).sqrt();
        if len > 0.0 {
            cols[i * 4] = x / len;
            cols[i * 4 + 1] = y / len;
            cols[i * 4 + 2] = z / len;
        }
    }
    Mat4::from_cols_array(&cols)
}

/// Inverse of every bone's bind-pose world matrix.
fn build_bind_world_inv(bones: &[BoneEntry]) -> Vec<Mat4> {
    let parents: Vec<u32> = bones.iter().map(|b| b.parent_index).collect();
    let locals: Vec<Mat4> = bones.iter().map(|b| bone_matrix(&b.matrix)).collect();
    compose_world(&parents, &locals)
        .into_iter()
        .map(|m| m.inverse())
        .collect()
}

/// Compose per-bone parent-local matrices into world matrices.
///
/// `locals[i]` is bone `i`'s transform relative to `parents[i]`.  Bones may
/// be listed in any order; the loop resolves parents before children.
fn compose_world(parents: &[u32], locals: &[Mat4]) -> Vec<Mat4> {
    let n = locals.len();
    let mut world = vec![Mat4::IDENTITY; n];
    let mut done = vec![false; n];
    for _ in 0..n {
        for i in 0..n {
            if done[i] {
                continue;
            }
            let parent = parents.get(i).copied().unwrap_or(u32::MAX);
            if parent == u32::MAX {
                world[i] = locals[i];
                done[i] = true;
            } else if (parent as usize) < n && done[parent as usize] {
                world[i] = world[parent as usize] * locals[i];
                done[i] = true;
            }
        }
    }
    world
}

/// Sample one bone's animation track at `time` (already rate-scaled).
///
/// Keyframes are dense (`frame_count + 1` samples).  Looping clips wrap at
/// `frame_count`, so the duplicated last sample is never played; other
/// clips clamp to the end.
fn sample_track(track: &Track, clip: &AnimationClip, time: f32) -> Mat4 {
    let keyframes = &track.keyframes;
    if keyframes.is_empty() {
        return Mat4::IDENTITY;
    }
    let last = keyframes.len() - 1;

    let (i0, i1, frac) = if clip.loop_mode == "loop" && clip.frame_count > 0 {
        let period = clip.frame_count as f32;
        let frame = (time * clip.fps).rem_euclid(period);
        let i0 = (frame.floor() as usize).min(last);
        let i1 = ((i0 + 1) % clip.frame_count as usize).min(last);
        (i0, i1, frame - frame.floor())
    } else {
        let frame = (time * clip.fps).clamp(0.0, last as f32);
        let i0 = frame.floor() as usize;
        (i0, (i0 + 1).min(last), frame - frame.floor())
    };

    interpolate(&keyframes[i0], &keyframes[i1], frac)
}

/// Interpolate two keyframes into a parent-local matrix (`T * R * S`).
///
/// The result is passed through [`bone_matrix`]'s normalisation step,
/// because Wallpaper Engine re-normalises the basis of the runtime bone
/// matrices every frame — the animation's scale is dropped as well.
fn interpolate(a: &Keyframe, b: &Keyframe, t: f32) -> Mat4 {
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    let translation = Vec3::new(lerp(a.tx, b.tx), lerp(a.ty, b.ty), lerp(a.tz, b.tz));
    let scale = Vec3::new(lerp(a.sx, b.sx), lerp(a.sy, b.sy), lerp(a.sz, b.sz));
    let ra = Quat::from_euler(EulerRot::XYZ, a.rx, a.ry, a.rz);
    let rb = Quat::from_euler(EulerRot::XYZ, b.rx, b.ry, b.rz);
    normalise_basis(Mat4::from_scale_rotation_translation(
        scale,
        ra.slerp(rb, t),
        translation,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Load the puppet fixture, or `None` when it is not available.
    ///
    /// The fixture lives in the (git-ignored) `test/` directory, so it is
    /// not part of a fresh checkout — the tests below are skipped then.
    fn sample_mesh() -> Option<PuppetMesh> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/test/yurucamp/models/fuzi_puppet.mdl");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("skipping: fixture {path} not available");
            return None;
        };
        let mdl = MdlFile::new(&bytes).expect("parse fixture");
        assert!(!mdl.animation.clips.is_empty(), "fixture has a clip");
        Some(extract_mesh(Rc::new(mdl), Vec2::ONE).expect("mesh"))
    }

    #[test]
    fn uv_scale_is_applied_to_the_mesh() {
        // Textures that are stored padded only carry the image in their
        // top-left region, so the puppet's UVs have to be scaled into it.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/test/yurucamp/models/fuzi_puppet.mdl");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("skipping: fixture {path} not available");
            return;
        };
        let mdl = MdlFile::new(&bytes).expect("parse fixture");
        let full = extract_mesh(Rc::new(mdl.clone()), Vec2::ONE).expect("mesh");
        let scaled = extract_mesh(Rc::new(mdl), Vec2::new(0.5, 0.25)).expect("mesh");
        for (a, b) in full.vertices.iter().zip(scaled.vertices.iter()) {
            assert!((a.uv[0] * 0.5 - b.uv[0]).abs() < 1e-6);
            assert!((a.uv[1] * 0.25 - b.uv[1]).abs() < 1e-6);
        }
    }

    #[test]
    fn bone_basis_is_normalised_like_wallpaper_engine() {
        // A bone matrix with a baked-in scale: WE rebuilds it as
        // `col_i = normalize(row_i)`, i.e. the scale is dropped.
        let mut raw = [0.0f32; 16];
        raw[0] = 2.0; // row 0, col 0
        raw[5] = 3.0; // row 1, col 1
        raw[10] = 4.0; // row 2, col 2
        raw[15] = 1.0;
        let m = bone_matrix(&raw);
        assert!((m.col(0).x - 1.0).abs() < 1e-6);
        assert!((m.col(1).y - 1.0).abs() < 1e-6);
        assert!((m.col(2).z - 1.0).abs() < 1e-6);
    }

    #[test]
    fn bone_matrix_transposes_the_stored_rows() {
        // The MDL stores the matrix row-major with the translation in row 3,
        // so the translation ends up in the matrix's last column.
        let mut raw = [0.0f32; 16];
        raw[0] = 1.0;
        raw[5] = 1.0;
        raw[10] = 1.0;
        raw[12] = 10.0; // row 3, col 0 -> translation x
        raw[13] = 20.0; // row 3, col 1 -> translation y
        raw[14] = 30.0; // row 3, col 2 -> translation z
        raw[15] = 1.0;
        let m = bone_matrix(&raw);
        assert!((m.col(3).x - 10.0).abs() < 1e-6);
        assert!((m.col(3).y - 20.0).abs() < 1e-6);
        assert!((m.col(3).z - 30.0).abs() < 1e-6);
    }

    #[test]
    fn rest_pose_matches_bind_pose() {
        let Some(mesh) = sample_mesh() else { return };
        let clips: Vec<PuppetAnimation> = (0..mesh.mdl.animation.clips.len())
            .map(|clip_index| PuppetAnimation {
                clip_index,
                rate: 1.0,
                blend: 1.0,
                additive: false,
            })
            .collect();
        let skinned = mesh.skinned_vertices(&clips, 0.0);
        assert_eq!(skinned.len(), mesh.vertices.len());
        for (a, b) in skinned.iter().zip(mesh.vertices.iter()) {
            for axis in 0..3 {
                assert!(
                    (a.pos[axis] - b.pos[axis]).abs() < 1e-3,
                    "frame 0 must reproduce the rest mesh",
                );
            }
        }
    }

    #[test]
    fn animation_deforms_the_mesh() {
        let Some(mesh) = sample_mesh() else { return };
        let clips = [PuppetAnimation {
            clip_index: 0,
            rate: 1.0,
            blend: 1.0,
            additive: false,
        }];
        let rest = &mesh.vertices;
        let moved = (1..=10).any(|step| {
            let skinned = mesh.skinned_vertices(&clips, step as f32 * 0.2);
            skinned.iter().zip(rest.iter()).any(|(a, b)| {
                (0..3).any(|axis| (a.pos[axis] - b.pos[axis]).abs() > 1e-3)
            })
        });
        assert!(moved, "the clip should deform the mesh at some frame");
    }

    #[test]
    fn additive_layers_are_identity_at_frame_zero() {
        let Some(mesh) = sample_mesh() else { return };
        let layer = PuppetAnimation {
            clip_index: 0,
            rate: 1.0,
            blend: 1.0,
            additive: true,
        };
        let skinned = mesh.skinned_vertices(&[layer, layer], 0.0);
        for (a, b) in skinned.iter().zip(mesh.vertices.iter()) {
            for axis in 0..3 {
                assert!(
                    (a.pos[axis] - b.pos[axis]).abs() < 1e-3,
                    "additive layers must cancel at frame 0",
                );
            }
        }
    }
}
