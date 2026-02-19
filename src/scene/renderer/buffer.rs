use wgpu::*;

pub struct Buffers {
    pub vertex: Buffer,
    pub index: Buffer,
    pub projection: Buffer,
}

const MAX_TEXTURE: u32 = 512;
const MAX_VERTEX: u32 = 512 * 4;
const MAX_INDEX: u32 = 512 * 6;

impl Buffers {
    pub fn new(device: &Device) -> Self {
        let vertex = device.create_buffer(&BufferDescriptor {
            label: Some("vertex buffer"),
            usage: BufferUsages::COPY_DST | BufferUsages::VERTEX,
            mapped_at_creation: false,
            size: (std::mem::size_of::<super::draw::Vertex>() as u64 * MAX_VERTEX as u64),
        });
        let index = device.create_buffer(&BufferDescriptor {
            label: Some("index buffer"),
            usage: BufferUsages::COPY_DST | BufferUsages::INDEX,
            mapped_at_creation: false,
            size: (std::mem::size_of::<u32>() as u64 * MAX_INDEX as u64),
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
        }
    }
}
