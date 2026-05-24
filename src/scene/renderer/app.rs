//! Main WGPU application struct and rendering orchestration.
//!
//! [`WgpuApp`] owns all GPU resources and drives the per-frame
//! render loop: time tracking, parallax computation, uniform upload,
//! intermediate effect passes, and the final swapchain render pass.

use std::time::Instant;

use glam::Vec3;
use log;
use wgpu::*;

use crate::{MAX_INDEX, MAX_TEXTURE, MAX_VERTEX};

use super::{
    buffer::Buffers, draw::DrawQueue, intermediate_pass, post_process::PostProcess,
    projection::ProjectionBindGroups, render_pass, surface::AppSurface,
};

pub use super::surface::InitAppSurface;

/// User interaction parameters that adapters can update (cursor position, etc.)
#[derive(Debug, Clone)]
pub struct UserParams {
    /// Normalized cursor position in [0, 1] range (0,0) = top-left, (1,1) = bottom-right
    pub cursor_position: [f32; 2],
}

impl Default for UserParams {
    fn default() -> Self {
        Self {
            // Center by default.  On Wayland (wlr adapter) cursor tracking
            // is unavailable, so staying at centre means no parallax shift.
            cursor_position: [0.5, 0.5],
        }
    }
}

/// Top-level application state owning all WGPU resources.
pub struct WgpuApp {
    pub surface: AppSurface,
    pub buffers: Buffers,
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
    pub user_params: UserParams,
    /// Reusable staging buffer for per-frame uniform writes.
    /// Allocated once and grown on demand to avoid per-frame heap allocations.
    pub uniform_staging: Vec<u8>,
}

impl WgpuApp {
    /// Return the adapter's cursor position for depth parallax.
    /// Falls back to centre when no adapter provides cursor tracking
    /// (e.g. the wlr adapter on Wayland).
    fn compute_parallax_cursor(&self) -> [f32; 2] {
        self.user_params.cursor_position
    }

    pub async fn new(
        scene_path: String,
        surface: InitAppSurface,
        size: [u32; 2],
        no_effects: bool,
        assets_path: Option<String>,
    ) -> Self {
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::VULKAN | Backends::METAL,
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: None,
                required_features: Features::TEXTURE_BINDING_ARRAY
                    | Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
                    | Features::TEXTURE_COMPRESSION_BC,
                required_limits: Limits {
                    max_binding_array_elements_per_shader_stage: MAX_TEXTURE,
                    ..Default::default()
                },
                experimental_features: ExperimentalFeatures::disabled(),
                memory_hints: MemoryHints::MemoryUsage,
                trace: Trace::Off,
            })
            .await
            .unwrap();

        let surface = AppSurface::new(surface, &instance, &adapter, size);
        let buffers = Buffers::new(&device, MAX_INDEX as u64, MAX_VERTEX as u64);
        let projection_bindgroup = ProjectionBindGroups::new(&device);
        let audio_stream = rodio::OutputStreamBuilder::open_default_stream().unwrap();

        Self {
            surface,
            buffers,
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
            no_effects: no_effects,
            user_params: UserParams::default(),
            uniform_staging: Vec::new(),
        }
    }

    /// Advance one frame: update time, write uniforms, run effects, render to screen.
    ///
    /// All GPU work (uniform copies, intermediate passes, final pass) is
    /// batched into a single command encoder and submitted once.
    pub fn render(&mut self) -> Option<()> {
        // --- Time tracking ---
        let now = Instant::now();
        let delta = now.saturating_duration_since(self.start_time);
        self.start_time = now;
        self.elapsed_ms = self.elapsed_ms.wrapping_add(delta.as_millis() as u64);
        // Wrap g_Time to 1 hour to maintain f32 precision
        let elapsed = ((self.elapsed_ms % 3_600_000) as f32) / 1000.0;

        log::trace!("frame start: elapsed={:.2}s", elapsed);

        let draw_queue = match self.draw_queue.as_ref() {
            Some(dq) => dq,
            None => {
                log::error!("ABORT: draw_queue is None");
                return None;
            }
        };
        let post_process = match self.post_process.as_ref() {
            Some(pp) => pp,
            None => {
                log::error!("ABORT: post_process is None");
                return None;
            }
        };
        let screen_res = [self.surface.config.width, self.surface.config.height];
        log::trace!(
            "screen_res={:?} n_objects={}",
            screen_res,
            draw_queue.queue.len()
        );

        // --- Parallax: use adapter cursor position ---
        let mut params = self.user_params.clone();
        params.cursor_position = self.compute_parallax_cursor();

        // --- Upload per-frame uniforms to all effect bind groups ---
        // Effect uniforms are written once with identity projection (effect
        // pipelines always operate in NDC/texture space).
        log::trace!("writing effect uniforms...");
        render_pass::write_effect_uniforms(
            &self.queue,
            &mut self.uniform_staging,
            draw_queue.queue.as_ref(),
            elapsed,
            screen_res,
            &params,
        );

        let has_intermediates = draw_queue.queue.iter().any(|o| o.intermediates.is_some());
        log::trace!("has_intermediates={}", has_intermediates);

        // --- Single command encoder for all GPU work ---
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor::default());

        if has_intermediates {
            intermediate_pass::render_intermediate_passes(
                &mut encoder,
                &self.device,
                &self.projection_bindgroup,
                draw_queue,
                post_process,
            );
            log::trace!("intermediate passes done");
        }

        // --- Final render pass to swapchain ---
        log::trace!("starting final render pass...");
        let output = render_pass::render_final_pass(
            &mut encoder,
            &self.device,
            &self.surface,
            &self.buffers,
            &self.projection_bindgroup,
            draw_queue,
            self.clear_color,
        );

        // --- Submit once ---
        log::trace!("submitting to queue...");
        self.queue.submit(Some(encoder.finish()));

        match output {
            Some(frame) => {
                log::trace!("presenting...");
                frame.present();
                log::trace!("frame done");
                Some(())
            }
            None => {
                log::warn!("final render pass FAILED");
                None
            }
        }
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        self.surface.config.width = size[0];
        self.surface.config.height = size[1];
        self.surface
            .surface
            .configure(&self.device, &self.surface.config);
    }
}
