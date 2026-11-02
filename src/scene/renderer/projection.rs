//! Orthographic camera projection for wallpaper scenes.
//!
//! Builds a combined projection×view matrix from the scene.json camera
//! parameters (eye, center, up, orthogonal projection bounds).

use crate::scene::{loader::scene::Root, renderer::buffer::Buffers};
use bytemuck::bytes_of;
use glam::{Mat4, Vec3};
use wgpu::*;

#[repr(C)]
#[derive(Debug, bytemuck::Pod, bytemuck::Zeroable, Clone, Copy)]
pub struct CameraUniform {
    pub projection: [[f32; 4]; 4],
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

pub struct ProjectionBindGroups {
    pub projection_layout: BindGroupLayout,
    pub projection: Option<BindGroup>,
    /// Identity matrix bind group for intermediate (NDC-space) passes.
    pub identity: Option<BindGroup>,
    /// Separate buffer holding identity matrix, so intermediate passes
    /// don't need to overwrite the main projection buffer.
    pub identity_buffer: Buffer,
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

        let identity_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("identity projection buffer"),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
            size: std::mem::size_of::<CameraUniform>() as u64,
        });

        Self {
            projection_layout: layout,
            projection: None,
            identity: None,
            identity_buffer,
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

        // Upload identity to the identity buffer (done once, never changes)
        let identity = CameraUniform {
            projection: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        queue.write_buffer(&self.identity_buffer, 0, bytes_of(&identity));

        self.identity = Some(device.create_bind_group(&BindGroupDescriptor {
            label: Some("identity projection bindgroup"),
            layout: &self.projection_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: self.identity_buffer.as_entire_binding(),
            }],
        }));
    }
}

impl Projection {
    pub fn new(root: &Root) -> Self {
        let center = root.camera.center.parse().unwrap_or_default();
        let eye = root.camera.eye.parse().unwrap_or_default();
        let up = root.camera.up.parse().unwrap_or_default();

        Projection {
            center,
            eye,
            up,
            width: root.general.orthogonalprojection.width as f32,
            height: root.general.orthogonalprojection.height as f32,
            nearz: root.general.nearz as f32,
            farz: root.general.farz as f32,
            _fov: root.general.fov as f32,
        }
    }

    pub fn create_camera_uniform(&self) -> CameraUniform {
        // Wallpaper Engine builds its view-projection as
        //   ortho(0, w, 0, h) * translate(eye) * lookAt(eye, center, up)
        // The eye translation cancels the lookAt translation, so the net
        // effect is the plain orthographic matrix with the camera's
        // rotation (identity when eye and center share x/y).
        //
        // **The scene's world space has its origin in the bottom-left
        // corner of the scene** — the scene spans `[0, w] × [0, h]`.
        // Real `scene.json` files confirm it: the full-screen layer of a
        // wallpaper is always at `(w/2, h/2)` with `size = (w, h)`
        // (e.g. `origin "1920 1080"`, `size "3840 2160"` for a 4K
        // scene), i.e. the layer covers `[0, w] × [0, h]`.
        //
        // => the orthographic box is anchored at the origin
        // (`left = 0`, `bottom = 0`), *not* centred on it.  A centred box
        // would put the whole scene in the top-right quadrant.
        let view = Mat4::look_at_rh(self.eye, self.center, self.up);
        let ortho = Mat4::orthographic_rh(
            0.0,
            self.width,
            0.0,
            self.height,
            self.nearz,
            self.farz,
        );
        let eye_translate = Mat4::from_translation(self.eye);

        CameraUniform {
            projection: (ortho * eye_translate * view).to_cols_array_2d(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera that looks straight down the -z axis, like a default
    /// Wallpaper Engine scene camera (`center = 0 0 0`, `up = 0 1 0`).
    /// The eye has to be different from the centre — `lookAt` is
    /// degenerate otherwise.
    fn centred_camera(width: f32, height: f32) -> Projection {
        Projection {
            center: Vec3::ZERO,
            eye: Vec3::new(0.0, 0.0, 1000.0),
            up: Vec3::Y,
            nearz: 0.1,
            farz: 1000.0,
            width,
            height,
            _fov: 0.0,
        }
    }

    fn ndc(proj: &Projection, p: Vec3) -> Vec3 {
        Mat4::from_cols_array_2d(&proj.create_camera_uniform().projection).transform_point3(p)
    }

    #[test]
    fn scene_origin_is_the_bottom_left_corner() {
        // The scene spans [0, w] x [0, h]: the world origin is the
        // bottom-left corner of the viewport.
        let proj = centred_camera(1920.0, 1080.0);
        let n = ndc(&proj, Vec3::ZERO);
        assert!((n.x + 1.0).abs() < 1e-5, "scene x origin must map to the left edge: {n:?}");
        assert!((n.y + 1.0).abs() < 1e-5, "scene y origin must map to the bottom edge: {n:?}");
    }

    #[test]
    fn full_screen_layer_covers_the_whole_viewport() {
        // A full-screen layer of a wallpaper sits at (w/2, h/2) with the
        // scene's resolution as its size, so its quad covers [0,w]x[0,h].
        let proj = centred_camera(1920.0, 1080.0);
        let top_left = ndc(&proj, Vec3::new(0.0, 1080.0, 0.0));
        let bottom_right = ndc(&proj, Vec3::new(1920.0, 0.0, 0.0));
        assert!((top_left.x + 1.0).abs() < 1e-5, "{top_left:?}");
        assert!((top_left.y - 1.0).abs() < 1e-5, "{top_left:?}");
        assert!((bottom_right.x - 1.0).abs() < 1e-5, "{bottom_right:?}");
        assert!((bottom_right.y + 1.0).abs() < 1e-5, "{bottom_right:?}");
    }

    #[test]
    fn scene_centre_maps_to_the_viewport_centre() {
        // The centre of the scene (w/2, h/2) is the centre of the screen.
        let proj = centred_camera(1920.0, 1080.0);
        let centre = ndc(&proj, Vec3::new(960.0, 540.0, 0.0));
        assert!(centre.x.abs() < 1e-5, "{centre:?}");
        assert!(centre.y.abs() < 1e-5, "{centre:?}");
    }

    #[test]
    fn scene_corner_maps_to_the_viewport_corner() {
        let proj = centred_camera(1920.0, 1080.0);
        let corner = ndc(&proj, Vec3::new(1920.0, 1080.0, 0.0));
        assert!((corner.x - 1.0).abs() < 1e-5, "{corner:?}");
        assert!((corner.y - 1.0).abs() < 1e-5, "{corner:?}");
    }

    /// A real wallpaper `scene.json` (4K scene) — trimmed to the parts the
    /// projection needs.  The full-screen layer sits at `(w/2, h/2)` with
    /// the scene's resolution as its size.
    const REAL_SCENE: &str = r#"
    {
        "camera": { "center": "0 0 -1", "eye": "0 0 0", "up": "0 1 0" },
        "general": {
            "nearz": 0.01, "farz": 10000.0,
            "orthogonalprojection": { "width": 3840, "height": 2160 }
        },
        "objects": [
            {
                "id": 1, "name": "background", "image": "materials/bg.json",
                "origin": "1920 1080 0", "size": "3840 2160", "alignment": "center"
            }
        ]
    }"#;

    #[test]
    fn real_wallpaper_scene_covers_the_viewport() {
        use crate::scene::loader::scene::{Camera, Orthogonalprojection};

        let scene: serde_json::Value = serde_json::from_str(REAL_SCENE).expect("parse scene");
        let camera: Camera = serde_json::from_value(scene["camera"].clone()).expect("camera");
        let ortho: Orthogonalprojection =
            serde_json::from_value(scene["general"]["orthogonalprojection"].clone()).expect("ortho");

        let proj = Projection {
            center: camera.center.parse().unwrap_or_default(),
            eye: camera.eye.parse().unwrap_or_default(),
            up: camera.up.parse().unwrap_or_default(),
            width: ortho.width as f32,
            height: ortho.height as f32,
            nearz: scene["general"]["nearz"].as_f64().unwrap() as f32,
            farz: scene["general"]["farz"].as_f64().unwrap() as f32,
            _fov: 0.0,
        };

        // The scene's bottom-left corner and top-right corner.
        let lo = ndc(&proj, Vec3::new(0.0, 0.0, 0.0));
        let hi = ndc(&proj, Vec3::new(3840.0, 2160.0, 0.0));
        assert!((lo.x + 1.0).abs() < 1e-5, "{lo:?}");
        assert!((lo.y + 1.0).abs() < 1e-5, "{lo:?}");
        assert!((hi.x - 1.0).abs() < 1e-5, "{hi:?}");
        assert!((hi.y - 1.0).abs() < 1e-5, "{hi:?}");

        // The full-screen layer's anchor — the scene's centre — is the
        // centre of the screen.
        let centre = ndc(&proj, Vec3::new(1920.0, 1080.0, 0.0));
        assert!(centre.x.abs() < 1e-5, "{centre:?}");
        assert!(centre.y.abs() < 1e-5, "{centre:?}");
    }
}
