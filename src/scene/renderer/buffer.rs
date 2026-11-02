use glam::{Vec2, Vec3};
use wgpu::*;

use super::vertex::Vertex;

/// Depth bias applied to every layer's transformed z.
///
/// The renderer has no depth buffer, but the clip volume still has a depth
/// range: WebGPU's NDC z is `[0, 1]`, so geometry has to sit at least
/// `near` in front of the camera to survive clipping.  Wallpaper Engine's
/// layers live on the z = 0 plane, which is exactly on the near plane, so
/// the whole scene is pushed one unit towards the camera.
///
/// The layer's own z (`origin`) and any z motion of a puppet's bones are
/// preserved on top of this bias: they are part of the transformed
/// position, exactly like in Wallpaper Engine (which multiplies the whole
/// position by `g_ModelViewProjectionMatrix`).
pub const Z_BIAS: f32 = -1.0;

pub struct Buffers {
    pub vertex: Buffer,
    pub index: Buffer,
    pub projection: Buffer,

    pub vertex_len: u32,
    pub index_len: u32,
}

impl Buffers {
    pub(super) fn new(device: &Device, index_capacity: u64, vertex_capacity: u64) -> Self {
        let vertex = device.create_buffer(&BufferDescriptor {
            label: Some("vertex buffer"),
            usage: BufferUsages::COPY_DST | BufferUsages::VERTEX,
            mapped_at_creation: false,
            size: (std::mem::size_of::<Vertex>() as u64 * vertex_capacity),
        });
        let index = device.create_buffer(&BufferDescriptor {
            label: Some("index buffer"),
            usage: BufferUsages::COPY_DST | BufferUsages::INDEX,
            mapped_at_creation: false,
            size: (std::mem::size_of::<u32>() as u64 * index_capacity),
        });
        let projection = device.create_buffer(&BufferDescriptor {
            label: Some("projection buffer"),
            usage: BufferUsages::COPY_DST | BufferUsages::UNIFORM,
            mapped_at_creation: false,
            size: (std::mem::size_of::<super::projection::CameraUniform>() as u64),
        });

        Self {
            vertex,
            index,
            projection,
            vertex_len: 0,
            index_len: 0,
        }
    }

    pub fn draw_rect(&mut self, queue: &Queue, pos: [Vec3; 4], uv_max: Vec2) {
        let rect = [
            Vertex {
                pos: pos[0].to_array(),
                uv: [0.0, 0.0],
            },
            Vertex {
                pos: pos[1].to_array(),
                uv: [uv_max.x, 0.0],
            },
            Vertex {
                pos: pos[2].to_array(),
                uv: [uv_max.x, uv_max.y],
            },
            Vertex {
                pos: pos[3].to_array(),
                uv: [0.0, uv_max.y],
            },
        ];

        let indices: [u32; 6] = [0, 2, 1, 0, 3, 2].map(|f| f + self.vertex_len);

        queue.write_buffer(
            &self.vertex,
            std::mem::size_of::<Vertex>() as BufferAddress * self.vertex_len as BufferAddress,
            bytemuck::bytes_of(&rect),
        );

        queue.write_buffer(
            &self.index,
            std::mem::size_of::<u32>() as BufferAddress * self.index_len as BufferAddress,
            bytemuck::bytes_of(&indices),
        );

        self.index_len += indices.len() as u32;
        self.vertex_len += rect.len() as u32;
    }

    pub fn draw_texture(
        &mut self,
        queue: &Queue,
        model: glam::Mat4,
        size: Vec2,
        uv_max: Vec2,
    ) {
        let local = Self::quad_vertices(size, uv_max);

        let rect = local.map(|v| {
            let p = model.transform_point3(Vec3::from_array(v.pos));
            Vec3::new(p.x, p.y, p.z + Z_BIAS)
        });

        self.draw_rect(queue, rect, uv_max);
    }

    /// The four object-local vertices of a layer's quad (positions + UVs).
    ///
    /// The quad is centred on the layer's anchor and spans `size`; the
    /// texture's top-left corner is at `(-half.x, +half.y)` with uv `(0,0)`
    /// (y-up, upright — same as Wallpaper Engine).
    pub fn quad_vertices(size: Vec2, uv_max: Vec2) -> [Vertex; 4] {
        let half = Vec2::new(size.x / 2.0, size.y / 2.0);
        let corners = [
            Vec3::new(-half.x, half.y, 0.0),
            Vec3::new(half.x, half.y, 0.0),
            Vec3::new(half.x, -half.y, 0.0),
            Vec3::new(-half.x, -half.y, 0.0),
        ];
        let uvs = [
            [0.0, 0.0],
            [uv_max.x, 0.0],
            [uv_max.x, uv_max.y],
            [0.0, uv_max.y],
        ];

        std::array::from_fn(|i| Vertex {
            pos: corners[i].to_array(),
            uv: uvs[i],
        })
    }

    /// Upload an arbitrary triangle mesh to the GPU vertex/index buffers.
    ///
    /// Vertex positions are in object-local units (for a puppet: the MDL's
    /// own units, verbatim — Wallpaper Engine never rescales them
    /// either).  The `model` matrix (already including parent composition
    /// and the alignment anchor) maps them to world space.
    pub fn draw_mesh(
        &mut self,
        queue: &Queue,
        vertices: &[Vertex],
        indices: &[u32],
        model: glam::Mat4,
    ) -> [u32; 2] {
        let index_start = self.index_len;

        let transformed: Vec<Vertex> = vertices
            .iter()
            .map(|v| place_vertex(v, model))
            .collect();

        // Offset indices by the current vertex count
        let offset_indices: Vec<u32> =
            indices.iter().map(|i| i + self.vertex_len).collect();

        let vertex_bytes: &[u8] = bytemuck::cast_slice(&transformed);
        let index_bytes: &[u8] = bytemuck::cast_slice(&offset_indices);

        queue.write_buffer(
            &self.vertex,
            std::mem::size_of::<Vertex>() as BufferAddress * self.vertex_len as BufferAddress,
            vertex_bytes,
        );
        queue.write_buffer(
            &self.index,
            std::mem::size_of::<u32>() as BufferAddress * self.index_len as BufferAddress,
            index_bytes,
        );

        self.vertex_len += vertices.len() as u32;
        self.index_len += indices.len() as u32;

        [index_start, self.index_len]
    }

    /// Rewrite an existing vertex range with freshly skinned positions.
    ///
    /// Positions use the same object-local convention as [`draw_mesh`], so
    /// the caller can hand back the output of
    /// [`crate::scene::loader::mdl::PuppetMesh::skinned_vertices`].  The
    /// index buffer is untouched — only the vertex positions change each
    /// frame.
    pub fn update_mesh(
        &self,
        queue: &Queue,
        vertex_offset: u32,
        vertices: &[Vertex],
        model: glam::Mat4,
    ) {
        let transformed: Vec<Vertex> = vertices
            .iter()
            .map(|v| place_vertex(v, model))
            .collect();

        queue.write_buffer(
            &self.vertex,
            std::mem::size_of::<Vertex>() as BufferAddress * vertex_offset as BufferAddress,
            bytemuck::cast_slice(&transformed),
        );
    }
}

/// Place an object-local vertex in world space.
///
/// The vertex is transformed by the `model` matrix, which carries the
/// parent composition, the rotation/scale and the alignment anchor.  The
/// **transformed z is kept**: Wallpaper Engine multiplies the whole
/// position by `g_ModelViewProjectionMatrix`, so a puppet whose bones move
/// in z (or a layer rotated about x/y) keeps its depth instead of being
/// flattened onto a constant plane.  The layer's z only acts as a depth
/// bias ([`Z_BIAS`]).
fn place_vertex(v: &Vertex, model: glam::Mat4) -> Vertex {
    let world = model.transform_point3(Vec3::from_array(v.pos));
    Vertex {
        pos: [world.x, world.y, world.z + Z_BIAS],
        uv: v.uv,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Mat4;

    fn vertex(pos: [f32; 3]) -> Vertex {
        Vertex {
            pos,
            uv: [0.25, 0.75],
        }
    }

    #[test]
    fn place_vertex_keeps_the_transformed_z() {
        // Wallpaper Engine multiplies the whole position by the model-view-
        // projection matrix, so a layer's z survives the transform.
        let model = Mat4::from_translation(Vec3::new(10.0, 20.0, 5.0));
        let out = place_vertex(&vertex([1.0, 2.0, 3.0]), model);
        assert_eq!(out.pos[0], 11.0);
        assert_eq!(out.pos[1], 22.0);
        assert_eq!(out.pos[2], 3.0 + 5.0 + Z_BIAS);
    }

    #[test]
    fn place_vertex_keeps_bone_motion_in_z() {
        // A puppet vertex that was pushed forward by a bone keeps its depth.
        let model = Mat4::from_translation(Vec3::new(0.0, 0.0, 0.0));
        let out = place_vertex(&vertex([0.0, 0.0, 7.0]), model);
        assert_eq!(out.pos[2], 7.0 + Z_BIAS);
    }

    #[test]
    fn place_vertex_preserves_the_uvs() {
        let out = place_vertex(&vertex([0.0, 0.0, 0.0]), Mat4::IDENTITY);
        assert_eq!(out.uv, [0.25, 0.75]);
    }

    #[test]
    fn quad_is_centred_and_y_up() {
        // The texture's top-left corner is at (-half.x, +half.y) with uv
        // (0, 0) — the same orientation as Wallpaper Engine.
        let quad = Buffers::quad_vertices(Vec2::new(100.0, 50.0), Vec2::new(1.0, 1.0));
        assert_eq!(quad[0].pos, [-50.0, 25.0, 0.0]);
        assert_eq!(quad[0].uv, [0.0, 0.0]);
        assert_eq!(quad[1].pos, [50.0, 25.0, 0.0]);
        assert_eq!(quad[1].uv, [1.0, 0.0]);
        assert_eq!(quad[2].pos, [50.0, -25.0, 0.0]);
        assert_eq!(quad[2].uv, [1.0, 1.0]);
        assert_eq!(quad[3].pos, [-50.0, -25.0, 0.0]);
        assert_eq!(quad[3].uv, [0.0, 1.0]);
    }
}
