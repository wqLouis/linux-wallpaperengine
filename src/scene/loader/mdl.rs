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
//! 3. **Normalise to `[0, 1]^2`** using the scene.json `obj_dims`:
//!    `pos_01 = (pos + obj_dims/2) / obj_dims`.  The renderer's `draw_mesh`
//!    multiplies by `obj_dims` to recover the original world-space
//!    coordinate, preserving the relative size of sub-meshes.
//!
//! ## Matrix convention
//!
//! MDLS/MDLE matrices are stored row-major with the translation in row 3
//! (row-vector convention).  `Mat4::from_cols_array` interprets the same
//! bytes column-major, which is exactly the transpose needed for glam's
//! column-vector convention; the animation keyframes reproduce that
//! matrix when built with `T * R * S`.

use std::rc::Rc;

use glam::{EulerRot, Mat4, Quat, Vec3};
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
    /// Rest-pose vertices normalised to `[0, 1]^2` (used when not animating).
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// Inverse bind-pose world matrix, one per bone.
    pub bind_world_inv: Vec<Mat4>,
}

impl PuppetMesh {
    /// CPU-skin the mesh for the given animation layers at `time` (seconds)
    /// and normalise the result to `[0, 1]^2` with `obj_dims`.
    ///
    /// Non-additive layers replace the bind pose; additive layers then apply
    /// their `bind⁻¹ * clip` delta on top (as observed in Wallpaper Engine
    /// scenes, e.g. a `breathing` + `eyes` body rig).  Falls back to the
    /// rest mesh when no layer resolves.
    pub fn skinned_vertices(
        &self,
        animations: &[PuppetAnimation],
        time: f32,
        obj_dims: [f32; 2],
    ) -> Vec<Vertex> {
        let mdl = &self.mdl;
        let bones = &mdl.bones.bones;
        if bones.is_empty() {
            return self.vertices.clone();
        }

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
        let mut locals: Vec<Mat4> = bones
            .iter()
            .map(|b| Mat4::from_cols_array(&b.matrix))
            .collect();
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
                    let bind = Mat4::from_cols_array(&bones[bone].matrix);
                    let delta = scale_delta(bind.inverse() * *sampled_local, *blend);
                    *local *= delta;
                }
            }
        }

        let parents: Vec<u32> = bones.iter().map(|b| b.parent_index).collect();
        let world = compose_world(&parents, &locals);

        let obj_w = obj_dims[0].max(f32::EPSILON);
        let obj_h = obj_dims[1].max(f32::EPSILON);
        let half_w = obj_w * 0.5;
        let half_h = obj_h * 0.5;

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
                    pos: [(out.x + half_w) / obj_w, (out.y + half_h) / obj_h, 0.0],
                    uv: [cp.tex_u, cp.tex_v],
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
        let locals: Vec<Mat4> = bones
            .iter()
            .map(|b| Mat4::from_cols_array(&b.matrix))
            .collect();
        let world = compose_world(&parents, &locals);
        let bone = *world.get(entry.bone_index as usize)?;
        Some(bone * Mat4::from_cols_array(&entry.transform))
    }
}

/// Attempt to extract a renderable mesh + skeleton from a parsed MDL file.
///
/// `obj_dims` is the scene.json object size.  It is used as the
/// normaliser for the `[0, 1]` mapping and is the expected extent
/// of the mesh's vertex positions in object-local space.
pub fn extract_mesh(mdl: Rc<MdlFile>, obj_dims: [f32; 2]) -> Option<PuppetMesh> {
    let num_records = mdl.data.records.len();
    if num_records == 0 || mdl.data.triangles.is_empty() {
        log::debug!("mdl mesh: no records or triangles");
        return None;
    }

    let num_tri_total = mdl.data.triangles.len();

    log::debug!(
        "mdl mesh: {} tris, {} verts, {} bones, {} clips, obj_dims={}x{}",
        num_tri_total,
        num_records,
        mdl.bones.bones.len(),
        mdl.animation.clips.len(),
        obj_dims[0] as u32,
        obj_dims[1] as u32,
    );

    // Map raw object-local positions to [0, 1] using obj_dims.
    // Guard against zero-size to avoid division by zero.
    let obj_w = obj_dims[0].max(f32::EPSILON);
    let obj_h = obj_dims[1].max(f32::EPSILON);
    let half_w = obj_w * 0.5;
    let half_h = obj_h * 0.5;

    let vertices: Vec<Vertex> = mdl
        .data
        .records
        .iter()
        .map(|cp| Vertex {
            pos: [(cp.pos_x + half_w) / obj_w, (cp.pos_y + half_h) / obj_h, 0.0],
            uv: [cp.tex_u, cp.tex_v],
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

/// Inverse of every bone's bind-pose world matrix.
fn build_bind_world_inv(bones: &[BoneEntry]) -> Vec<Mat4> {
    let parents: Vec<u32> = bones.iter().map(|b| b.parent_index).collect();
    let locals: Vec<Mat4> = bones
        .iter()
        .map(|b| Mat4::from_cols_array(&b.matrix))
        .collect();
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
fn interpolate(a: &Keyframe, b: &Keyframe, t: f32) -> Mat4 {
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    let translation = Vec3::new(lerp(a.tx, b.tx), lerp(a.ty, b.ty), lerp(a.tz, b.tz));
    let scale = Vec3::new(lerp(a.sx, b.sx), lerp(a.sy, b.sy), lerp(a.sz, b.sz));
    let ra = Quat::from_euler(EulerRot::XYZ, a.rx, a.ry, a.rz);
    let rb = Quat::from_euler(EulerRot::XYZ, b.rx, b.ry, b.rz);
    Mat4::from_scale_rotation_translation(scale, ra.slerp(rb, t), translation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_mesh() -> (PuppetMesh, [f32; 2]) {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/test/yurucamp/models/fuzi_puppet.mdl");
        let bytes = std::fs::read(path).expect("fixture mdl");
        let mdl = MdlFile::new(&bytes).expect("parse fixture");
        assert!(!mdl.animation.clips.is_empty(), "fixture has a clip");
        let dims = [1920.0, 1080.0];
        (extract_mesh(Rc::new(mdl), dims).expect("mesh"), dims)
    }

    #[test]
    fn rest_pose_matches_bind_pose() {
        let (mesh, dims) = sample_mesh();
        let clips: Vec<PuppetAnimation> = (0..mesh.mdl.animation.clips.len())
            .map(|clip_index| PuppetAnimation {
                clip_index,
                rate: 1.0,
                blend: 1.0,
                additive: false,
            })
            .collect();
        let skinned = mesh.skinned_vertices(&clips, 0.0, dims);
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
        let (mesh, dims) = sample_mesh();
        let clips = [PuppetAnimation {
            clip_index: 0,
            rate: 1.0,
            blend: 1.0,
            additive: false,
        }];
        let rest = &mesh.vertices;
        let moved = (1..=10).any(|step| {
            let skinned = mesh.skinned_vertices(&clips, step as f32 * 0.2, dims);
            skinned.iter().zip(rest.iter()).any(|(a, b)| {
                (0..3).any(|axis| (a.pos[axis] - b.pos[axis]).abs() > 1e-3)
            })
        });
        assert!(moved, "the clip should deform the mesh at some frame");
    }

    #[test]
    fn additive_layers_are_identity_at_frame_zero() {
        let (mesh, dims) = sample_mesh();
        let layer = PuppetAnimation {
            clip_index: 0,
            rate: 1.0,
            blend: 1.0,
            additive: true,
        };
        let skinned = mesh.skinned_vertices(&[layer, layer], 0.0, dims);
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
