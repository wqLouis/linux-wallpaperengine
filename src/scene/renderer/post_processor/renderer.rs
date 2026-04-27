#![allow(dead_code, unused_variables)]
use std::rc::Rc;

use wgpu::*;

use crate::scene::renderer::{app::WgpuApp, draw::DrawObject, post_process::PostProcess};

impl PostProcess {
    fn process(
        &self,
        app: &WgpuApp,
        draw_object: &DrawObject,
        pipelines: &Vec<&Rc<RenderPipeline>>,
    ) -> Option<()> {
        let res = app.resolution?;
        let source = draw_object.bindgroup.clone();
        let target = self.blank_texture.clone();
        let target_view = target.create_view(&TextureViewDescriptor::default());

        Some(())
    }
}

/*
impl WgpuApp {
    /// This function process textures with multiple render pipelines
    /// WIP
    pub(super) fn pipelines_process_texture(
        &mut self,
        pipelines: &Vec<&Rc<RenderPipeline>>,
        draw_object: &DrawObject,
    ) {
        let resolution = self.resolution.unwrap();
        let mut post_process = self.post_process.as_mut().unwrap();

        let mut source: &Texture = &post_process.blank_texture;
        let source_view = source.create_view(&Default::default());

        let render_pass_desc = RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &post_process.blank_texture.create_view(&Default::default()),
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
                depth_slice: None,
            })],
            ..Default::default()
        };

        draw_rect(
            &mut post_process.blank_buffers,
            &self.queue,
            [
                Vec3::new(0.0, 0.0, -1.0),
                Vec3::new(resolution[0] as f32, 0.0, -1.0),
                Vec3::new(resolution[0] as f32, resolution[1] as f32, -1.0),
                Vec3::new(0.0, resolution[1] as f32, -1.0),
            ],
        );

        let mut is_first_draw: bool = true;

        for pipeline in pipelines {
            let mut encoder = self
                .device
                .create_command_encoder(&CommandEncoderDescriptor::default());

            {
                {
                    let mut render_pass = encoder.begin_render_pass(&render_pass_desc);

                    if is_first_draw {
                        render_pass.set_vertex_buffer(0, self.buffers.vertex.slice(..));
                        render_pass
                            .set_index_buffer(self.buffers.index.slice(..), IndexFormat::Uint32);
                    } else {
                        render_pass
                            .set_vertex_buffer(0, post_process.blank_buffers.vertex.slice(..));
                        render_pass.set_index_buffer(
                            post_process.blank_buffers.index.slice(..),
                            IndexFormat::Uint32,
                        );
                    }

                    render_pass.set_pipeline(pipeline);
                    render_pass.set_bind_group(1, &self.projection_bindgroup.projection, &[]);

                    if is_first_draw {
                        render_pass.set_bind_group(0, &draw_object.bindgroup, &[]); // The intermediate texture
                    } else {
                        render_pass.set_bind_group(0, &post_process.blank_bindgroup, &[]);
                    }

                    if is_first_draw {
                        render_pass.draw_indexed(
                            draw_object.index_range[0]..draw_object.index_range[1],
                            0,
                            0..1,
                        );
                    } else {
                        render_pass.draw_indexed(0..6, 0, 0..1);
                    }

                    is_first_draw = false;
                }

                self.queue.submit(Some(encoder.finish()));

                source = &post_process.blank_texture;
            }
        }
    }
}

 */
