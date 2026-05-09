use wgpu::*;

/// A fullscreen quad in normalized device coordinates `[-1, 1]`.
/// Used by ping-pong intermediate effect passes.
pub const NDC_VERTICES: [Vertex; 4] = [
    Vertex {
        pos: [-1.0, 1.0, 0.0],
        uv: [0.0, 0.0],
    },
    Vertex {
        pos: [1.0, 1.0, 0.0],
        uv: [1.0, 0.0],
    },
    Vertex {
        pos: [1.0, -1.0, 0.0],
        uv: [1.0, 1.0],
    },
    Vertex {
        pos: [-1.0, -1.0, 0.0],
        uv: [0.0, 1.0],
    },
];

/// A single vertex with position and UV coordinates.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
}

impl Vertex {
    pub fn create_buffer_layout<'a>() -> VertexBufferLayout<'a> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as BufferAddress,
            step_mode: VertexStepMode::Vertex,
            attributes: &[
                VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: VertexFormat::Float32x3,
                },
                VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as BufferAddress,
                    shader_location: 1,
                    format: VertexFormat::Float32x2,
                },
            ],
        }
    }
}
