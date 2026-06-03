//! GPU-side mip chain generation for Wallpaper Engine textures.
//!
//! The Wallpaper Engine `.tex` parser no longer synthesizes mipmaps on the
//! CPU — mip generation is the renderer's job, and the GPU is much faster
//! at it. This module provides a reusable [`MipChainGenerator`] that
//! downsamples an existing mip level into the next one using hardware
//! linear filtering (the same technique a "blit with linear" would use).
//!
//! # Usage
//!
//! 1. Create one [`MipChainGenerator`] per renderer in `load.rs`.
//! 2. After [`crate::scene::renderer::draw::DrawObject`] uploads the
//!    levels that are physically in the parser's payload
//!    ([`Tex::mip_levels`]), call
//!    [`MipChainGenerator::generate`] with the just-created `wgpu::Texture`
//!    and the [`Tex`] it was sourced from. The generator will populate
//!    every level from `Tex::actual_mip_count()` up to
//!    `Tex::mipmap_count() - 1`.
//!
//! # Format support
//!
//! Only non-block-compressed filterable formats are supported (RGBA8,
//! RGBA8 sRGB, R8, RG8, RGBA16F, …). For BCn textures (`dxt1` / `dxt5`)
//! the parser already extracts whatever levels are physically present in
//! the payload; if the on-disk chain is shorter than the header
//! advertises the gap is left for the application to handle (BCn is not
//! filterable on the GPU, so a linear blit cannot fill it in).
//!
//! One pipeline is cached per destination format, so adding mip
//! generation for a new format is just a matter of pointing the
//! generator at a texture in that format.

use std::{cell::RefCell, collections::HashMap};

use pkg_parser::pkg_parser::tex_parser::Tex;
use wgpu::*;

const MIPGEN_SHADER: &str = include_str!("./mipgen.wgsl");

/// Generates missing mip levels for a `wgpu::Texture` on the GPU.
///
/// Cheap to construct (one shared shader module + bind group layout +
/// sampler, lazily-created pipelines per format) and safe to share
/// across many textures in a frame.
pub struct MipChainGenerator {
    /// Lazily-created pipeline per destination format. Interior mutability
    /// lets [`MipChainGenerator::generate`] take `&self`, which is what
    /// the renderer wants (the generator is stored in `WgpuApp` and
    /// passed through `DrawQueue` / `DrawObject` / `upload_texture`).
    pipelines: RefCell<HashMap<TextureFormat, RenderPipeline>>,
    bind_group_layout: BindGroupLayout,
    sampler: Sampler,
}

impl MipChainGenerator {
    /// Create a new generator. The shared bind group layout and sampler
    /// are created immediately; per-format pipelines (and the shader
    /// module each one references) are built on first use.
    pub fn new(device: &Device) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("mipgen.bgl"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("mipgen.sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: MipmapFilterMode::Nearest,
            ..Default::default()
        });

        Self {
            pipelines: RefCell::new(HashMap::new()),
            bind_group_layout,
            sampler,
        }
    }

    /// Populate every missing mip level of `texture` on the GPU.
    ///
    /// Levels that the parser already wrote to `texture` (i.e. the first
    /// `tex.actual_mip_count()` levels) are left untouched. Missing
    /// levels are filled in starting at index `tex.actual_mip_count()` and
    /// continuing down to `tex.mipmap_count() - 1`. The command encoder
    /// receives the mipgen render passes; it is the caller's job to
    /// `finish()` and `submit()` it.
    ///
    /// Returns the number of mip levels that were generated. Returns `0`
    /// if there was nothing to do, or if the texture's format is not
    /// filterable (e.g. BCn) — a warning is logged in the latter case.
    pub fn generate(
        &self,
        device: &Device,
        encoder: &mut CommandEncoder,
        texture: &Texture,
        tex: &Tex,
    ) -> u32 {
        let total = tex.mipmap_count.max(1);
        let start = tex.actual_mip_count();
        if start >= total {
            return 0;
        }
        if !is_filterable(texture.format()) {
            log::warn!(
                "mipgen: format {:?} is not filterable on the GPU, \
                 skipping {} missing mip level(s) for {}x{} ({} actual / {} advertised)",
                texture.format(),
                total - start,
                tex.dimension[0],
                tex.dimension[1],
                start,
                total,
            );
            return 0;
        }

        // Get or create the pipeline for this destination format.
        let format = texture.format();
        if !self.pipelines.borrow().contains_key(&format) {
            let pipeline = self.build_pipeline(device, format);
            self.pipelines.borrow_mut().insert(format, pipeline);
        }
        // Use a scope to bound the RefCell borrow: we need `&RenderPipeline`
        // for `set_pipeline` below, but we don't want to hold the borrow
        // across the encode calls.
        let pipeline_ref = self.pipelines.borrow();
        let pipeline = pipeline_ref.get(&format).expect("just inserted");

        // Walk the source dimensions down to the mip just before the
        // first missing level, so each render pass samples from the
        // previous level at the correct size.
        let mut src_w = tex.dimension[0];
        let mut src_h = tex.dimension[1];
        for _ in 1..start {
            src_w = (src_w / 2).max(1);
            src_h = (src_h / 2).max(1);
        }

        let mut generated = 0u32;
        log::debug!(
            "mipgen: {}x{} fmt={:?} — generating {} mip level(s) (start={}, total={})",
            tex.dimension[0],
            tex.dimension[1],
            texture.format(),
            total - start,
            start,
            total,
        );
        for level in start..total {
            let dst_w = (src_w / 2).max(1);
            let dst_h = (src_h / 2).max(1);

            let src_view = texture.create_view(&TextureViewDescriptor {
                label: Some("mipgen.src"),
                format: None,
                dimension: Some(TextureViewDimension::D2),
                aspect: TextureAspect::All,
                base_mip_level: level - 1,
                mip_level_count: Some(1),
                base_array_layer: 0,
                array_layer_count: Some(1),
                usage: Some(TextureUsages::TEXTURE_BINDING),
            });
            let dst_view = texture.create_view(&TextureViewDescriptor {
                label: Some("mipgen.dst"),
                format: None,
                dimension: Some(TextureViewDimension::D2),
                aspect: TextureAspect::All,
                base_mip_level: level,
                mip_level_count: Some(1),
                base_array_layer: 0,
                array_layer_count: Some(1),
                usage: Some(TextureUsages::RENDER_ATTACHMENT),
            });

            let bind_group = device.create_bind_group(&BindGroupDescriptor {
                label: Some("mipgen.bg"),
                layout: &self.bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::TextureView(&src_view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::Sampler(&self.sampler),
                    },
                ],
            });

            let mut rpass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("mipgen.pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &dst_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        // Clear to transparent black: the destination mip
                        // is uninitialized memory, and we want a defined
                        // value in case the shader doesn't cover every
                        // pixel (it does, but be defensive).
                        load: LoadOp::Clear(Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rpass.set_pipeline(pipeline);
            rpass.set_bind_group(0, &bind_group, &[]);
            rpass.draw(0..3, 0..1);
            drop(rpass);

            src_w = dst_w;
            src_h = dst_h;
            generated += 1;
        }

        log::debug!(
            "mipgen: filled {} mip level(s) for {}x{} fmt={:?}",
            generated,
            tex.dimension[0],
            tex.dimension[1],
            texture.format(),
        );

        generated
    }

    fn build_pipeline(&self, device: &Device, format: TextureFormat) -> RenderPipeline {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("mipgen.shader"),
            source: ShaderSource::Wgsl(MIPGEN_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("mipgen.layout"),
            bind_group_layouts: &[&self.bind_group_layout],
            immediate_size: 0,
        });

        device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("mipgen.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }
}

/// Returns `true` when `format` supports linear filtering on the GPU, so
/// the mipgen shader can sample it. Block-compressed formats, depth/
/// stencil formats, and integer formats are excluded.
fn is_filterable(format: TextureFormat) -> bool {
    use TextureFormat as F;
    matches!(
        format,
        // 8-bit single/dual/quad channel
        F::R8Unorm
            | F::R8Snorm
            | F::R8Uint
            | F::R8Sint
            | F::Rg8Unorm
            | F::Rg8Snorm
            | F::Rg8Uint
            | F::Rg8Sint
            | F::Rgba8Unorm
            | F::Rgba8UnormSrgb
            | F::Bgra8Unorm
            | F::Bgra8UnormSrgb
            | F::Rgba8Uint
            | F::Rgba8Sint
            | F::Rgba8Snorm
            // 16-bit
            | F::R16Unorm
            | F::R16Snorm
            | F::R16Float
            | F::Rg16Unorm
            | F::Rg16Snorm
            | F::Rg16Float
            | F::Rgba16Unorm
            | F::Rgba16Snorm
            | F::Rgba16Float
            // 32-bit float
            | F::R32Float
            | F::Rg32Float
            | F::Rgba32Float
    )
}
