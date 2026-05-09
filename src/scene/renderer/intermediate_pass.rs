//! Intermediate effect render passes (ping-pong).
//!
//! For objects with post-process effects, this module renders the source
//! texture through each effect's shader pipeline using a pair of render
//! targets (ping-pong). The final result is used by the final render pass.

use std::rc::Rc;

use bytemuck::bytes_of;
use wgpu::*;

use super::{
    app::UserParams,
    draw::{DrawObject, DrawQueue},
    effect_bindgroup,
    ping_pong::PingPongTextures,
    post_process::PostProcess,
    projection::ProjectionBindGroups,
    render_pass,
};

/// Run multi-pass effect rendering for objects with post-process shaders.
///
/// For each object with effects:
/// 1. Renders the source texture to ping-pong target A
/// 2. Applies each effect shader, alternating between A and B
/// 3. Copies the final result back to A for the final pass
///
/// Temporarily overwrites the projection buffer with identity for NDC rendering.
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
    queue.write_buffer(&buffers.projection, 0, bytes_of(&identity_matrix()));
    render_pass::write_effect_uniforms(
        queue,
        draw_queue.queue.as_ref(),
        elapsed,
        &identity_matrix(),
        screen_res,
        user_params,
    );

    let mut inter_encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());

    for draw_object in draw_queue.queue.iter() {
        let Some(ref pp) = draw_object.intermediates else {
            continue;
        };

        render_source_pass(
            &mut inter_encoder,
            draw_object,
            pp,
            draw_queue,
            projection_bindgroup,
        );

        let n_effects = draw_object.effect_bindgroups.len();
        let mut source_view = &pp.view_a;
        let mut target_view = &pp.view_b;

        for (i, effect_bg) in draw_object.effect_bindgroups.iter().enumerate() {
            let pipedata = draw_queue
                .render_pipelines
                .values()
                .find(|d| Rc::ptr_eq(&d.pipeline, &effect_bg.pipeline));

            let Some(pipedata) = pipedata else {
                eprintln!("[intermediate] skipping effect: pipeline not found");
                continue;
            };

            {
                let mut pass = inter_encoder.begin_render_pass(&RenderPassDescriptor {
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
                pass.set_pipeline(&effect_bg.pipeline);
                pass.set_vertex_buffer(0, pp.ndc_vbuf.slice(..));
                pass.set_index_buffer(pp.ndc_ibuf.slice(..), IndexFormat::Uint32);
                pass.set_bind_group(1, projection_bindgroup.projection.as_ref(), &[]);

                let inter_bg = effect_bindgroup::make_effect_intermediate_bindgroup(
                    device,
                    pipedata,
                    effect_bg,
                    source_view,
                    &post_process.sampler,
                );
                pass.set_bind_group(0, &inter_bg, &[]);
                pass.draw_indexed(0..6, 0, 0..1);
            }

            if i != n_effects - 1 {
                std::mem::swap(&mut source_view, &mut target_view);
            }
        }

        if n_effects % 2 == 1 {
            let bg = pp.make_bindgroup_for(
                device,
                &post_process.layout,
                &post_process.sampler,
                target_view,
            );
            let mut pass = inter_encoder.begin_render_pass(&RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &pp.view_a,
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
            pass.set_pipeline(&draw_queue.image_pipeline);
            pass.set_vertex_buffer(0, pp.ndc_vbuf.slice(..));
            pass.set_index_buffer(pp.ndc_ibuf.slice(..), IndexFormat::Uint32);
            pass.set_bind_group(0, &bg, &[]);
            pass.set_bind_group(1, projection_bindgroup.projection.as_ref(), &[]);
            pass.draw_indexed(0..6, 0, 0..1);
        }
    }

    queue.submit(Some(inter_encoder.finish()));
    queue.write_buffer(&buffers.projection, 0, bytes_of(projection_matrix));
    render_pass::write_effect_uniforms(
        queue,
        draw_queue.queue.as_ref(),
        elapsed,
        projection_matrix,
        screen_res,
        user_params,
    );
}

fn render_source_pass(
    encoder: &mut CommandEncoder,
    draw_object: &DrawObject,
    pp: &PingPongTextures,
    draw_queue: &DrawQueue,
    projection_bindgroup: &ProjectionBindGroups,
) {
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(RenderPassColorAttachment {
            view: &pp.view_a,
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
    pass.set_pipeline(&draw_queue.image_pipeline);
    pass.set_vertex_buffer(0, pp.ndc_vbuf.slice(..));
    pass.set_index_buffer(pp.ndc_ibuf.slice(..), IndexFormat::Uint32);
    pass.set_bind_group(0, &draw_object.bindgroup, &[]);
    pass.set_bind_group(1, projection_bindgroup.projection.as_ref(), &[]);
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
