use glam::{Mat4, Vec3, Vec4};
use winit::dpi::PhysicalSize;

use crate::scene::General;

use super::Camera;

#[repr(C)]
#[derive(Debug, bytemuck::Pod, bytemuck::Zeroable, Clone, Copy)]
pub struct CameraUniform {
    projection: [[f32; 4]; 4],
}

pub struct Projection {
    center: Vec3,
    eye: Vec3,
    up: Vec3,
    pub aspect: f32,
    nearz: f32,
    farz: f32,
    pub height: f32,
    pub width: f32,
    fov: f32,
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
    pub fn create_projection_matrix(&self, window_size: &PhysicalSize<f32>) -> CameraUniform {
        let view = Mat4::look_at_rh(self.eye, self.center, self.up);
        let h_ratio = window_size.height / self.height;
        let scaled_w = self.width * h_ratio;
        let overflow_w = (scaled_w - window_size.width) / 2.0;

        let projection = Mat4::orthographic_rh(
            overflow_w,
            scaled_w - overflow_w,
            window_size.height,
            0.0,
            self.nearz,
            self.farz,
        );

        println!(
            "{}",
            (projection * view) * Vec4::new(2000.0, 1080.0, 0.0, 1.0)
        );

        CameraUniform {
            projection: (projection * view).to_cols_array_2d(),
        }
    }
}
