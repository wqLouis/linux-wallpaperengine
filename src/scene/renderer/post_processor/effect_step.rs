//! Unified effect step model. Single-pass and multi-pass effects are
//! flattened into [`EffectStep`]s with named FBOs allocated per-effect.

use std::{collections::BTreeMap, rc::Rc};

use serde::Deserialize;
use wgpu::*;

use crate::scene::{
    loader::{
        object::Effect,
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

// ── Types ─────────────────────────────────────────────────────

pub struct EffectStep {
    pub pipeline: RenderPipeline,
    pub bindgroup: EffectBindGroup,
    pub pipedata: EffectPipelineData,
    pub bind_inputs: Vec<(String, u32)>,
    pub target: Option<String>,
}

pub struct FboTexture {
    #[allow(dead_code)] pub texture: Texture,
    pub view: TextureView,
}

// ── Effect JSON deserialization ───────────────────────────────

#[derive(Debug, Deserialize)] #[serde(rename_all = "camelCase")]
struct EffectDefPass { material: String, #[serde(default)] target: Option<String>, #[serde(default)] bind: Vec<EffectDefBind> }
#[derive(Debug, Deserialize)] struct EffectDefBind { name: String, index: u32 }
#[derive(Debug, Deserialize)] struct EffectDefFbo { name: String, #[serde(default)] scale: Option<f32> }
#[derive(Debug, Deserialize)] struct EffectDef { #[serde(default)] passes: Vec<EffectDefPass>, #[serde(default)] fbos: Vec<EffectDefFbo> }

// ── Public builder ────────────────────────────────────────────

pub fn build_effect_steps(
    device: &Device, queue: &Queue, effects: &[Effect], scene: &Scene,
    post_process: &PostProcess, pipelines: &mut BTreeMap<String, EffectPipelineData>,
    proj_bgl: &BindGroupLayout, source_view: &TextureView,
    source_w: u32, source_h: u32, no_effects: bool,
) -> (Vec<EffectStep>, BTreeMap<String, FboTexture>, bool) {
    if no_effects { return (vec![], BTreeMap::new(), false); }

    let mut steps = Vec::new();
    let mut fbos = BTreeMap::new();

    for effect in effects {
        let raw = match scene.jsons.get(&effect.file) { Some(r) => r, None => continue };
        let def: EffectDef = match serde_json::from_str(&raw[..]) { Ok(d) => d, Err(_) => continue };

        if def.passes.len() <= 1 && def.fbos.is_empty() {
            if let Some(s) = build_step(device, queue, effect, effect.passes.first(), None,
                                         scene, post_process, pipelines, proj_bgl, source_view, source_w, source_h) {
                steps.push(s);
            }
        } else {
            for fbo_def in &def.fbos {
                let s = fbo_def.scale.unwrap_or(1.0).max(1.0);
                let (w, h) = (((source_w as f32) / s).max(1.0) as u32, ((source_h as f32) / s).max(1.0) as u32);
                let tex = device.create_texture(&TextureDescriptor {
                    label: None, size: Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1, sample_count: 1, dimension: TextureDimension::D2,
                    format: TextureFormat::Rgba8UnormSrgb,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING, view_formats: &[],
                });
                let view = tex.create_view(&Default::default());
                fbos.insert(fbo_def.name.clone(), FboTexture { texture: tex, view });
            }
            for (i, def_pass) in def.passes.iter().enumerate() {
                if let Some(s) = build_step(device, queue, effect, effect.passes.get(i),
                                             Some(def_pass), scene, post_process, pipelines, proj_bgl,
                                             source_view, source_w, source_h) {
                    steps.push(s);
                }
            }
        }
    }
    let has_any = !steps.is_empty();
    (steps, fbos, has_any)
}

// ── Shared step builder ───────────────────────────────────────

/// Build one EffectStep. `def_pass` is Some for multi-pass internal steps.
fn build_step(
    device: &Device, queue: &Queue, effect: &Effect,
    scene_pass: Option<&crate::scene::loader::object::Pass>,
    def_pass: Option<&EffectDefPass>,
    scene: &Scene, post_process: &PostProcess,
    pipelines: &mut BTreeMap<String, EffectPipelineData>,
    proj_bgl: &BindGroupLayout, source_view: &TextureView,
    source_w: u32, source_h: u32,
) -> Option<EffectStep> {
    let scene_pass = scene_pass?;

    let (pipeline, pipedata, bind_inputs, target) = if let Some(dp) = def_pass {
        // Multi-pass step
        let mat_raw = scene.jsons.get(&dp.material)?;
        let mat_json: serde_json::Value = serde_json::from_str(&mat_raw[..]).ok()?;
        let shader = mat_json["passes"][0]["shader"].as_str()?;
        let p = pipeline_handler::create_effect_pipeline_for_multipass(
            device, &format!("shaders/{}.frag", shader), &format!("shaders/{}.vert", shader),
            &dp.material, &scene_pass.textures, scene_pass.combos.as_ref(),
            pipelines, scene, proj_bgl,
        )?;
        let pd = pipelines.values().find(|d| Rc::ptr_eq(&d.pipeline, &p))?.clone();
        let bi: Vec<(String, u32)> = dp.bind.iter().map(|b| (b.name.clone(), b.index)).collect();
        (p.as_ref().clone(), pd, bi, dp.target.clone())
    } else {
        // Single-pass
        let p = pipeline_handler::get_or_create_pipeline(
            device, effect.file.clone(), &scene_pass.textures, scene_pass.combos.as_ref(),
            pipelines, scene, proj_bgl,
        )?;
        let pd = pipelines.values().find(|d| Rc::ptr_eq(&d.pipeline, &p))?.clone();
        (p.as_ref().clone(), pd, vec![("previous".to_string(), 0)], None)
    };

    let (mask_tex, mask_view, noise_tex, noise_view) = load_mask_and_noise(device, queue, scene, scene_pass);
    let bindgroup = EffectBindGroup::new(
        device, post_process, &pipedata, source_view,
        mask_view.as_ref(), noise_view.as_ref(), Rc::clone(&pipedata.pipeline),
        pipedata.layout.uniform_material_keys.clone(),
        scene_pass.constantshadervalues.clone().unwrap_or_default(),
        build_tex_resolutions(&pipedata, source_w, source_h, mask_tex.as_ref(), noise_tex.as_ref()),
        mask_tex, noise_tex,
    )?;

    Some(EffectStep { pipeline, bindgroup, pipedata, bind_inputs, target })
}

fn load_mask_and_noise(device: &Device, queue: &Queue, scene: &Scene,
    pass: &crate::scene::loader::object::Pass) -> (Option<Texture>, Option<TextureView>, Option<Texture>, Option<TextureView>)
{
    let load = |p: &str| load_mask_texture(device, queue, scene, p).map(|(t, v)| (Some(t), Some(v))).unwrap_or((None, None));
    let (mt, mv) = pass.textures.get(1).and_then(|t| t.as_deref()).map_or((None, None), load);
    let (nt, nv) = pass.textures.get(2).and_then(|t| t.as_deref()).map_or((None, None), load);
    (mt, mv, nt, nv)
}

fn build_tex_resolutions(pipedata: &EffectPipelineData, sw: u32, sh: u32,
    mask: Option<&Texture>, noise: Option<&Texture>) -> BTreeMap<String, [f32; 4]>
{
    let (sw, sh) = (sw as f32, sh as f32);
    pipedata.layout.sampler_names.iter().enumerate().map(|(i, name)| {
        let (w, h) = match i {
            1 => mask.map(|t| (t.width() as f32, t.height() as f32)).unwrap_or((sw, sh)),
            2 => noise.map(|t| (t.width() as f32, t.height() as f32)).unwrap_or((sw, sh)),
            _ => (sw, sh),
        };
        (format!("{}Resolution", name), [w, h, w, h])
    }).collect()
}

// ── Bindgroup builder ─────────────────────────────────────────

pub fn make_step_bindgroup(
    device: &Device, step: &EffectStep, source_view: &TextureView,
    fbos: &BTreeMap<String, FboTexture>, sampler: &Sampler,
) -> BindGroup {
    let n = step.pipedata.layout.sampler_count();
    let mut entries: Vec<BindGroupEntry<'_>> = Vec::with_capacity(n + 2);
    for slot in 0..n {
        let view = resolve_texture(slot as u32, step, source_view, fbos);
        entries.push(BindGroupEntry { binding: slot as u32 * 2, resource: BindingResource::TextureView(view) });
    }
    entries.push(BindGroupEntry { binding: WM_SAMPLER_BINDING, resource: BindingResource::Sampler(sampler) });
    if let Some(ref buf) = step.bindgroup.uniform_buffer {
        entries.push(BindGroupEntry { binding: step.pipedata.layout.uniform_binding, resource: buf.as_entire_binding() });
    }
    device.create_bind_group(&BindGroupDescriptor { label: None, layout: &step.pipedata.bindgroup_layout, entries: &entries })
}

fn resolve_texture<'a>(slot: u32, step: &'a EffectStep, source_view: &'a TextureView,
                        fbos: &'a BTreeMap<String, FboTexture>) -> &'a TextureView {
    if let Some((name, _)) = step.bind_inputs.iter().find(|(_, i)| *i == slot) {
        if name == "previous" { return source_view; }
        if let Some(fbo) = fbos.get(name) { return &fbo.view; }
    }
    match slot {
        0 => source_view,
        1 => step.bindgroup.mask_view.as_ref().unwrap_or(&step.bindgroup.blank_view),
        2 => step.bindgroup.noise_view.as_ref().unwrap_or(&step.bindgroup.blank_view),
        _ => &step.bindgroup.blank_view,
    }
}
