//! Unified effect step model.
//!
//! Both single-pass effects (e.g. tint, foliagesway) and multi-pass effects
//! (e.g. shine, blur_precise) are flattened into a single ordered list of
//! [`EffectStep`]s. Named FBOs used by multi-pass effect chains are allocated
//! as [`FboTexture`]s. A single render loop in `intermediate_pass` processes
//! every step, using ping-pong views for steps without a target and named
//! FBOs for steps that target one.

use std::{collections::BTreeMap, rc::Rc};

use serde::Deserialize;
use wgpu::*;

use crate::scene::{
    loader::{
        object::{Effect},
        scene_loader::Scene,
    },
    renderer::{
        effect_bindgroup::EffectBindGroup,
        post_process::PostProcess,
        post_processor::{
            pipeline_handler::{self, EffectPipelineData, load_mask_texture},
            shader_header::WM_SAMPLER_BINDING,
        },
    },
};

// ── Unified step ──────────────────────────────────────────────

/// One render step in an effect chain. Can come from a single-pass effect
/// (1 step) or a multi-pass effect (N steps).
pub struct EffectStep {
    pub pipeline: RenderPipeline,
    pub bindgroup: EffectBindGroup,
    pub pipedata: EffectPipelineData,
    /// Maps sampler slot → logical source name.
    /// "previous" = the current ping-pong source.
    /// Other names = named FBOs allocated alongside the steps.
    pub bind_inputs: Vec<(String, u32)>,
    /// Where to render. `None` = ping-pong target (view_a / view_b).
    /// `Some(name)` = named FBO texture.
    pub target: Option<String>,
}

/// A named intermediate render target (FBO).
pub struct FboTexture {
    #[allow(dead_code)]
    pub texture: Texture,
    pub view: TextureView,
}

// ── Effect definition parsing ─────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EffectDefPass {
    material: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    bind: Vec<EffectDefBind>,
}

#[derive(Debug, Deserialize)]
struct EffectDefBind {
    name: String,
    index: u32,
}

#[derive(Debug, Deserialize)]
struct EffectDefFbo {
    name: String,
    #[serde(default)]
    scale: Option<f32>,
}

/// Minimal effect.json structure needed for step extraction.
#[derive(Debug, Deserialize)]
struct EffectDef {
    #[serde(default)]
    passes: Vec<EffectDefPass>,
    #[serde(default)]
    fbos: Vec<EffectDefFbo>,
}

// ── Public API ────────────────────────────────────────────────

/// Build a flat list of [`EffectStep`]s from an object's effects,
/// and allocate any named FBOs required by multi-pass effects.
///
/// Returns `(steps, fbos, has_any)`. `has_any` is true when there is
/// at least one step (so ping-pong intermediates are needed).
pub fn build_effect_steps(
    device: &Device,
    queue: &Queue,
    effects: &[Effect],
    scene: &Scene,
    post_process: &PostProcess,
    pipelines: &mut BTreeMap<String, EffectPipelineData>,
    projection_bgl: &BindGroupLayout,
    source_view: &TextureView,
    source_w: u32,
    source_h: u32,
    no_effects: bool,
) -> (Vec<EffectStep>, BTreeMap<String, FboTexture>, bool) {
    if no_effects {
        return (Vec::new(), BTreeMap::new(), false);
    }

    let mut steps = Vec::new();
    let mut fbos = BTreeMap::new();

    for effect in effects {
        let raw = match scene.jsons.get(&effect.file) {
            Some(r) => r,
            None => continue,
        };
        let def: EffectDef = match serde_json::from_str(&raw[..]) {
            Ok(d) => d,
            Err(_) => continue,
        };

        if def.passes.len() <= 1 && def.fbos.is_empty() {
            // ── Single-pass ────────────────────────────
            let step = build_single_step(
                device, queue, effect, scene, post_process, pipelines,
                projection_bgl, source_view, source_w, source_h,
            );
            if let Some(s) = step {
                steps.push(s);
            }
        } else {
            // ── Multi-pass ─────────────────────────────
            let (mut mp_steps, mp_fbos) = build_multipass_steps(
                device, queue, effect, &def, scene, post_process, pipelines,
                projection_bgl, source_view, source_w, source_h,
            );
            steps.append(&mut mp_steps);
            fbos.extend(mp_fbos);
        }
    }

    let has_any = !steps.is_empty();
    (steps, fbos, has_any)
}

// ── Single-pass step builder ──────────────────────────────────

fn build_single_step(
    device: &Device,
    queue: &Queue,
    effect: &Effect,
    scene: &Scene,
    post_process: &PostProcess,
    pipelines: &mut BTreeMap<String, EffectPipelineData>,
    projection_bgl: &BindGroupLayout,
    source_view: &TextureView,
    source_w: u32,
    source_h: u32,
) -> Option<EffectStep> {
    let pass = effect.passes.first()?;

    let pipeline = pipeline_handler::get_or_create_pipeline(
        device,
        effect.file.clone(),
        &pass.textures,
        pass.combos.as_ref(),
        pipelines,
        scene,
        projection_bgl,
    )?;

    let pipedata = pipelines.values().find(|d| Rc::ptr_eq(&d.pipeline, &pipeline))?.clone();

    let mask_path = pass.textures.get(1).and_then(|t| t.as_deref());
    let noise_path = pass.textures.get(2).and_then(|t| t.as_deref());
    let load_tex = |p: &str| {
        load_mask_texture(device, queue, scene, p)
            .map(|(t, v)| (Some(t), Some(v)))
            .unwrap_or((None, None))
    };
    let (mask_tex, mask_view) = mask_path.map_or((None, None), load_tex);
    let (noise_tex, noise_view) = noise_path.map_or((None, None), load_tex);

    let constants = pass.constantshadervalues.clone().unwrap_or_default();
    let material_keys = pipedata.layout.uniform_material_keys.clone();

    let tex_resolutions = build_tex_resolutions(
        &pipedata, source_w, source_h,
        mask_tex.as_ref(), noise_tex.as_ref(),
    );

    let bindgroup = EffectBindGroup::new(
        device, post_process, &pipedata, source_view,
        mask_view.as_ref(), noise_view.as_ref(),
        Rc::clone(&pipedata.pipeline),
        material_keys, constants, tex_resolutions,
        mask_tex, noise_tex,
    )?;

    let bind_inputs = vec![("previous".to_string(), 0u32)];

    Some(EffectStep {
        pipeline: pipeline.as_ref().clone(),
        bindgroup,
        pipedata,
        bind_inputs,
        target: None, // single-pass → ping-pong
    })
}

// ── Multi-pass step builder ───────────────────────────────────

fn build_multipass_steps(
    device: &Device,
    queue: &Queue,
    effect: &Effect,
    def: &EffectDef,
    scene: &Scene,
    post_process: &PostProcess,
    pipelines: &mut BTreeMap<String, EffectPipelineData>,
    projection_bgl: &BindGroupLayout,
    source_view: &TextureView,
    source_w: u32,
    source_h: u32,
) -> (Vec<EffectStep>, BTreeMap<String, FboTexture>) {
    let mut steps = Vec::new();
    let mut fbos = BTreeMap::new();

    // Allocate FBOs
    for fbo_def in &def.fbos {
        let scale = fbo_def.scale.unwrap_or(1.0).max(1.0);
        let w = ((source_w as f32) / scale).max(1.0) as u32;
        let h = ((source_h as f32) / scale).max(1.0) as u32;
        let tex = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        fbos.insert(fbo_def.name.clone(), FboTexture { texture: tex, view });
    }

    for (i, def_pass) in def.passes.iter().enumerate() {
        let scene_pass = effect.passes.get(i);

        let pass_textures: Vec<Option<String>> = scene_pass
            .map(|p| p.textures.clone()).unwrap_or_default();
        let pass_combos = scene_pass.and_then(|p| p.combos.as_ref());
        let constants = scene_pass
            .and_then(|p| p.constantshadervalues.clone()).unwrap_or_default();

        // Resolve material → shader
        let material_raw = match scene.jsons.get(&def_pass.material) {
            Some(r) => r,
            None => continue,
        };
        let material_json: serde_json::Value = match serde_json::from_str(&material_raw[..]) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let shader_name = match material_json["passes"][0]["shader"].as_str() {
            Some(s) => s,
            None => continue,
        };
        let frag_path = format!("shaders/{}.frag", shader_name);
        let vert_path = format!("shaders/{}.vert", shader_name);

        let pipeline = match pipeline_handler::create_effect_pipeline_for_multipass(
            device, &frag_path, &vert_path, &def_pass.material,
            &pass_textures, pass_combos, pipelines, scene, projection_bgl,
        ) {
            Some(p) => p,
            None => continue,
        };

        let pipedata = match pipelines.values().find(|d| Rc::ptr_eq(&d.pipeline, &pipeline)) {
            Some(d) => d.clone(),
            None => continue,
        };

        let mask_path = pass_textures.get(1).and_then(|t| t.as_deref());
        let noise_path = pass_textures.get(2).and_then(|t| t.as_deref());
        let load_tex = |p: &str| {
            load_mask_texture(device, queue, scene, p)
                .map(|(t, v)| (Some(t), Some(v)))
                .unwrap_or((None, None))
        };
        let (mask_tex, mask_view) = mask_path.map_or((None, None), load_tex);
        let (noise_tex, noise_view) = noise_path.map_or((None, None), load_tex);

        let material_keys = pipedata.layout.uniform_material_keys.clone();
        let tex_resolutions = build_tex_resolutions(
            &pipedata, source_w, source_h,
            mask_tex.as_ref(), noise_tex.as_ref(),
        );

        let bindgroup = match EffectBindGroup::new(
            device, post_process, &pipedata, source_view,
            mask_view.as_ref(), noise_view.as_ref(),
            Rc::clone(&pipedata.pipeline),
            material_keys, constants, tex_resolutions,
            mask_tex, noise_tex,
        ) {
            Some(bg) => bg,
            None => continue,
        };

        let bind_inputs: Vec<(String, u32)> = def_pass.bind.iter()
            .map(|b| (b.name.clone(), b.index))
            .collect();

        steps.push(EffectStep {
            pipeline: pipeline.as_ref().clone(),
            bindgroup,
            pipedata,
            bind_inputs,
            target: def_pass.target.clone(),
        });
    }

    (steps, fbos)
}

// ── Helpers ───────────────────────────────────────────────────

fn build_tex_resolutions(
    pipedata: &EffectPipelineData,
    source_w: u32,
    source_h: u32,
    mask_tex: Option<&Texture>,
    noise_tex: Option<&Texture>,
) -> BTreeMap<String, [f32; 4]> {
    let sw = source_w as f32;
    let sh = source_h as f32;
    let mut map = BTreeMap::new();
    for (i, name) in pipedata.layout.sampler_names.iter().enumerate() {
        let key = format!("{}Resolution", name);
        let (w, h) = match i {
            0 => (sw, sh),
            1 => mask_tex.map(|t| (t.width() as f32, t.height() as f32)).unwrap_or((sw, sh)),
            2 => noise_tex.map(|t| (t.width() as f32, t.height() as f32)).unwrap_or((sw, sh)),
            _ => (sw, sh),
        };
        map.insert(key, [w, h, w, h]);
    }
    map
}

// ── Unified intermediate bindgroup builder ────────────────────

/// Build a bindgroup for a step, replacing texture views per `bind_inputs`.
///
/// * `source_view` — the current ping-pong source (view_a or view_b).
/// * `fbos` — named FBOs for multi-pass steps.
/// * `sampler` — shared sampler.
pub fn make_step_bindgroup(
    device: &Device,
    step: &EffectStep,
    source_view: &TextureView,
    fbos: &BTreeMap<String, FboTexture>,
    sampler: &Sampler,
) -> BindGroup {
    let n = step.pipedata.layout.sampler_count();
    let mut entries: Vec<BindGroupEntry<'_>> = Vec::with_capacity(n + 2);

    for slot in 0..n {
        let view = resolve_texture(slot as u32, step, source_view, fbos);
        entries.push(BindGroupEntry {
            binding: slot as u32 * 2,
            resource: BindingResource::TextureView(view),
        });
    }

    entries.push(BindGroupEntry {
        binding: WM_SAMPLER_BINDING,
        resource: BindingResource::Sampler(sampler),
    });

    if let Some(ref buf) = step.bindgroup.uniform_buffer {
        entries.push(BindGroupEntry {
            binding: step.pipedata.layout.uniform_binding,
            resource: buf.as_entire_binding(),
        });
    }

    device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &step.pipedata.bindgroup_layout,
        entries: &entries,
    })
}

/// Decide which TextureView feeds a given sampler slot.
fn resolve_texture<'a>(
    slot: u32,
    step: &'a EffectStep,
    source_view: &'a TextureView,
    fbos: &'a BTreeMap<String, FboTexture>,
) -> &'a TextureView {
    // Check bind_inputs override
    if let Some((name, _)) = step.bind_inputs.iter().find(|(_, i)| *i == slot) {
        if name == "previous" {
            return source_view;
        }
        if let Some(fbo) = fbos.get(name) {
            return &fbo.view;
        }
    }

    // Fall back to EffectBindGroup defaults
    match slot {
        0 => source_view,
        1 => step.bindgroup.mask_view.as_ref().unwrap_or(&step.bindgroup.blank_view),
        2 => step.bindgroup.noise_view.as_ref().unwrap_or(&step.bindgroup.blank_view),
        _ => &step.bindgroup.blank_view,
    }
}
