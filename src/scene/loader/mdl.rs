//! Puppet mesh extraction from parsed MDL files.
//!
//! Converts [`MdlFile`] control points and triangles into a GPU-ready
//! mesh ([`PuppetMesh`]) with vertex positions and texture coordinates.

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
    /// Number of original triangles (before filtering).
    #[allow(dead_code)]
    pub num_triangles_total: usize,
}

/// Attempt to extract a renderable mesh from a parsed MDL file.
///
/// Returns `None` if the MDL contains no usable geometry.
///
/// # Triangle selection
///
/// The MDL gap between control-point records and the MDLS bone section
/// contains two regions: a *quads* section (5-byte header + 6×u16 per
/// quad → 2 triangles, indices in `[0, num_records)`) followed by a
/// *render triangles* section (3×u16 per triangle with higher indices
/// derived from quad subdivision).
///
/// Only quads-derived triangles are directly usable without a
/// tessellator, so we use `mdl.data.quads` (not `.triangles`).
pub fn extract_mesh(mdl: &MdlFile) -> Option<PuppetMesh> {
    let num_records = mdl.data.records.len();
    if num_records == 0 || mdl.data.quads.is_empty() {
        log::debug!("mdl mesh: no records or quads");
        return None;
    }

    let num_tri_total = mdl.data.quads.len();

    // Quads are guaranteed to index within [0, num_records), but we
    // keep this filter as a defensive safety check.
    let valid_tris: Vec<_> = mdl
        .data
        .quads
        .iter()
        .filter(|t| {
            let a = t.a as usize;
            let b = t.b as usize;
            let c = t.c as usize;
            a < num_records && b < num_records && c < num_records
        })
        .collect();

    if valid_tris.is_empty() {
        log::debug!(
            "mdl mesh: {} total triangles, 0 within control-point range (max index {})",
            num_tri_total,
            num_records,
        );
        return None;
    }

    log::debug!(
        "mdl mesh: {} / {} triangles valid, {} control points",
        valid_tris.len(),
        num_tri_total,
        num_records,
    );

    // Convert control points to vertices.
    //
    // The parser stores pos/tex as `raw_i16 / 32767.0` ([-1,1] range).
    // Semantically these are u16 values in [0, 65535] that should map
    // to [0,1].  We recover the original u16 by round-tripping through
    // i16 and reinterpreting the bit pattern.
    //
    // tex_u / tex_v likewise use the u16 range [0,65535] → [0,1] UV.
    let vertices: Vec<Vertex> = mdl
        .data
        .records
        .iter()
        .map(|cp| {
            // Recover raw u16: round(f32 * 32767) → i16 → reinterpret as u16
            let px = ((cp.pos_x * 32767.0).round() as i16 as u16) as f32 / 65535.0;
            let py = ((cp.pos_y * 32767.0).round() as i16 as u16) as f32 / 65535.0;
            let tu = ((cp.tex_u * 32767.0).round() as i16 as u16) as f32 / 65535.0;
            let tv = ((cp.tex_v * 32767.0).round() as i16 as u16) as f32 / 65535.0;
            Vertex {
                pos: [px, py, 0.0],
                uv: [tu, tv],
            }
        })
        .collect();

    let indices: Vec<u32> = valid_tris
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
