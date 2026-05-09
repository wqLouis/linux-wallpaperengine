//! Orthographic camera projection for wallpaper scenes.
//!
//! Builds a combined projection×view matrix from the scene.json camera
//! parameters (eye, center, up, orthogonal projection bounds).

use crate::scene::{loader::scene::Root, renderer::buffer::Buffers};
use bytemuck::bytes_of;
use glam::{Mat4, Vec3};
use wgpu::*;

/// Camera view-projection matrix uploaded to the GPU.
#[repr(C)]
#[derive(Debug, bytemuck::Pod, bytemuck::Zeroable, Clone, Copy)]
pub struct CameraUniform {
    pub projection: [[f32; 4]; 4],
}

/// Orthographic camera parameters parsed from the scene.
pub struct Projection {
    center: Vec3,
    eye: Vec3,
    up: Vec3,
    nearz: f32,
    farz: f32,
    height: f32,
    width: f32,
    _fov: f32,
}

/// Bind group layout and bind group for the camera projection uniform.
pub struct ProjectionBindGroups {
    pub projection_layout: BindGroupLayout,
    pub projection: Option<BindGroup>,
}

impl ProjectionBindGroups {
    pub fn new(device: &Device) -> Self {
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("projection bindgroup layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        Self {
            projection_layout: layout,
            projection: None,
        }
    }

    pub fn create_projection_bindgroup(
        &mut self,
        buffers: &Buffers,
        device: &Device,
        queue: &Queue,
        camera_uniform: &CameraUniform,
    ) {
        self.projection = Some(device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &self.projection_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: buffers.projection.as_entire_binding(),
            }],
        }));

        queue.write_buffer(&buffers.projection, 0, bytes_of(camera_uniform));
    }
}

impl Projection {
    pub fn new(root: &Root) -> Self {
        Projection {
            center: root.camera.center.parse().unwrap(),
            eye: root.camera.eye.parse().unwrap(),
            up: root.camera.up.parse().unwrap(),
            width: root.general.orthogonalprojection.width as f32,
            height: root.general.orthogonalprojection.height as f32,
            nearz: root.general.nearz as f32,
            farz: root.general.farz as f32,
            _fov: root.general.fov as f32,
        }
    }

    pub fn create_camera_uniform(&self) -> CameraUniform {
        let view = Mat4::look_at_rh(self.eye, self.center, self.up);

        let projection =
            Mat4::orthographic_rh(0.0, self.width, 0.0, self.height, self.nearz, self.farz);

        CameraUniform {
            projection: (projection * view).to_cols_array_2d(),
        }
    }
}
