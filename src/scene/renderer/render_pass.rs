//! Final rendering pass and effect uniform writing.
//!
//! This module handles the final render pass that draws objects to the
//! swapchain surface, and writes per-frame uniforms (time, cursor, etc.)
//! into effect bind group buffers using a reusable staging buffer.

use glam::Vec3;
use log;
use wgpu::*;

use super::{
    app::UserParams,
    buffer::Buffers,
    draw::{DrawObject, DrawQueue},
    post_processor::effect_param::SystemUniforms,
    projection::ProjectionBindGroups,
    surface::AppSurface,
};

/// Render all draw objects to the swapchain surface using the provided encoder.
///
/// Each object is drawn with either its direct bind group (no post-processing)
/// or the intermediate ping-pong texture (after applying effects).
///
/// Returns the swapchain output texture (caller must call `present()` after
/// submitting the encoder).  Returns `None` on surface error.
pub fn render_final_pass<'a>(
    encoder: &'a mut CommandEncoder,
    device: &Device,
    surface: &'a AppSurface,
    buffers: &'a Buffers,
    projection_bindgroup: &'a ProjectionBindGroups,
    draw_queue: &'a DrawQueue,
    clear_color: Vec3,
) -> Option<SurfaceTexture> {
    // Acquire the next swapchain frame
    let output = match surface.surface.get_current_texture() {
        Ok(frame) => {
            log::trace!("acquired swapchain texture");
            frame
        }
        Err(SurfaceError::Lost | SurfaceError::Outdated) => {
            log::warn!("surface lost/outdated, reconfiguring...");
            surface.surface.configure(device, &surface.config);
            return None;
        }
        Err(SurfaceError::Timeout) => {
            log::warn!("surface timeout, reconfiguring...");
            surface.surface.configure(device, &surface.config);
            return None;
        }
        Err(e) => {
            log::error!("surface error: {:?}", e);
            return None;
        }
    };

    let view = output
        .texture
        .create_view(&TextureViewDescriptor::default());

    log::trace!("drawing {} objects...", draw_queue.queue.len());
    {
        let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color {
                        r: (clear_color.x / 255.0) as f64,
                        g: (clear_color.y / 255.0) as f64,
                        b: (clear_color.z / 255.0) as f64,
                        a: 1.0,
                    }),
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });

        render_pass.set_vertex_buffer(0, buffers.vertex.slice(..));
        render_pass.set_index_buffer(buffers.index.slice(..), IndexFormat::Uint32);
        render_pass.set_bind_group(1, projection_bindgroup.projection.as_ref(), &[]);

        for draw_object in draw_queue.queue.iter() {
            render_pass.set_pipeline(&draw_queue.image_pipeline);

            // Use the intermediate result (post-effects) if available,
            // otherwise use the original texture bind group
            let bg = if let Some(ref pp) = draw_object.intermediates {
                // Use cached final-pass bind group to avoid per-frame allocation.
                pp.cached_final_bg_a.as_ref().unwrap_or(&draw_object.bindgroup)
            } else {
                &draw_object.bindgroup
            };
            render_pass.set_bind_group(0, bg, &[]);
            render_pass.draw_indexed(
                draw_object.index_range[0]..draw_object.index_range[1],
                0,
                0..1,
            );
        }
    }

    Some(output)
}

/// Write per-frame uniform data into effect bind group buffers.
///
/// Uses a caller-provided reusable staging buffer to avoid per-frame
/// heap allocations. The staging buffer is resized as needed for the
/// largest uniform block.
///
/// Effect uniforms always use the identity projection matrix since
/// effect pipelines operate in NDC/texture space.
///
/// Steps that use immediates (push constants) are skipped here — their
/// data is written directly via `set_immediates()` in the render pass.
pub fn write_effect_uniforms(
    queue: &Queue,
    staging: &mut Vec<u8>,
    objects: &[DrawObject],
    elapsed: f32,
    screen_res: [u32; 2],
    user_params: &UserParams,
) {
    // Identity matrix — effect pipelines always work in NDC space.
    let identity: [[f32; 4]; 4] = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];

    for draw_object in objects {
        for step in &draw_object.effect_steps {
            // Skip steps that use immediates — data is pushed per render pass.
            if step.pipedata.layout.use_immediates {
                continue;
            }
            if let Some(ref buf) = step.bindgroup.uniform_buffer {
                let buf_size = step.bindgroup.uniform_layout.total_size() as usize;
                // Reuse the caller's staging buffer; grow if needed (amortized).
                if staging.len() < buf_size {
                    staging.resize(buf_size, 0);
                }
                // Zero out the slice we'll use (populate_effect_params calls
                // write_all_defaults first, but being explicit is safe).
                staging[..buf_size].fill(0);

                let sys = SystemUniforms {
                    screen_resolution: screen_res,
                    tex_resolutions: step.bindgroup.tex_resolutions.clone(),
                    cursor_position: user_params.cursor_position,
                };

                step.bindgroup.uniform_layout.populate_effect_params(
                    &mut staging[..buf_size],
                    &step.bindgroup.constants,
                    &step.bindgroup.material_keys,
                    elapsed,
                    &identity,
                    &sys,
                );

                queue.write_buffer(buf, 0, &staging[..buf_size]);
            }
        }
    }
}

/// Build immediates (push constant) data for a single effect step.
/// Returns the byte slice ready to pass to `RenderPass::set_immediates()`.
pub fn build_immediates_data<'a>(
    staging: &'a mut Vec<u8>,
    step: &crate::scene::renderer::post_processor::effect_step::EffectStep,
    elapsed: f32,
    screen_res: [u32; 2],
    user_params: &UserParams,
) -> &'a [u8] {
    let buf_size = step.bindgroup.uniform_layout.total_size() as usize;
    if staging.len() < buf_size {
        staging.resize(buf_size, 0);
    }
    staging[..buf_size].fill(0);

    let identity: [[f32; 4]; 4] = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];

    let sys = SystemUniforms {
        screen_resolution: screen_res,
        tex_resolutions: step.bindgroup.tex_resolutions.clone(),
        cursor_position: user_params.cursor_position,
    };

    step.bindgroup.uniform_layout.populate_effect_params(
        &mut staging[..buf_size],
        &step.bindgroup.constants,
        &step.bindgroup.material_keys,
        elapsed,
        &identity,
        &sys,
    );

    &staging[..buf_size]
}
