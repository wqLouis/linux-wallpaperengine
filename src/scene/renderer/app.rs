//! Top-level WGPU application state and render loop.
//!
//! [`WgpuApp`] owns the GPU device, swapchain, and all rendering
//! resources.  It is created once by an adapter, loaded with a
//! wallpaper scene, then drives the per-frame render loop.

use std::{sync::Arc, time::Instant};

use glam::{Mat4, Vec3};
use log;
use wgpu::*;

use crate::MAX_TEXTURE;

use super::{
    buffer::Buffers,
    draw::{self, DrawQueue},
    intermediate_pass,
    post_process::PostProcess,
    projection::{CameraUniform, ProjectionBindGroups},
    render_pass::{self, FinalFrame},
    surface::AppSurface,
};

pub use super::surface::InitAppSurface;

/// Cursor / user-interaction state updated by adapters.
#[derive(Debug, Clone)]
pub struct UserParams {
    pub cursor_position: [f32; 2],
}

impl Default for UserParams {
    fn default() -> Self {
        Self {
            cursor_position: [0.5, 0.5],
        }
    }
}

/// Application state owning all WGPU resources.
pub struct WgpuApp {
    pub surface: AppSurface,
    pub buffers: Option<Buffers>,
    pub projection_bindgroup: ProjectionBindGroups,
    pub scene_path: String,
    pub assets_path: Option<String>,
    pub clear_color: Vec3,
    pub device: Device,
    pub queue: Queue,
    pub audio_stream: rodio::OutputStream,
    pub draw_queue: Option<DrawQueue>,
    pub post_process: Option<PostProcess>,
    pub resolution: Option<[u32; 2]>,
    pub start_time: Instant,
    pub elapsed_ms: u64,
    pub projection_matrix: [[f32; 4]; 4],
    pub no_effects: bool,
    pub no_mdl: bool,
    pub user_params: UserParams,
    pub uniform_staging: Vec<u8>,
    pub has_immediates: bool,
    pub has_clear_texture: bool,
    pub has_subgroup: bool,
    pub show_progress: bool,

    /// Extra scale applied to the camera projection in NDC space.
    ///
    /// Used to implement the fit modes: the scene is scaled up (cover) or
    /// down (contain) so the wallpaper keeps its aspect ratio.
    pub view_scale: [f32; 2],

    /// Staging buffer used to copy off-screen frames back to the CPU.
    readback_buffer: Option<Buffer>,
}

impl WgpuApp {
    fn compute_parallax_cursor(&self) -> [f32; 2] {
        self.user_params.cursor_position
    }

    pub async fn new(
        scene_path: String,
        surface: InitAppSurface,
        size: [u32; 2],
        no_effects: bool,
        no_mdl: bool,
        assets_path: Option<String>,
        show_progress: bool,
    ) -> Self {
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::VULKAN | Backends::METAL,
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::LowPower,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .unwrap();

        let required = Features::TEXTURE_BINDING_ARRAY
            | Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
            | Features::TEXTURE_COMPRESSION_BC;

        let optional = Features::IMMEDIATES
            | Features::PARTIALLY_BOUND_BINDING_ARRAY
            | Features::SUBGROUP
            | Features::CLEAR_TEXTURE;

        let supported = adapter.features();
        let enabled_features = required | (optional & supported);

        let check = |f: Features, name: &str| {
            if enabled_features.contains(f) {
                log::info!("feature '{}' enabled", name);
            } else {
                log::warn!("feature '{}' not supported by adapter", name);
            }
        };
        check(Features::IMMEDIATES, "IMMEDIATES (push constants)");
        check(Features::PARTIALLY_BOUND_BINDING_ARRAY, "PARTIALLY_BOUND");
        check(Features::SUBGROUP, "SUBGROUP");
        check(Features::CLEAR_TEXTURE, "CLEAR_TEXTURE");

        let max_immediate_size = if enabled_features.contains(Features::IMMEDIATES) {
            adapter.limits().max_immediate_size
        } else {
            0
        };

        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: None,
                required_features: enabled_features,
                required_limits: Limits {
                    max_binding_array_elements_per_shader_stage: MAX_TEXTURE,
                    max_immediate_size,
                    ..Default::default()
                },
                experimental_features: ExperimentalFeatures::disabled(),
                memory_hints: MemoryHints::MemoryUsage,
                trace: Trace::Off,
            })
            .await
            .unwrap();

        // wgpu treats uncaptured errors as fatal by default: it logs
        // "Handling wgpu errors as fatal by default" and panics.  The
        // release profile is built with `panic = "abort"`, so a single
        // broken shader (or any other validation error) would kill the
        // whole application.  Install a handler that only logs the error,
        // so the failure is reported and the rest of the wallpaper keeps
        // running.
        let error_handler: Arc<dyn wgpu::UncapturedErrorHandler> =
            Arc::new(|error: wgpu::Error| {
                log::error!("wgpu error: {error}");
            });
        device.on_uncaptured_error(error_handler);

        let has_immediates = enabled_features.contains(Features::IMMEDIATES);
        let has_clear_texture = enabled_features.contains(Features::CLEAR_TEXTURE);
        let has_subgroup = enabled_features.contains(Features::SUBGROUP);

        let surface = AppSurface::new(surface, &instance, &adapter, &device, size);
        // Buffers are allocated later in load() once we know geometry size.
        let projection_bindgroup = ProjectionBindGroups::new(&device);
        let audio_stream = rodio::OutputStreamBuilder::open_default_stream().unwrap();

        Self {
            surface,
            buffers: None,
            projection_bindgroup,
            scene_path,
            assets_path,
            clear_color: Vec3::ZERO,
            device,
            queue,
            audio_stream,
            draw_queue: None,
            resolution: None,
            post_process: None,
            start_time: Instant::now(),
            elapsed_ms: 0,
            projection_matrix: [[1.0; 4]; 4],
            no_effects,
            no_mdl,
            user_params: UserParams::default(),
            uniform_staging: Vec::new(),
            has_immediates,
            has_clear_texture,
            has_subgroup,
            show_progress,
            view_scale: [1.0, 1.0],
            readback_buffer: None,
        }
    }

    pub fn render(&mut self) -> Option<()> {
        let now = Instant::now();
        let delta = now.saturating_duration_since(self.start_time);
        self.start_time = now;
        self.elapsed_ms = self.elapsed_ms.wrapping_add(delta.as_millis() as u64);
        let elapsed = (self.elapsed_ms % 3_600_000) as f32 / 1000.0;

        let draw_queue = self.draw_queue.as_ref()?;
        let post_process = self.post_process.as_ref()?;
        let screen_res = [self.surface.config.width, self.surface.config.height];

        let mut params = self.user_params.clone();
        params.cursor_position = self.compute_parallax_cursor();

        // Re-skin animated puppets before this frame is recorded.
        if let Some(buffers) = self.buffers.as_mut() {
            draw::update_animated_meshes(&self.queue, buffers, draw_queue, elapsed);
        }

        // The encoder of this frame: the conversion of the YUV planes of
        // the videos is recorded into it, before the pass that samples
        // the textures.
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor::default());

        // Decode and upload the animated (video) textures of this frame.
        draw::update_video_textures(&mut encoder, &self.device, &self.queue, draw_queue, elapsed);

        render_pass::write_effect_uniforms(
            &self.queue,
            &mut self.uniform_staging,
            draw_queue.queue.as_ref(),
            elapsed,
            screen_res,
            &params,
        );

        let has_intermediates = draw_queue.queue.iter().any(|o| o.intermediates.is_some());

        if has_intermediates {
            let frame = intermediate_pass::FrameParams {
                projection_bindgroup: &self.projection_bindgroup,
                post_process,
                elapsed,
                screen_res,
                user_params: &params,
            };
            intermediate_pass::render_intermediate_passes(
                &mut encoder,
                &self.device,
                draw_queue,
                &frame,
                &mut self.uniform_staging,
            );
        }

        let buffers = self.buffers.as_ref()?;

        let output = render_pass::render_final_pass(
            &mut encoder,
            &self.device,
            &self.surface,
            buffers,
            &self.projection_bindgroup,
            draw_queue,
            self.clear_color,
        );

        self.queue.submit(Some(encoder.finish()));

        match output {
            Some(FinalFrame::Present(frame)) => {
                frame.present();
                Some(())
            }
            Some(FinalFrame::Offscreen) => Some(()),
            None => None,
        }
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        self.surface.resize(&self.device, size);
    }

    /// Scale the camera projection in NDC space.
    ///
    /// The scene is projected to the full NDC range by default.  Scaling the
    /// NDC coordinates makes the wallpaper overflow the viewport (cover) or
    /// leave empty borders around it (contain) while keeping its aspect
    /// ratio.
    pub fn set_view_scale(&mut self, sx: f32, sy: f32) {
        self.view_scale = [sx, sy];
        self.upload_projection();
    }

    /// Re-upload the (possibly scaled) camera projection to the GPU.
    fn upload_projection(&self) {
        let Some(buffers) = self.buffers.as_ref() else {
            return;
        };

        let base = Mat4::from_cols_array_2d(&self.projection_matrix);
        let scaled =
            Mat4::from_scale(Vec3::new(self.view_scale[0], self.view_scale[1], 1.0)) * base;

        let uniform = CameraUniform {
            projection: scaled.to_cols_array_2d(),
        };
        self.queue
            .write_buffer(&buffers.projection, 0, bytemuck::bytes_of(&uniform));
    }

    /// Copy the off-screen frame into `dst` (tightly packed RGBA8).
    ///
    /// Returns false when the scene is not rendered off-screen or when the
    /// copy failed.
    pub fn read_offscreen_frame(&mut self, dst: &mut [u8]) -> bool {
        let (texture, width, height) = {
            let Some(offscreen) = self.surface.offscreen() else {
                return false;
            };
            (
                offscreen.texture.clone(),
                self.surface.config.width,
                self.surface.config.height,
            )
        };

        let row_bytes = width as usize * 4;
        if dst.len() < row_bytes * height as usize {
            log::error!(
                "readback buffer too small: {} < {}",
                dst.len(),
                row_bytes * height as usize
            );
            return false;
        }

        // The copy destination has to be aligned to 256 bytes per row.
        let bytes_per_row = row_bytes.div_ceil(256) * 256;
        let size = bytes_per_row * height as usize;

        let recreate = match self.readback_buffer.as_ref() {
            Some(buffer) => buffer.size() < size as u64,
            None => true,
        };
        if recreate {
            self.readback_buffer = Some(self.device.create_buffer(&BufferDescriptor {
                label: Some("frame readback"),
                size: size as u64,
                usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }));
        }
        let buffer = self.readback_buffer.as_ref().unwrap();

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            TexelCopyBufferInfo {
                buffer,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row as u32),
                    rows_per_image: Some(height),
                },
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        // Map the staging buffer and wait for the copy to complete.
        let slice = buffer.slice(..);
        let mapped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = mapped.clone();
        slice.map_async(MapMode::Read, move |result| {
            if result.is_ok() {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        });

        if let Err(e) = self.device.poll(PollType::Wait {
            submission_index: None,
            timeout: None,
        }) {
            log::error!("device poll failed: {:?}", e);
            return false;
        }

        if !mapped.load(std::sync::atomic::Ordering::SeqCst) {
            log::error!("failed to map the readback buffer");
            return false;
        }

        {
            let data = slice.get_mapped_range();
            for row in 0..height as usize {
                let src = &data[row * bytes_per_row..row * bytes_per_row + row_bytes];
                let dst_row = &mut dst[row * row_bytes..(row + 1) * row_bytes];
                dst_row.copy_from_slice(src);
            }
        }
        buffer.unmap();

        true
    }
}
