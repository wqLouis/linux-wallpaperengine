use std::{borrow::Cow, collections::BTreeMap, rc::Rc};

use serde_json::Value;
use wgpu::*;

use crate::scene::{
    loader::scene_loader::Scene,
    renderer::{
        post_processor::{
            effect_param::UniformLayout,
            pipeline_helpers,
            shader_header,
            transform::{EffectLayout, collect_layout, preprocess_pair},
        },
        vertex::Vertex,
    },
};

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
    device: &Device,
    effect_path: String,
    pass_textures: &[Option<String>],
    pass_combos: Option<&BTreeMap<String, i64>>,
    pipelines: &mut BTreeMap<String, EffectPipelineData>,
    scene: &Scene,
    projection_bgl: &BindGroupLayout,
    has_immediates: bool,
    has_subgroup: bool,
) -> Option<Rc<RenderPipeline>> {
    let cache_key = make_cache_key(&effect_path, pass_textures, pass_combos);
    if let Some(data) = pipelines.get(&cache_key) {
        return Some(Rc::clone(&data.pipeline));
    }

    let effect_json: Value = serde_json::from_str(&scene.jsons.get(&effect_path)?[..]).ok()?;
    let material_path = effect_json["passes"][0]["material"].as_str()?;
    let material_json: Value = serde_json::from_str(&scene.jsons.get(material_path)?[..]).ok()?;
    let shader_name = material_json["passes"][0]["shader"].as_str()?;

    let data = compile_pipeline(
        device,
        &format!("shaders/{}.frag", shader_name),
        &format!("shaders/{}.vert", shader_name),
        material_json,
        pass_textures,
        pass_combos,
        scene,
        projection_bgl,
        has_immediates,
        has_subgroup,
    )?;
    let rc = Rc::clone(&data.pipeline);
    pipelines.insert(cache_key, data);
    Some(rc)
}

/// Get or create a pipeline for a multi-pass effect step (given material + shader paths directly).
pub fn create_effect_pipeline_for_multipass(
    device: &Device,
    frag_path: &str,
    vert_path: &str,
    material_path: &str,
    pass_textures: &[Option<String>],
    pass_combos: Option<&BTreeMap<String, i64>>,
    pipelines: &mut BTreeMap<String, EffectPipelineData>,
    scene: &Scene,
    projection_bgl: &BindGroupLayout,
    has_immediates: bool,
    has_subgroup: bool,
) -> Option<Rc<RenderPipeline>> {
    let cache_key = make_cache_key(material_path, pass_textures, pass_combos);
    if let Some(data) = pipelines.get(&cache_key) {
        return Some(Rc::clone(&data.pipeline));
    }

    let material_json: Value = serde_json::from_str(&scene.jsons.get(material_path)?[..]).ok()?;
    let data = compile_pipeline(
        device, frag_path, vert_path, material_json,
        pass_textures, pass_combos, scene, projection_bgl,
        has_immediates, has_subgroup,
    )?;
    let rc = Rc::clone(&data.pipeline);
    pipelines.insert(cache_key, data);
    Some(rc)
}

// ── Internal ──────────────────────────────────────────────────

fn make_cache_key(
    base: &str,
    pass_textures: &[Option<String>],
    pass_combos: Option<&BTreeMap<String, i64>>,
) -> String {
    let mut key = base.to_string();
    if pass_textures.get(1).and_then(|t| t.as_deref()).is_some() {
        key.push_str("|M1");
    }
    if pass_textures.get(2).and_then(|t| t.as_deref()).is_some() {
        key.push_str("|T1");
    }
    if let Some(combos) = pass_combos {
        for (k, v) in combos {
            key.push_str(&format!("|{}={}", k, v));
        }
    }
    key
}

/// Shared pipeline compilation: material_json already loaded, shader paths resolved.
fn compile_pipeline(
    device: &Device,
    frag_path: &str,
    vert_path: &str,
    material_json: Value,
    pass_textures: &[Option<String>],
    pass_combos: Option<&BTreeMap<String, i64>>,
    scene: &Scene,
    projection_bgl: &BindGroupLayout,
    has_immediates: bool,
    has_subgroup: bool,
) -> Option<EffectPipelineData> {
    let frag_raw = &*scene.misc.get(frag_path)?;
    let vert_raw = &*scene.misc.get(vert_path)?;
    let frag_source = std::str::from_utf8(frag_raw).ok()?;
    let vert_source = std::str::from_utf8(vert_raw).ok()?;

    // Priority: shader defaults → material.json → scene pass
    let mut defines = pipeline_helpers::collect_default_defines(vert_source, frag_source);

    if let Some(mat_combos) = material_json["passes"][0].get("combos").and_then(|c| c.as_object()) {
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

    let define_refs: Vec<(&str, &str)> = defines.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let headers = shader_header::get_headers(&scene.misc);

    // First pass: collect layout to know uniform sizes.
    let layout_pre = collect_layout(vert_source, frag_source, &headers);
    let total_uniform_size = layout_pre.total_uniform_size();
    // Use immediates if available AND uniform block fits within immediate size limit.
    // Vulkan push constants are typically 128–256 bytes; wgpu reports via Limits::max_immediate_size.
    let max_immediate = if has_immediates { device.limits().max_immediate_size } else { 0 };
    let use_immediates = has_immediates && total_uniform_size > 0 && total_uniform_size <= max_immediate as u64;
    if use_immediates {
        log::info!(
            "Shader {}: using immediates for {} bytes of uniforms (limit={})",
            frag_path, total_uniform_size, max_immediate
        );
    }

    let (vert_processed, frag_processed, layout) =
        preprocess_pair(vert_source, frag_source, &headers, &defines, has_subgroup, use_immediates);

    let vert_module = device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Glsl { shader: Cow::Owned(vert_processed), stage: naga::ShaderStage::Vertex, defines: &define_refs },
    });
    let frag_module = device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Glsl { shader: Cow::Owned(frag_processed), stage: naga::ShaderStage::Fragment, defines: &define_refs },
    });

    let effect_bgl = pipeline_helpers::create_effect_bindgroup_layout(device, &layout);
    // immediate_size must exactly match the data passed to set_immediates().
    // Use the UniformLayout computed here (shared with EffectPipelineData below).
    let uniform_layout = UniformLayout::new(&layout.uniform_decls);
    let immediate_size = if use_immediates { uniform_layout.total_size() as u32 } else { 0 };
    let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[&effect_bgl, projection_bgl],
        immediate_size,
    });
    let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        vertex: VertexState { module: &vert_module, entry_point: Some("main"), compilation_options: Default::default(), buffers: &[Vertex::create_buffer_layout()] },
        primitive: PrimitiveState { topology: PrimitiveTopology::TriangleList, strip_index_format: None, front_face: FrontFace::Ccw, cull_mode: Some(Face::Back), unclipped_depth: false, polygon_mode: PolygonMode::Fill, conservative: false },
        depth_stencil: None,
        multisample: MultisampleState { count: 1, mask: !0, alpha_to_coverage_enabled: false },
        fragment: Some(FragmentState {
            module: &frag_module, entry_point: Some("main"), compilation_options: Default::default(),
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
        multiview_mask: None, cache: None,
    });

    Some(EffectPipelineData {
        pipeline: Rc::new(pipeline),
        compute_pipeline: None,
        uniform_layout,
        bindgroup_layout: effect_bgl,
        layout,
    })
}

// ── Texture loader ────────────────────────────────────────────

pub fn load_mask_texture(
    device: &Device,
    queue: &Queue,
    scene: &Scene,
    path: &str,
) -> Option<(Texture, TextureView)> {
    let tex_key = format!("materials/{}.tex", path);
    let tex = scene.textures.get(&tex_key)?;

    let (format, bytes_per_row) = match tex.extension.as_str() {
        "r8" => (TextureFormat::R8Unorm, tex.dimension[0] * 1),
        "rg88" => (TextureFormat::Rg8Unorm, tex.dimension[0] * 2),
        "dxt1" => (TextureFormat::Bc1RgbaUnormSrgb, tex.dimension[0].div_ceil(4) * 8),
        "dxt5" => (TextureFormat::Bc3RgbaUnormSrgb, tex.dimension[0].div_ceil(4) * 16),
        _ => (TextureFormat::Rgba8Unorm, tex.dimension[0] * 4),
    };

    let texture = device.create_texture(&TextureDescriptor {
        label: None,
        size: Extent3d { width: tex.dimension[0], height: tex.dimension[1], depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: TextureDimension::D2,
        format, usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST, view_formats: &[],
    });
    queue.write_texture(
        TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: Origin3d::ZERO, aspect: TextureAspect::All },
        &tex.payload,
        TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: None },
        Extent3d { width: tex.dimension[0], height: tex.dimension[1], depth_or_array_layers: 1 },
    );
    let view = texture.create_view(&Default::default());
    Some((texture, view))
}
