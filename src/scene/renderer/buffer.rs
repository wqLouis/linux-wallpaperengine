use glam::{Vec2, Vec3};
use wgpu::*;

use super::vertex::Vertex;

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

    pub fn draw_rect(&mut self, queue: &Queue, pos: [Vec3; 4]) {
        let rect = [
            Vertex {
                pos: pos[0].to_array(),
                uv: [0.0, 0.0],
            },
            Vertex {
                pos: pos[1].to_array(),
                uv: [1.0, 0.0],
            },
            Vertex {
                pos: pos[2].to_array(),
                uv: [1.0, 1.0],
            },
            Vertex {
                pos: pos[3].to_array(),
                uv: [0.0, 1.0],
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
        z: f32,
        size: Vec2,
    ) {
        let half = Vec2::new(size.x / 2.0, size.y / 2.0);
        let corners = [
            Vec3::new(-half.x, half.y, 0.0),
            Vec3::new(half.x, half.y, 0.0),
            Vec3::new(half.x, -half.y, 0.0),
            Vec3::new(-half.x, -half.y, 0.0),
        ];

        let rect = corners.map(|c| {
            let p = model.transform_point3(c);
            Vec3::new(p.x, p.y, z)
        });

        self.draw_rect(queue, rect);
    }

    /// Upload an arbitrary triangle mesh to the GPU vertex/index buffers.
    ///
    /// Vertex positions are in object-local normalised space `[0,1]^2`
    /// (same convention as [`draw_texture`] corners).  They are scaled by
    /// `size`, recentered, and the `model` matrix (already including
    /// parent composition, alignment, and pivot) maps them to world space.
    pub fn draw_mesh(
        &mut self,
        queue: &Queue,
        vertices: &[Vertex],
        indices: &[u32],
        model: glam::Mat4,
        z: f32,
        size: Vec2,
    ) -> [u32; 2] {
        let index_start = self.index_len;

        let half = Vec2::new(size.x / 2.0, size.y / 2.0);

        // Transform each vertex: [0,1] → [-half, +half] → model matrix.
        let transformed: Vec<Vertex> = vertices
            .iter()
            .map(|v| {
                let local = Vec3::new(
                    v.pos[0] * size.x - half.x,
                    v.pos[1] * size.y - half.y,
                    0.0,
                );
                let world = model.transform_point3(local);
                Vertex {
                    pos: [world.x, world.y, z],
                    uv: v.uv,
                }
            })
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
}
