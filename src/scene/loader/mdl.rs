//! Puppet mesh extraction from parsed MDL files.
//!
//! Converts [`MdlFile`] control points and triangles into a GPU-ready
//! mesh ([`PuppetMesh`]) with vertex positions and texture coordinates.
//!
//! ## Position pipeline
//!
//! 1. **Use raw control-point positions directly** — the MDL records
//!    already contain the vertex positions in the rest (reference)
//!    pose, in object-local space (centred around 0 with extent
//!    matching the scene.json `obj_dims`).  The bone matrices stored
//!    in MDLS are *bind-pose* references used as a basis for
//!    animation; they are **not** applied at rest, since doing so
//!    would double-transform the mesh and stretch its parts apart.
//!
//! 2. **Normalise to `[0, 1]^2`** using the scene.json `obj_dims`.
//!    `pos_01 = (pos + obj_dims/2) / obj_dims`.  The renderer's
//!    `draw_mesh` then multiplies by `obj_dims` to recover the
//!    original world-space coordinate.  This preserves the relative
//!    size of sub-meshes (a small eye stays a small eye inside its
//!    obj_dims box — bbox-fitting would over-scale small parts to
//!    fill the box).

use pkg_parser::pkg_parser::mdl_parser::MdlFile;

use crate::scene::renderer::vertex::Vertex;

/// Extracted GPU-ready mesh from an MDL puppet model.
#[derive(Debug, Clone)]
pub struct PuppetMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// Number of control points in the source MDL (for debugging).
    #[allow(dead_code)]
    pub num_control_points: usize,
    /// Number of triangles (for debugging).
    #[allow(dead_code)]
    pub num_triangles_total: usize,
}

/// Attempt to extract a renderable mesh from a parsed MDL file.
///
/// `obj_dims` is the scene.json object size.  It is used as the
/// normaliser for the `[0, 1]` mapping and is the expected extent
/// of the mesh's vertex positions in object-local space.
pub fn extract_mesh(mdl: &MdlFile, obj_dims: [f32; 2]) -> Option<PuppetMesh> {
    let num_records = mdl.data.records.len();
    if num_records == 0 || mdl.data.triangles.is_empty() {
        log::debug!("mdl mesh: no records or triangles");
        return None;
    }

    let num_tri_total = mdl.data.triangles.len();
    let bones = &mdl.bones.bones;

    log::debug!(
        "mdl mesh: {} tris, {} verts, {} bones, obj_dims={}x{}",
        num_tri_total, num_records, bones.len(),
        obj_dims[0] as u32, obj_dims[1] as u32,
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

    Some(PuppetMesh {
        vertices,
        indices,
        num_control_points: num_records,
        num_triangles_total: num_tri_total,
    })
}
