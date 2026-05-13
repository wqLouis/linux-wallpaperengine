//! Unified intermediate effect render passes.
//!
//! All effect steps (single-pass and multi-pass flattened) are processed
//! in order using a ping-pong pair of render targets. Steps with a named
//! FBO target write to that FBO instead of ping-pong; steps without a
//! target (including single-pass effects and the final step of multi-pass
//! chains) write to the current ping-pong destination.

use bytemuck::bytes_of;
use log;
use wgpu::*;

use super::{
    app::UserParams,
    draw::DrawQueue,
    post_process::PostProcess,
    post_processor::effect_step,
    projection::ProjectionBindGroups,
    render_pass,
};

pub fn render_intermediate_passes(
    device: &Device,
    queue: &Queue,
    buffers: &super::buffer::Buffers,
    projection_bindgroup: &ProjectionBindGroups,
    projection_matrix: &[[f32; 4]; 4],
    draw_queue: &DrawQueue,
    post_process: &PostProcess,
    elapsed: f32,
    screen_res: [u32; 2],
    user_params: &UserParams,
) {
    log::trace!(
        "starting intermediate passes, {} objects",
        draw_queue.queue.len()
    );
    queue.write_buffer(&buffers.projection, 0, bytes_of(&identity_matrix()));
    render_pass::write_effect_uniforms(
        queue,
        draw_queue.queue.as_ref(),
        elapsed,
        &identity_matrix(),
        screen_res,
        user_params,
    );

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    let proj_bg = projection_bindgroup.projection.as_ref().unwrap();

    for (obj_idx, draw_object) in draw_queue.queue.iter().enumerate() {
        let Some(ref pp) = draw_object.intermediates else {
            continue;
        };

        log::trace!(
            "object[{}] has {} steps",
            obj_idx,
            draw_object.effect_steps.len(),
        );

        // Step 1: copy source texture → view_a
        copy_texture(
            &mut encoder,
            &draw_queue.image_pipeline,
            pp,
            &draw_object.bindgroup,
            proj_bg,
            &pp.view_a,
        );

        // Step 2: process all effect steps in order.
        // Track which ping-pong view holds the latest result.
        let mut cur_is_a = true;

        for step in &draw_object.effect_steps {
            // Determine source: always the current ping-pong result.
            let source_view = if cur_is_a { &pp.view_a } else { &pp.view_b };

            // Build intermediate bindgroup, resolving texture views
            // per step.bind_inputs (ping-pong source / named FBOs).
            let inter_bg = effect_step::make_step_bindgroup(
                device,
                step,
                source_view,
                &draw_object.fbos,
                &post_process.sampler,
            );

            let target_view = match &step.target {
                Some(fbo_name) => match draw_object.fbos.get(fbo_name) {
                    Some(fbo) => {
                        // FBO writes don't advance ping-pong state.
                        &fbo.view
                    }
                    None => {
                        log::error!("unknown FBO '{}', skipping step", fbo_name);
                        continue;
                    }
                },
                None => {
                    // Ping-pong: write to the other view.
                    let dst = if cur_is_a { &pp.view_b } else { &pp.view_a };
                    cur_is_a = !cur_is_a;
                    dst
                }
            };

            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        }),
                        store: StoreOp::Store,
                    },
                })],
                ..Default::default()
            });

            pass.set_pipeline(&step.pipeline);
            pass.set_vertex_buffer(0, pp.ndc_vbuf.slice(..));
            pass.set_index_buffer(pp.ndc_ibuf.slice(..), IndexFormat::Uint32);
            pass.set_bind_group(0, &inter_bg, &[]);
            pass.set_bind_group(1, proj_bg, &[]);
            pass.draw_indexed(0..6, 0, 0..1);
        }

        // Step 3: ensure final result is in view_a.
        if !cur_is_a {
            let bg = pp.make_bindgroup(
                device,
                &post_process.layout,
                &post_process.sampler,
                &pp.view_b,
            );
            copy_texture(
                &mut encoder,
                &draw_queue.image_pipeline,
                pp,
                &bg,
                proj_bg,
                &pp.view_a,
            );
        }
    }

    log::trace!("submitting intermediate encoder...");
    queue.submit(Some(encoder.finish()));
    queue.write_buffer(&buffers.projection, 0, bytes_of(projection_matrix));
    render_pass::write_effect_uniforms(
        queue,
        draw_queue.queue.as_ref(),
        elapsed,
        projection_matrix,
        screen_res,
        user_params,
    );
    log::trace!("intermediate passes done");
}

fn copy_texture(
    encoder: &mut CommandEncoder,
    pipeline: &RenderPipeline,
    pp: &super::ping_pong::PingPongTextures,
    bindgroup: &BindGroup,
    proj_bg: &BindGroup,
    dst_view: &TextureView,
) {
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(RenderPassColorAttachment {
            view: dst_view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                }),
                store: StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.set_vertex_buffer(0, pp.ndc_vbuf.slice(..));
    pass.set_index_buffer(pp.ndc_ibuf.slice(..), IndexFormat::Uint32);
    pass.set_bind_group(0, bindgroup, &[]);
    pass.set_bind_group(1, proj_bg, &[]);
    pass.draw_indexed(0..6, 0, 0..1);
}

fn identity_matrix() -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}
