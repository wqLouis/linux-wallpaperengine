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
    buffer::Buffers,
    draw::DrawQueue,
    intermediate_pass,
    post_process::PostProcess,
    projection::ProjectionBindGroups,
    render_pass,
    surface::AppSurface,
};

pub use super::surface::InitAppSurface;

/// User interaction parameters that adapters can update (cursor position, etc.)
#[derive(Debug, Clone)]
pub struct UserParams {
    /// Normalized cursor position in [0, 1] range (0,0) = top-left, (1,1) = bottom-right
    pub cursor_position: [f32; 2],
    /// Raw pixel cursor position
    #[allow(dead_code)]
    pub cursor_pixel: [u32; 2],
}

impl Default for UserParams {
    fn default() -> Self {
        Self {
            // Center by default.  On Wayland (wlr adapter) cursor tracking
            // is unavailable, so staying at centre means no parallax shift.
            cursor_position: [0.5, 0.5],
            cursor_pixel: [0, 0],
        }
    }
}

/// Top-level application state owning all WGPU resources.
pub struct WgpuApp {
    pub surface: AppSurface,
    pub buffers: Buffers,
    pub projection_bindgroup: ProjectionBindGroups,
    pub scene_path: String,
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
                    | Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
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
        }
    }

    /// Advance one frame: update time, write uniforms, run effects, render to screen.
    pub fn render(&mut self) -> Option<()> {
        // --- Time tracking ---
        let now = Instant::now();
        let delta = now.saturating_duration_since(self.start_time);
        self.start_time = now;
        self.elapsed_ms = self.elapsed_ms.wrapping_add(delta.as_millis() as u64);
        // Wrap g_Time to 1 hour to maintain f32 precision
        let elapsed = ((self.elapsed_ms % 3_600_000) as f32) / 1000.0;

        log::debug!("frame start: elapsed={:.2}s", elapsed);

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
        log::debug!("screen_res={:?} n_objects={}", screen_res, draw_queue.queue.len());

        // --- Parallax: use adapter cursor position ---
        let mut params = self.user_params.clone();
        params.cursor_position = self.compute_parallax_cursor();

        // --- Upload per-frame uniforms to all effect bind groups ---
        log::debug!("writing effect uniforms...");
        render_pass::write_effect_uniforms(
            &self.queue,
            draw_queue.queue.as_ref(),
            elapsed,
            &self.projection_matrix,
            screen_res,
            &params,
        );

        // --- Run intermediate post-process passes (effects) ---
        let has_intermediates = draw_queue.queue.iter().any(|o| o.intermediates.is_some());
        log::debug!("has_intermediates={}", has_intermediates);
        if has_intermediates {
            intermediate_pass::render_intermediate_passes(
                &self.device,
                &self.queue,
                &self.buffers,
                &self.projection_bindgroup,
                &self.projection_matrix,
                draw_queue,
                post_process,
                elapsed,
                screen_res,
                &params,
            );
            log::debug!("intermediate passes done");
        }

        // --- Final render pass to swapchain ---
        log::debug!("starting final render pass...");
        let result = render_pass::render_final_pass(
            &self.device,
            &self.queue,
            &self.surface,
            &self.buffers,
            &self.projection_bindgroup,
            draw_queue,
            post_process,
            self.clear_color,
        );
        if result.is_some() {
            log::debug!("final render pass OK");
        } else {
            log::warn!("final render pass FAILED");
        }
        result
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        self.surface.config.width = size[0];
        self.surface.config.height = size[1];
        self.surface
            .surface
            .configure(&self.device, &self.surface.config);
    }
}


