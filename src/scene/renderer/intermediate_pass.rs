//! Unified intermediate effect render passes.
//!
//! All effect steps (single-pass and multi-pass flattened) are processed
//! in order using a ping-pong pair of render targets. Steps with a named
//! FBO target write to that FBO instead of ping-pong; steps without a
//! target (including single-pass effects and the final step of multi-pass
//! chains) write to the current ping-pong destination.
//!
//! Uses pre-cached bind groups (see [`EffectStep::cache_intermediate_bindgroups`])
//! and an identity projection bind group to avoid per-frame allocations and
//! projection-buffer round-trips.

use log;
use wgpu::*;

use super::{
    draw::DrawQueue,
    post_process::PostProcess,
    projection::ProjectionBindGroups,
};

/// Run intermediate post-process passes for all objects with effects.
///
/// Writes into the shared `encoder` so the caller can batch intermediate
/// and final passes into a single submission.
pub fn render_intermediate_passes(
    encoder: &mut CommandEncoder,
    device: &Device,
    projection_bindgroup: &ProjectionBindGroups,
    draw_queue: &DrawQueue,
    post_process: &PostProcess,
) {
    log::trace!(
        "starting intermediate passes, {} objects",
        draw_queue.queue.len()
    );

    // Use the identity projection bind group for NDC-space rendering.
    let proj_bg = projection_bindgroup
        .identity
        .as_ref()
        .expect("identity projection bindgroup not initialized");

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
            encoder,
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
            // Select the correct pre-cached intermediate bind group.
            let inter_bg = if cur_is_a {
                step.cached_bg_a
                    .as_ref()
                    .unwrap_or_else(|| {
                        log::error!("cached_bg_a missing for step, recreating");
                        panic!("cached_bg_a missing")
                    })
            } else {
                step.cached_bg_b
                    .as_ref()
                    .unwrap_or_else(|| {
                        log::error!("cached_bg_b missing for step, recreating");
                        panic!("cached_bg_b missing")
                    })
            };

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
            pass.set_bind_group(0, inter_bg, &[]);
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
                encoder,
                &draw_queue.image_pipeline,
                pp,
                &bg,
                proj_bg,
                &pp.view_a,
            );
        }
    }

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
