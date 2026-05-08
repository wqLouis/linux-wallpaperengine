use std::{borrow::Cow, collections::BTreeMap, rc::Rc};

use serde_json::Value;
use wgpu::*;

use crate::scene::{
    loader::scene_loader::Scene,
    renderer::{
        post_processor::{
            effect_param::UniformLayout,
            pipeline_helpers,
            shader_preprocessor::{EffectLayout, preprocess_pair},
        },
        vertex::Vertex,
    },
};

#[derive(Debug)]
pub struct EffectPipelineData {
    pub pipeline: Rc<RenderPipeline>,
    pub layout: EffectLayout,
    pub bindgroup_layout: BindGroupLayout,
    pub uniform_layout: UniformLayout,
}

pub fn get_or_create_pipeline(
    device: &Device,
    effect_path: String,
    pass_textures: &[Option<String>],
    pipelines: &mut BTreeMap<String, EffectPipelineData>,
    scene: &Scene,
    projection_bgl: &BindGroupLayout,
) -> Option<Rc<RenderPipeline>> {
    let cache_key = compute_cache_key(&effect_path, pass_textures);

    if let Some(data) = pipelines.get(&cache_key) {
        return Some(Rc::clone(&data.pipeline));
    }

    let data = create_effect_pipeline(device, &effect_path, pass_textures, scene, projection_bgl)?;
    let pipeline_rc = Rc::clone(&data.pipeline);
    pipelines.insert(cache_key, data);
    Some(pipeline_rc)
}

fn compute_cache_key(effect_path: &str, pass_textures: &[Option<String>]) -> String {
    let mut key = effect_path.to_string();
    // textures[1] = g_Texture1 (MASK combo), textures[2] = g_Texture2 (TIMEOFFSET)
    if pass_textures.get(1).and_then(|t| t.as_deref()).is_some() {
        key.push_str("|M1");
    }
    if pass_textures.get(2).and_then(|t| t.as_deref()).is_some() {
        key.push_str("|T1");
    }
    key
}

fn create_effect_pipeline(
    device: &Device,
    effect_path: &str,
    pass_textures: &[Option<String>],
    scene: &Scene,
    projection_bgl: &BindGroupLayout,
) -> Option<EffectPipelineData> {
    let effect_json: Value = serde_json::from_str(scene.jsons.get(effect_path)?).ok()?;

    let material_path = effect_json["passes"][0]["material"].as_str()?;

    let material_json: Value = serde_json::from_str(scene.jsons.get(material_path)?).ok()?;

    let shader_name = material_json["passes"][0]["shader"].as_str()?;

    let frag_path = format!("shaders/{}.frag", shader_name);
    let vert_path = format!("shaders/{}.vert", shader_name);

    let frag_raw = scene.misc.get(&frag_path)?;
    let vert_raw = scene.misc.get(&vert_path)?;

    let frag_source = std::str::from_utf8(frag_raw).ok()?;
    let vert_source = std::str::from_utf8(vert_raw).ok()?;

    let mut defines = pipeline_helpers::collect_default_defines(vert_source, frag_source);

    pipeline_helpers::apply_texture_combos(&mut defines, pass_textures);

    let define_refs: Vec<(&str, &str)> = defines
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let (vert_processed, frag_processed, layout) = preprocess_pair(vert_source, frag_source);

    let vert_module = device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Glsl {
            shader: Cow::Owned(vert_processed),
            stage: naga::ShaderStage::Vertex,
            defines: &define_refs,
        },
    });

    let frag_module = device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Glsl {
            shader: Cow::Owned(frag_processed),
            stage: naga::ShaderStage::Fragment,
            defines: &define_refs,
        },
    });

    let effect_bgl = pipeline_helpers::create_effect_bindgroup_layout(device, &layout);

    let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[&effect_bgl, projection_bgl],
        immediate_size: 0,
    });

    let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
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
                blend: Some(BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::SrcAlpha,
                        dst_factor: BlendFactor::OneMinusSrcAlpha,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent::OVER,
                }),
                write_mask: ColorWrites::all(),
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    let uniform_layout = UniformLayout::new(&layout.uniform_decls);

    Some(EffectPipelineData {
        pipeline: Rc::new(pipeline),
        layout,
        bindgroup_layout: effect_bgl,
        uniform_layout,
    })
}

pub fn load_mask_texture(
    device: &Device,
    queue: &Queue,
    scene: &Scene,
    path: &str,
) -> Option<(Texture, TextureView)> {
    // Try multiple path resolutions: the .pkg stores files under "materials/"
    // but JSON references may use relative paths like "masks/..."
    let candidates = [format!("{}.tex", path), format!("materials/{}.tex", path)];
    let tex = candidates.iter().find_map(|key| scene.textures.get(key));
    let tex = tex?;

    // Select GPU format based on the texture's native encoding.
    // R8/RG88 stay single/two-channel (not expanded to RGBA),
    // PNG/JPG/DXT are already RGBA from parse_to_rgba.
    let (format, bpp) = match tex.extension.as_str() {
        "r8" => (TextureFormat::R8Unorm, 1u32),
        "rg88" => (TextureFormat::Rg8Unorm, 2u32),
        _ => (TextureFormat::Rgba8Unorm, 4u32),
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
            bytes_per_row: Some(tex.dimension[0] * bpp),
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
