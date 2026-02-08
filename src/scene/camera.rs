use glam::{Mat4, Vec3};

use crate::scene::General;

use super::Camera;

pub struct Projection {
    center: Vec3,
    eye: Vec3,
    up: Vec3,
    aspect: f32,
    nearz: f32,
    farz: f32,
    height: f32,
    width: f32,
    fov: f32,
}

pub struct WgpuCamera {
    position: Vec3,
    yaw: Vec3,
    pitch: Vec3,
}

impl Camera {
    pub fn new(&self, general: &General) -> Projection {
        // Transform json camera read from json to Wgpu camera
        let center = self.center.parse().unwrap_or_default();
        let eye = self.eye.parse().unwrap_or_default();
        let up = self.up.parse().unwrap_or_default();
        let height: f32 = general.orthogonalprojection.height as f32;
        let width: f32 = general.orthogonalprojection.width as f32;

        Projection {
            // Very rough may breaks
            center: Vec3 {
                x: center[0] as f32,
                y: center[1] as f32,
                z: center[2] as f32,
            },
            eye: Vec3 {
                x: eye[0] as f32,
                y: eye[1] as f32,
                z: eye[2] as f32,
            },
            up: Vec3 {
                x: up[0] as f32,
                y: up[1] as f32,
                z: up[2] as f32,
            },
            aspect: width / height,
            width: width,
            height: height,
            nearz: general.nearz as f32,
            farz: general.farz as f32,
            fov: general.fov as f32,
        }
    }
}

impl Projection {
    pub fn create_projection_matrix(&self) -> Mat4 {
        let view = Mat4::look_at_rh(self.eye, self.center, self.up);

        let projection =
            Mat4::orthographic_rh(0.0, self.width, 0.0, self.height, self.nearz, self.farz);

        projection * view
    }
}
