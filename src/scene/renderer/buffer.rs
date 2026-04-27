use wgpu::*;

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
            size: (std::mem::size_of::<super::draw::Vertex>() as u64 * vertex_len),
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
}
