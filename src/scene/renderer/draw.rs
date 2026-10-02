//! Draw queue construction: batches scene objects into GPU draw calls.
//!
//! Each [`TextureObject`] from the scene loader is converted into a
//! [`DrawObject`] with its texture bind group, a flattened list of
//! [`EffectStep`]s (unifying single-pass and multi-pass effects), and
//! optional ping-pong intermediate textures for post-processing.

use std::{collections::BTreeMap, rc::Rc};

use wgpu::*;

use crate::scene::{
    loader::{mip_loader::MipChainGenerator, object_loader::TextureObject},
    renderer::{
        buffer::Buffers,
        ping_pong::PingPongTextures,
        post_processor::{
            context::{EffectContext, EffectTarget, PipelineMap},
            effect_step::{self, EffectStep, FboTexture},
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

/// Context for building the draw queue and its effect pipelines.
pub struct DrawContext<'a> {
    pub effects: EffectContext<'a>,
    pub mipgen: &'a MipChainGenerator,
    pub no_effects: bool,
}

pub struct DrawQueue {
    pub queue: Rc<Vec<DrawObject>>,
    #[allow(dead_code)]
    pub render_pipelines: PipelineMap,
    pub image_pipeline: RenderPipeline,
    /// Additive (`One / One`) blend pipeline used only for the
    /// intermediate source -> ping-pong copy. Kept separate from
    /// `image_pipeline` so the final pass keeps the correct
    /// straight-alpha blend for non-effect objects.
    pub copy_pipeline: RenderPipeline,
}

impl DrawQueue {
    pub fn new(
        ctx: &DrawContext,
        buffers: &mut Buffers,
        texture_objects: Vec<TextureObject>,
        image_pipeline: RenderPipeline,
        copy_pipeline: RenderPipeline,
    ) -> Self {
        let mut render_pipelines = PipelineMap::new();

        let draw_objects: Vec<DrawObject> = texture_objects
            .into_iter()
            .map(|tex_obj| DrawObject::build(ctx, buffers, tex_obj, &mut render_pipelines))
            .collect();

        Self {
            queue: Rc::new(draw_objects),
            render_pipelines,
            image_pipeline,
            copy_pipeline,
        }
    }
}

impl DrawObject {
    fn build(
        ctx: &DrawContext,
        buffers: &mut Buffers,
        texture_object: TextureObject,
        pipelines: &mut PipelineMap,
    ) -> Self {
        let effects = &ctx.effects;
        let device = effects.device;
        let queue = effects.queue;
        let post_process = effects.post_process;
        let index_start = buffers.index_len;

        let texture = Self::upload_texture(device, queue, &texture_object, ctx.mipgen);
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

        let target = EffectTarget {
            view: &source_view,
            width: tex_w,
            height: tex_h,
        };
        let (mut effect_steps, fbos, has_steps) = effect_step::build_effect_steps(
            effects,
            &texture_object.effects,
            pipelines,
            &target,
            ctx.no_effects,
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

        let index_range = if let Some(ref mesh) = texture_object.mesh {
            log::debug!(
                "drawing mesh: {} verts, {} indices",
                mesh.vertices.len(),
                mesh.indices.len(),
            );
            buffers.draw_mesh(
                queue,
                &mesh.vertices,
                &mesh.indices,
                texture_object.model,
                texture_object.transform.position.z - 1.0,
                texture_object.size,
            )
        } else {
            buffers.draw_texture(
                queue,
                texture_object.model,
                texture_object.transform.position.z - 1.0,
                texture_object.size,
                texture_object.uv_scale,
            );
            [index_start, buffers.index_len]
        };

        Self {
            index_range,
            bindgroup,
            effect_steps,
            fbos,
            intermediates,
        }
    }

    fn upload_texture(
        device: &Device,
        queue: &Queue,
        tex_obj: &TextureObject,
        mipgen: &MipChainGenerator,
    ) -> Texture {
        let ext = tex_obj.texture.extension.as_str();
        let is_bcn = matches!(ext, "dxt1" | "dxt5");
        let format = match ext {
            "r8" => TextureFormat::R8Unorm,
            "rg88" => TextureFormat::Rg8Unorm,
            "dxt1" => TextureFormat::Bc1RgbaUnormSrgb,
            "dxt5" => TextureFormat::Bc3RgbaUnormSrgb,
            _ => TextureFormat::Rgba8UnormSrgb,
        };

        let w = tex_obj.texture.dimension[0];
        let h = tex_obj.texture.dimension[1];
        // Allocate the full chain the header advertised, so the GPU has
        // room for the levels the parser didn't ship but the renderer
        // will generate on the GPU. `mipmap_count` is 0 for legacy
        // TEXB0001/0002/0003 headers, in which case we fall back to a
        // single level.
        //
        // BCn textures cannot be rendered into, so the GPU mip generator
        // cannot fill the missing levels; sampling them would read
        // undefined memory (black/transparent). Allocate only the levels
        // the parser actually extracted for BCn.
        let mip_level_count = if is_bcn {
            tex_obj.texture.actual_mip_count().max(1)
        } else {
            tex_obj.texture.mipmap_count.max(1)
        };

        log::debug!(
            "upload_texture: {}x{} fmt={:?} mips={} (advertised={}, actual={}, missing={})",
            w,
            h,
            format,
            mip_level_count,
            tex_obj.texture.mipmap_count,
            tex_obj.texture.actual_mip_count(),
            tex_obj.texture.missing_mip_count(),
        );

        // Non-BCn textures will get GPU-generated mips via `MipChainGenerator`,
        // which needs RENDER_ATTACHMENT. BCn textures can't be filtered on
        // the GPU, so they don't need it.
        let mut usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
        if !is_bcn {
            usage |= TextureUsages::RENDER_ATTACHMENT;
        }

        let texture = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });

        // Upload level 0
        let bytes_per_row = if is_bcn {
            w.div_ceil(4) * if ext == "dxt1" { 8 } else { 16 }
        } else {
            w * match ext {
                "r8" => 1,
                "rg88" => 2,
                _ => 4,
            }
        };
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

        // Upload any mip levels the parser physically extracted from the
        // payload (BCn only, in practice).
        let mut level_w = w;
        let mut level_h = h;
        for (i, level_data) in tex_obj.texture.mip_levels.iter().enumerate() {
            level_w = (level_w / 2).max(1);
            level_h = (level_h / 2).max(1);
            let level = (i + 1) as u32;

            let level_bpr = if is_bcn {
                level_w.div_ceil(4) * if ext == "dxt1" { 8 } else { 16 }
            } else {
                level_w
                    * match ext {
                        "r8" => 1,
                        "rg88" => 2,
                        _ => 4,
                    }
            };

            queue.write_texture(
                TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level,
                    origin: Origin3d::ZERO,
                    aspect: TextureAspect::All,
                },
                level_data,
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(level_bpr),
                    rows_per_image: None,
                },
                Extent3d {
                    width: level_w,
                    height: level_h,
                    depth_or_array_layers: 1,
                },
            );
        }

        // Fill in any remaining mip levels on the GPU. The parser left
        // the chain incomplete by design; the GPU is much faster at
        // mip-downsampling than the CPU, and this keeps the parser out
        // of the rendering business.
        if tex_obj.texture.needs_gpu_mip_generation() && !is_bcn {
            let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
                label: Some("mipgen.encoder"),
            });
            let generated = mipgen.generate(device, &mut encoder, &texture, &tex_obj.texture);
            if generated > 0 {
                queue.submit(std::iter::once(encoder.finish()));
            }
        }

        texture
    }
}
