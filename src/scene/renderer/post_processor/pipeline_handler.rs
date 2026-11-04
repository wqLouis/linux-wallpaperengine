use std::{borrow::Cow, rc::Rc};

use serde_json::Value;
use wgpu::*;

use crate::scene::{
    loader::{
        scene_loader::Scene,
        shader::{EffectLayout, preprocess_pair_with_layout},
    },
    renderer::{
        error_scope,
        post_processor::{effect_param::UniformLayout, pipeline_helpers},
        vertex::Vertex,
    },
};

use super::context::{EffectContext, PipelineMap};
use crate::collections::{Map, sorted_entries};

#[derive(Debug, Clone)]
pub struct EffectPipelineData {
    pub pipeline: Rc<RenderPipeline>,
    /// Optional compute pipeline for effects that can run as compute shaders
    /// (e.g., blur, bloom downsampling). None for traditional fragment-shader effects.
    #[allow(dead_code)]
    pub compute_pipeline: Option<Rc<ComputePipeline>>,
    pub layout: EffectLayout,
    pub bindgroup_layout: BindGroupLayout,
    pub uniform_layout: UniformLayout,
}

// ── Public API ────────────────────────────────────────────────

/// Get or create a pipeline for a single-pass effect.
pub fn get_or_create_pipeline(
    ctx: &EffectContext,
    effect_path: String,
    pass_textures: &[Option<String>],
    pass_combos: Option<&Map<String, i64>>,
    pipelines: &mut PipelineMap,
) -> Option<Rc<RenderPipeline>> {
    let cache_key = make_cache_key(&effect_path, pass_textures, pass_combos);
    if let Some(data) = pipelines.get(&cache_key) {
        return Some(Rc::clone(&data.pipeline));
    }

    let effect_json: Value =
        serde_json::from_str(&ctx.scene.assets.json(&effect_path)?[..]).ok()?;
    let material_path = effect_json["passes"][0]["material"].as_str()?;
    let material_json: Value =
        serde_json::from_str(&ctx.scene.assets.json(material_path)?[..]).ok()?;
    let shader_name = material_json["passes"][0]["shader"].as_str()?;

    let data = compile_pipeline(
        ctx,
        &effect_path,
        &format!("shaders/{}.frag", shader_name),
        &format!("shaders/{}.vert", shader_name),
        material_json,
        pass_textures,
        pass_combos,
    )?;
    let rc = Rc::clone(&data.pipeline);
    pipelines.insert(cache_key, data);
    Some(rc)
}

/// Get or create a pipeline for a multi-pass effect step (given material + shader paths directly).
pub fn create_effect_pipeline_for_multipass(
    ctx: &EffectContext,
    frag_path: &str,
    vert_path: &str,
    material_path: &str,
    pass_textures: &[Option<String>],
    pass_combos: Option<&Map<String, i64>>,
    pipelines: &mut PipelineMap,
) -> Option<Rc<RenderPipeline>> {
    let cache_key = make_cache_key(material_path, pass_textures, pass_combos);
    if let Some(data) = pipelines.get(&cache_key) {
        return Some(Rc::clone(&data.pipeline));
    }

    let material_json: Value =
        serde_json::from_str(&ctx.scene.assets.json(material_path)?[..]).ok()?;
    let data = compile_pipeline(
        ctx,
        material_path,
        frag_path,
        vert_path,
        material_json,
        pass_textures,
        pass_combos,
    )?;
    let rc = Rc::clone(&data.pipeline);
    pipelines.insert(cache_key, data);
    Some(rc)
}

// ── Internal ──────────────────────────────────────────────────

fn make_cache_key(
    base: &str,
    pass_textures: &[Option<String>],
    pass_combos: Option<&Map<String, i64>>,
) -> String {
    let mut key = base.to_string();
    if pass_textures.get(1).and_then(|t| t.as_deref()).is_some() {
        key.push_str("|M1");
    }
    if pass_textures.get(2).and_then(|t| t.as_deref()).is_some() {
        key.push_str("|T1");
    }
    if let Some(combos) = pass_combos {
        // Sorted so the cache key only depends on the combo *set*, not on
        // the map's iteration order.
        for (k, v) in sorted_entries(combos) {
            key.push_str(&format!("|{}={}", k, v));
        }
    }
    key
}

/// Shared pipeline compilation: material_json already loaded, shader paths resolved.
///
/// `effect_id` names the effect (its json file or material) and is only
/// used to report failures.
fn compile_pipeline(
    ctx: &EffectContext,
    effect_id: &str,
    frag_path: &str,
    vert_path: &str,
    material_json: Value,
    pass_textures: &[Option<String>],
    pass_combos: Option<&Map<String, i64>>,
) -> Option<EffectPipelineData> {
    let program = ctx.scene.assets.shader(frag_path, vert_path)?;
    let frag_source = &program.fragment;
    let vert_source = &program.vertex;

    // Priority: shader defaults → material.json → scene pass
    let mut defines = program.default_defines.clone();

    if let Some(mat_combos) = material_json["passes"][0]
        .get("combos")
        .and_then(|c| c.as_object())
    {
        for (k, v) in mat_combos {
            if let Some(n) = v.as_i64() {
                defines.insert(k.clone(), n.to_string());
            }
        }
    }
    if let Some(combos) = pass_combos {
        for (k, v) in combos {
            defines.insert(k.clone(), v.to_string());
        }
    }
    pipeline_helpers::apply_texture_combos(&mut defines, pass_textures);

    let define_refs: Vec<(&str, &str)> = sorted_entries(&defines)
        .into_iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let headers = &program.headers;

    // The layout parsed at load time, with the path of the shaders: the
    // warnings of the preprocessing name the shader they are about.
    let mut layout_pre = program.layout.clone();
    layout_pre.shader_path = vert_path.trim_end_matches(".vert").to_string();
    // The immediates are the push constants of the effect.  They are only
    // used when the block of the uniforms fits into the limit of the
    // device (Vulkan push constants are typically 128-256 bytes; wgpu
    // reports the limit as Limits::max_immediate_size).  The decision is
    // made by the shader module, with the layout that is really compiled:
    // the layout of the load time can be a different size, because the
    // macros of the material select the branch that is compiled.
    let max_immediate = if ctx.has_immediates {
        ctx.device.limits().max_immediate_size
    } else {
        0
    };

    // Merge the shader source macros with the runtime combo defines
    // (combo defines take priority).
    let mut merged_defines = program.source_defines.clone();
    for (k, v) in &defines {
        merged_defines.insert(k.clone(), v.clone());
    }

    let (vert_processed, frag_processed, layout) = preprocess_pair_with_layout(
        vert_source,
        frag_source,
        headers,
        &merged_defines,
        &layout_pre,
        ctx.has_subgroup,
        max_immediate,
    );
    if layout.use_immediates {
        log::info!(
            "Shader {}: using immediates for {} bytes of uniforms (limit={})",
            frag_path,
            layout.total_uniform_size(),
            max_immediate
        );
    }

    // A broken shader - or a pipeline that cannot be built from it - must
    // not take the whole application down.  Build everything inside a
    // validation error scope so the error is captured here, log it and
    // skip this effect: the rest of the wallpaper keeps running.
    let vert_label = format!("shader {vert_path}");
    let frag_label = format!("shader {frag_path}");

    let built = error_scope::validation_scope(ctx.device, || {
        let vert_module = ctx.device.create_shader_module(ShaderModuleDescriptor {
            label: Some(&vert_label),
            source: ShaderSource::Glsl {
                shader: Cow::Owned(vert_processed),
                stage: naga::ShaderStage::Vertex,
                defines: &define_refs,
            },
        });
        let frag_module = ctx.device.create_shader_module(ShaderModuleDescriptor {
            label: Some(&frag_label),
            source: ShaderSource::Glsl {
                shader: Cow::Owned(frag_processed),
                stage: naga::ShaderStage::Fragment,
                defines: &define_refs,
            },
        });

        let effect_bgl = pipeline_helpers::create_effect_bindgroup_layout(ctx.device, &layout);
        // immediate_size must exactly match the data passed to set_immediates().
        // Use the UniformLayout computed here (shared with EffectPipelineData below).
        let uniform_layout = UniformLayout::new(&layout.uniform_decls);
        let immediate_size = if layout.use_immediates {
            uniform_layout.total_size() as u32
        } else {
            0
        };
        let pipeline_layout = ctx
            .device
            .create_pipeline_layout(&PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&effect_bgl, ctx.projection_bgl],
                immediate_size,
            });
        let pipeline = ctx
            .device
            .create_render_pipeline(&RenderPipelineDescriptor {
                label: None,
                layout: Some(&pipeline_layout),
                vertex: VertexState {
                    module: &vert_module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    buffers: &[Vertex::create_buffer_layout()],
                },
                primitive: PrimitiveState {
                    topology: PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: FrontFace::Ccw,
                    cull_mode: Some(Face::Back),
                    unclipped_depth: false,
                    polygon_mode: PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(FragmentState {
                    module: &frag_module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    targets: &[Some(ColorTargetState {
                        format: TextureFormat::Rgba8UnormSrgb,
                        // Additive blend (One / One) over the cleared destination.
                        // The destination is cleared to (0,0,0,0) at the start of
                        // each effect step, so `src + 0 == src` — the effect's
                        // output is stored as-is (not premultiplied). The final
                        // pass can then composite it with normal straight-alpha
                        // blending without the double-premultiplication artifact
                        // that produced a dark gray shade around the object.
                        blend: Some(BlendState {
                            color: BlendComponent {
                                src_factor: BlendFactor::One,
                                dst_factor: BlendFactor::One,
                                operation: BlendOperation::Add,
                            },
                            alpha: BlendComponent {
                                src_factor: BlendFactor::One,
                                dst_factor: BlendFactor::One,
                                operation: BlendOperation::Add,
                            },
                        }),
                        write_mask: ColorWrites::all(),
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });

        EffectPipelineData {
            pipeline: Rc::new(pipeline),
            compute_pipeline: None,
            uniform_layout,
            bindgroup_layout: effect_bgl,
            layout,
        }
    });

    match built {
        Ok(data) => Some(data),
        Err(err) => {
            log::error!(
                "effect '{effect_id}' could not be compiled (shader '{vert_path}' / '{frag_path}'): {err}"
            );
            log::warn!("effect '{effect_id}' will be skipped");
            None
        }
    }
}

// ── Texture loader ────────────────────────────────────────────

pub fn load_mask_texture(
    device: &Device,
    queue: &Queue,
    scene: &Scene,
    path: &str,
) -> Option<(Texture, TextureView)> {
    let tex_key = format!("materials/{}.tex", path);
    let tex = scene.assets.texture(&tex_key)?;

    let (format, bytes_per_row) = match tex.extension.as_str() {
        "r8" => (TextureFormat::R8Unorm, tex.dimension[0]),
        "rg88" => (TextureFormat::Rg8Unorm, tex.dimension[0] * 2),
        "dxt1" => (
            TextureFormat::Bc1RgbaUnormSrgb,
            tex.dimension[0].div_ceil(4) * 8,
        ),
        "dxt5" => (
            TextureFormat::Bc3RgbaUnormSrgb,
            tex.dimension[0].div_ceil(4) * 16,
        ),
        _ => (TextureFormat::Rgba8Unorm, tex.dimension[0] * 4),
    };

    let texture = device.create_texture(&TextureDescriptor {
        label: None,
        size: Extent3d {
            width: tex.dimension[0],
            height: tex.dimension[1],
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
        &tex.payload,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: None,
        },
        Extent3d {
            width: tex.dimension[0],
            height: tex.dimension[1],
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&Default::default());
    Some((texture, view))
}
