use glam::{Mat2, Vec2, Vec3};
use wgpu::*;

use super::vertex::Vertex;

pub struct Buffers {
    pub vertex: Buffer,
    pub index: Buffer,
    pub projection: Buffer,

    pub vertex_capacity: u32,
    pub index_capacity: u32,
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
            vertex_capacity: vertex_capacity as u32,
            index_capacity: index_capacity as u32,
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
        origin: Vec3,
        angles: Vec3,
        scale: Vec3,
        size: Vec2,
    ) {
        let size_scaled = size
            * Vec2 {
                x: scale.x,
                y: scale.y,
            };
        let z = origin.z - 1.0;

        let rotation_mat = Mat2::from_angle(angles.z);
        let half = Vec2::new(size_scaled.x / 2.0, size_scaled.y / 2.0);
        let corners = [
            Vec2::new(-half.x, half.y),
            Vec2::new(half.x, half.y),
            Vec2::new(half.x, -half.y),
            Vec2::new(-half.x, -half.y),
        ];

        let pos_offset = Vec2::new(origin.x, origin.y);
        let rect = corners.map(|v| {
            let rotated = rotation_mat * v + pos_offset;
            Vec3::new(rotated.x, rotated.y, z)
        });

        self.draw_rect(queue, rect);
    }

    /// Upload an arbitrary triangle mesh to the GPU vertex/index buffers.
    ///
    /// Vertex positions are in object-local normalised space `[0,1]^2`
    /// (same convention as [`draw_texture`] corners).  They are scaled by
    /// `size`, centered, rotated by `angles.z`, and translated by `origin`.
    pub fn draw_mesh(
        &mut self,
        queue: &Queue,
        vertices: &[Vertex],
        indices: &[u32],
        origin: Vec3,
        angles: Vec3,
        scale: Vec3,
        size: Vec2,
    ) -> [u32; 2] {
        let index_start = self.index_len;

        let size = size
            * Vec2 {
                x: scale.x,
                y: scale.y,
            };
        let z = origin.z - 1.0;

        let rotation_mat = Mat2::from_angle(angles.z);
        let half = Vec2::new(size.x / 2.0, size.y / 2.0);
        let pos_offset = Vec2::new(origin.x, origin.y);

        // Transform each vertex: [0,1] → [-half, +half] → rotate → translate
        let transformed: Vec<Vertex> = vertices
            .iter()
            .map(|v| {
                let p = Vec2::new(v.pos[0] * size.x - half.x, v.pos[1] * size.y - half.y);
                let rotated = rotation_mat * p + pos_offset;
                Vertex {
                    pos: [rotated.x, rotated.y, z],
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
