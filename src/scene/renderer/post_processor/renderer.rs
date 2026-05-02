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
