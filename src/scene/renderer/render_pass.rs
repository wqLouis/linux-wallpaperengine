//! Final rendering pass and effect uniform writing.
//!
//! This module handles the final render pass that draws objects to the
//! swapchain surface, and writes per-frame uniforms (time, cursor, etc.)
//! into effect bind group buffers.

use glam::Vec3;
use log;
use wgpu::*;

use super::{
    app::UserParams,
    buffer::Buffers,
    draw::{DrawObject, DrawQueue},
    post_process::PostProcess,
    post_processor::effect_param::SystemUniforms,
    projection::ProjectionBindGroups,
    surface::AppSurface,
};

/// Render all draw objects to the swapchain surface.
///
/// Each object is drawn with either its direct bind group (no post-processing)
/// or the intermediate ping-pong texture (after applying effects).
pub fn render_final_pass(
    device: &Device,
    queue: &Queue,
    surface: &AppSurface,
    buffers: &Buffers,
    projection_bindgroup: &ProjectionBindGroups,
    draw_queue: &DrawQueue,
    post_process: &PostProcess,
    clear_color: Vec3,
) -> Option<()> {
    // Acquire the next swapchain frame
    let output = match surface.surface.get_current_texture() {
        Ok(frame) => {
            log::debug!("acquired swapchain texture");
            frame
        }
        Err(SurfaceError::Lost | SurfaceError::Outdated) => {
            log::warn!("surface lost/outdated, reconfiguring...");
            surface.surface.configure(device, &surface.config);
            return None;
        }
        Err(SurfaceError::Timeout) => {
            log::warn!("surface timeout");
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
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());

    log::debug!("drawing {} objects...", draw_queue.queue.len());
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
                pp.make_bindgroup(device, &post_process.layout, &post_process.sampler)
            } else {
                draw_object.bindgroup.clone()
            };
            render_pass.set_bind_group(0, &bg, &[]);
            render_pass.draw_indexed(
                draw_object.index_range[0]..draw_object.index_range[1],
                0,
                0..1,
            );
        }
    }

    log::debug!("submitting to queue...");
    queue.submit(Some(encoder.finish()));
    log::debug!("presenting...");
    output.present();
    log::debug!("frame done");
    Some(())
}

/// Write per-frame uniform data into effect bind group buffers.
///
/// Each effect bind group has a uniform buffer containing system values
/// (time, projection, screen resolution, cursor position) and material
/// constants. This function populates and uploads that data every frame.
pub fn write_effect_uniforms(
    queue: &Queue,
    objects: &[DrawObject],
    elapsed: f32,
    projection: &[[f32; 4]; 4],
    screen_res: [u32; 2],
    user_params: &UserParams,
) {
    for draw_object in objects {
        for effect_bg in &draw_object.effect_bindgroups {
            if let Some(ref buf) = effect_bg.uniform_buffer {
                let buf_size = effect_bg.uniform_layout.total_size() as usize;
                let mut staging = vec![0u8; buf_size];

                let sys = SystemUniforms {
                    screen_resolution: screen_res,
                    tex_resolutions: effect_bg.tex_resolutions.clone(),
                    cursor_position: user_params.cursor_position,
                };

                effect_bg.uniform_layout.populate_effect_params(
                    &mut staging,
                    &effect_bg.constants,
                    &effect_bg.material_keys,
                    elapsed,
                    projection,
                    &sys,
                );

                queue.write_buffer(buf, 0, &staging);
            }
        }
    }
}
