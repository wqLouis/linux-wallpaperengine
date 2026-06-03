//! Top-level WGPU application state and render loop.
//!
//! [`WgpuApp`] owns the GPU device, swapchain, and all rendering
//! resources.  It is created once by an adapter, loaded with a
//! wallpaper scene, then drives the per-frame render loop.

use std::time::Instant;

use glam::Vec3;
use log;
use wgpu::*;

use crate::MAX_TEXTURE;

use super::{
    buffer::Buffers, draw::DrawQueue, intermediate_pass,
    post_process::PostProcess, projection::ProjectionBindGroups,
    render_pass, surface::AppSurface,
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
    pub has_partially_bound: bool,
    pub has_subgroup: bool,
    pub show_progress: bool,
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
                power_preference: PowerPreference::default(),
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

        let has_immediates = enabled_features.contains(Features::IMMEDIATES);
        let has_clear_texture = enabled_features.contains(Features::CLEAR_TEXTURE);
        let has_partially_bound =
            enabled_features.contains(Features::PARTIALLY_BOUND_BINDING_ARRAY);
        let has_subgroup = enabled_features.contains(Features::SUBGROUP);

        let surface = AppSurface::new(surface, &instance, &adapter, size);
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
            has_partially_bound,
            has_subgroup,
            show_progress,
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

        render_pass::write_effect_uniforms(
            &self.queue,
            &mut self.uniform_staging,
            draw_queue.queue.as_ref(),
            elapsed,
            screen_res,
            &params,
        );

        let has_intermediates =
            draw_queue.queue.iter().any(|o| o.intermediates.is_some());

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
                elapsed,
                screen_res,
                &params,
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
            Some(frame) => {
                frame.present();
                Some(())
            }
            None => None,
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
