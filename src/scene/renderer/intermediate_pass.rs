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
    render_pass,
    app::UserParams,
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
    elapsed: f32,
    screen_res: [u32; 2],
    user_params: &UserParams,
    staging: &mut Vec<u8>,
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

        // Step 1: clear ping-pong textures using GPU-side clear if available,
        // then copy source texture → view_a.
        if post_process.has_clear_texture {
            // Use CLEAR_TEXTURE feature for zero-cost GPU clears.
            let subresource = ImageSubresourceRange {
                aspect: TextureAspect::All,
                base_mip_level: 0,
                mip_level_count: None,
                base_array_layer: 0,
                array_layer_count: None,
            };
            encoder.clear_texture(&pp.tex_a, &subresource);
            encoder.clear_texture(&pp.tex_b, &subresource);
        }
        copy_texture(
            encoder,
            &draw_queue.copy_pipeline,
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

            // Use compute dispatch when a compute pipeline is available.
            // Compute shaders can be faster for data-parallel effects like blur/bloom.
            if let Some(ref compute_pipeline) = step.compute_pipeline {
                drop(pass); // End the render pass before starting a compute pass.

                // Determine target dimensions for workgroup dispatch.
                let (target_w, target_h) = match &step.target {
                    Some(fbo_name) => {
                        let fbo = draw_object.fbos.get(fbo_name).unwrap();
                        (fbo.width, fbo.height)
                    }
                    None => (pp.width, pp.height),
                };

                let mut cpass = encoder.begin_compute_pass(&ComputePassDescriptor::default());
                cpass.set_pipeline(compute_pipeline);
                cpass.set_bind_group(0, inter_bg, &[]);
                // Immediates must come after set_pipeline.
                if step.pipedata.layout.use_immediates {
                    let data = render_pass::build_immediates_data(
                        staging, step, elapsed, screen_res, user_params,
                    );
                    cpass.set_immediates(0, data);
                }
                // Dispatch one thread group per 8x8 tile; compute shader declares its workgroup size.
                let groups_x = target_w.div_ceil(8);
                let groups_y = target_h.div_ceil(8);
                cpass.dispatch_workgroups(groups_x, groups_y, 1);
            } else {
                pass.set_pipeline(&step.pipeline);
                // Immediates must come AFTER set_pipeline (wgpu validation).
                if step.pipedata.layout.use_immediates {
                    let data = render_pass::build_immediates_data(
                        staging, step, elapsed, screen_res, user_params,
                    );
                    pass.set_immediates(0, data);
                }
                pass.set_vertex_buffer(0, pp.ndc_vbuf.slice(..));
                pass.set_index_buffer(pp.ndc_ibuf.slice(..), IndexFormat::Uint32);
                pass.set_bind_group(0, inter_bg, &[]);
                pass.set_bind_group(1, proj_bg, &[]);
                pass.draw_indexed(0..6, 0, 0..1);
            }
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
                &draw_queue.copy_pipeline,
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
