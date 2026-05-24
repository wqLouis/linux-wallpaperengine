//! Draw queue construction: batches scene objects into GPU draw calls.
//!
//! Each [`TextureObject`] from the scene loader is converted into a
//! [`DrawObject`] with its texture bind group, a flattened list of
//! [`EffectStep`]s (unifying single-pass and multi-pass effects), and
//! optional ping-pong intermediate textures for post-processing.

use std::{collections::BTreeMap, rc::Rc};

use wgpu::*;

use crate::scene::{
    loader::{object_loader::TextureObject, scene_loader::Scene},
    renderer::{
        buffer::Buffers,
        ping_pong::PingPongTextures,
        post_process::PostProcess,
        post_processor::{
            effect_step::{self, EffectStep, FboTexture},
            pipeline_handler::{self},
        },
    },
};

pub struct DrawObject {
    pub index_range: [u32; 2],
    pub bindgroup: BindGroup,
    /// All effect steps (single-pass and multi-pass flattened together).
    pub effect_steps: Vec<EffectStep>,
    /// Named FBOs allocated for multi-pass effect chains.
    pub fbos: BTreeMap<String, FboTexture>,
    pub intermediates: Option<PingPongTextures>,
}

pub struct DrawQueue {
    pub queue: Rc<Vec<DrawObject>>,
    #[allow(dead_code)]
    pub render_pipelines: BTreeMap<String, pipeline_handler::EffectPipelineData>,
    pub image_pipeline: RenderPipeline,
}

impl DrawQueue {
    pub fn new(
        device: &Device,
        queue: &Queue,
        buffers: &mut Buffers,
        scene: &Scene,
        texture_objects: Vec<TextureObject>,
        image_pipeline: RenderPipeline,
        post_process: &PostProcess,
        projection_bgl: &BindGroupLayout,
        no_effects: bool,
    ) -> Self {
        let mut render_pipelines = BTreeMap::new();

        let draw_objects: Vec<DrawObject> = texture_objects
            .into_iter()
            .map(|tex_obj| {
                DrawObject::build(
                    device,
                    queue,
                    scene,
                    tex_obj,
                    post_process,
                    &mut render_pipelines,
                    buffers,
                    projection_bgl,
                    no_effects,
                )
            })
            .collect();

        Self {
            queue: Rc::new(draw_objects),
            render_pipelines,
            image_pipeline,
        }
    }
}

impl DrawObject {
    fn build(
        device: &Device,
        queue: &Queue,
        scene: &Scene,
        texture_object: TextureObject,
        post_process: &PostProcess,
        pipelines: &mut BTreeMap<String, pipeline_handler::EffectPipelineData>,
        buffers: &mut Buffers,
        projection_bgl: &BindGroupLayout,
        no_effects: bool,
    ) -> Self {
        let index_start = buffers.index_len;

        let texture = Self::upload_texture(device, queue, &texture_object);
        let source_view = texture.create_view(&Default::default());

        let bindgroup = device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &post_process.layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&source_view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&post_process.sampler),
                },
            ],
        });

        let tex_w = texture_object.texture.dimension[0];
        let tex_h = texture_object.texture.dimension[1];

        let (mut effect_steps, fbos, has_steps) = effect_step::build_effect_steps(
            device,
            queue,
            &texture_object.effects,
            scene,
            post_process,
            pipelines,
            projection_bgl,
            &source_view,
            tex_w,
            tex_h,
            no_effects,
        );

        let mut intermediates = if has_steps {
            let max_w = tex_w.max(post_process.blank_texture.width());
            let max_h = tex_h.max(post_process.blank_texture.height());
            Some(PingPongTextures::new(
                device,
                queue,
                post_process,
                max_w,
                max_h,
            ))
        } else {
            None
        };

        // Pre-cache intermediate and final-pass bind groups.
        if let Some(ref mut pp) = intermediates {
            for step in &mut effect_steps {
                step.cache_intermediate_bindgroups(
                    device,
                    &pp.view_a,
                    &pp.view_b,
                    &fbos,
                    &post_process.sampler,
                );
            }
            pp.cache_final_bindgroup(device, &post_process.layout, &post_process.sampler);
        }

        buffers.draw_texture(
            queue,
            texture_object.origin,
            texture_object.angles,
            texture_object.scale,
            texture_object.size,
        );

        Self {
            index_range: [index_start, buffers.index_len],
            bindgroup,
            effect_steps,
            fbos,
            intermediates,
        }
    }

    fn upload_texture(device: &Device, queue: &Queue, tex_obj: &TextureObject) -> Texture {
        let ext = tex_obj.texture.extension.as_str();
        let (format, bytes_per_row) = match ext {
            "r8" => (TextureFormat::R8Unorm, tex_obj.texture.dimension[0] * 1),
            "rg88" => (TextureFormat::Rg8Unorm, tex_obj.texture.dimension[0] * 2),
            // BCn compressed: raw DXT payload goes directly to GPU, no CPU decode.
            // Block size: 8 bytes (BC1) or 16 bytes (BC3) per 4×4 texel block.
            "dxt1" => (TextureFormat::Bc1RgbaUnormSrgb, tex_obj.texture.dimension[0].div_ceil(4) * 8),
            "dxt5" => (TextureFormat::Bc3RgbaUnormSrgb, tex_obj.texture.dimension[0].div_ceil(4) * 16),
            _ => (TextureFormat::Rgba8UnormSrgb, tex_obj.texture.dimension[0] * 4),
        };

        let w = tex_obj.texture.dimension[0];
        let h = tex_obj.texture.dimension[1];

        let texture = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &tex_obj.texture.payload,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: None,
            },
            Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );

        texture
    }
}
