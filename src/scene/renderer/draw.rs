//! Draw queue construction: batches scene objects into GPU draw calls.
//!
//! Each [`TextureObject`] from the scene loader is converted into a
//! [`DrawObject`] with its texture bind group, a flattened list of
//! [`EffectStep`]s (unifying single-pass and multi-pass effects), and
//! optional ping-pong intermediate textures for post-processing.

use std::{collections::BTreeMap, rc::Rc};

use wgpu::*;

use crate::scene::{
    loader::{mip_loader::MipChainGenerator, object_loader::TextureObject, scene_loader::Scene},
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
    /// Additive (`One / One`) blend pipeline used only for the
    /// intermediate source -> ping-pong copy. Kept separate from
    /// `image_pipeline` so the final pass keeps the correct
    /// straight-alpha blend for non-effect objects.
    pub copy_pipeline: RenderPipeline,
}

impl DrawQueue {
    pub fn new(
        device: &Device,
        queue: &Queue,
        buffers: &mut Buffers,
        scene: &Scene,
        texture_objects: Vec<TextureObject>,
        image_pipeline: RenderPipeline,
        copy_pipeline: RenderPipeline,
        post_process: &PostProcess,
        projection_bgl: &BindGroupLayout,
        mipgen: &MipChainGenerator,
        no_effects: bool,
        has_immediates: bool,
        has_partially_bound: bool,
        has_subgroup: bool,
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
                    mipgen,
                    no_effects,
                    has_immediates,
                    has_partially_bound,
                    has_subgroup,
                )
            })
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
        device: &Device,
        queue: &Queue,
        scene: &Scene,
        texture_object: TextureObject,
        post_process: &PostProcess,
        pipelines: &mut BTreeMap<String, pipeline_handler::EffectPipelineData>,
        buffers: &mut Buffers,
        projection_bgl: &BindGroupLayout,
        mipgen: &MipChainGenerator,
        no_effects: bool,
        has_immediates: bool,
        has_partially_bound: bool,
        has_subgroup: bool,
    ) -> Self {
        let index_start = buffers.index_len;

        let texture = Self::upload_texture(device, queue, &texture_object, mipgen);
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
            has_immediates,
            has_partially_bound,
            has_subgroup,
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
        let mip_level_count = tex_obj.texture.mipmap_count.max(1);

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
            w * match ext { "r8" => 1, "rg88" => 2, _ => 4 }
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
                level_w * match ext { "r8" => 1, "rg88" => 2, _ => 4 }
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
