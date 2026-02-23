use glam::{Mat4, Vec3};

use crate::scene::loader::scene::Root;

#[repr(C)]
#[derive(Debug, bytemuck::Pod, bytemuck::Zeroable, Clone, Copy)]
pub struct CameraUniform {
    projection: [[f32; 4]; 4],
}

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

impl Projection {
    pub fn new(root: &Root) -> Self {
        Projection {
            // Very rough may breaks
            center: Vec3 {
                x: 0.0,
                y: 0.0,
                z: root.camera.center.parse().unwrap()[2] as f32,
            },
            eye: Vec3 {
                x: 0.0,
                y: 0.0,
                z: root.camera.eye.parse().unwrap()[2] as f32,
            },
            up: Vec3 {
                x: root.camera.up.parse().unwrap()[0] as f32,
                y: root.camera.up.parse().unwrap()[1] as f32,
                z: root.camera.up.parse().unwrap()[2] as f32,
            },
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
