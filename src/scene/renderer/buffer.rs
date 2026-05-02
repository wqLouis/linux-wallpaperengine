use glam::{Mat2, Vec2, Vec3};
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
    pub(super) fn new(device: &Device, index_len: u64, vertex_len: u64) -> Self {
        let vertex = device.create_buffer(&BufferDescriptor {
            label: Some("vertex buffer"),
            usage: BufferUsages::COPY_DST | BufferUsages::VERTEX,
            mapped_at_creation: false,
            size: (std::mem::size_of::<Vertex>() as u64 * vertex_len),
        });
        let index = device.create_buffer(&BufferDescriptor {
            label: Some("index buffer"),
            usage: BufferUsages::COPY_DST | BufferUsages::INDEX,
            mapped_at_creation: false,
            size: (std::mem::size_of::<u32>() as u64 * index_len),
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
            Vertex { pos: pos[0].to_array(), uv: [0.0, 0.0] },
            Vertex { pos: pos[1].to_array(), uv: [1.0, 0.0] },
            Vertex { pos: pos[2].to_array(), uv: [1.0, 1.0] },
            Vertex { pos: pos[3].to_array(), uv: [0.0, 1.0] },
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

    pub fn draw_texture(&mut self, queue: &Queue, origin: Vec3, angles: Vec3, scale: Vec3, size: Vec2) {
        let size_scaled = size * Vec2 { x: scale.x, y: scale.y };
        let z = origin.z - 1.0;

        let rotation_mat = Mat2::from_angle(angles.z.to_radians());
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
}
