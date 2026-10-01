//! Shared context types for effect-pipeline construction.

use std::collections::BTreeMap;

use wgpu::{BindGroupLayout, Device, Queue, Texture, TextureView};

use crate::scene::{loader::scene_loader::Scene, renderer::post_process::PostProcess};

use super::pipeline_handler::EffectPipelineData;

/// Cache of compiled effect pipelines, keyed by effect/material + combos.
pub type PipelineMap = BTreeMap<String, EffectPipelineData>;

/// Immutable scene/GPU context shared by every effect-pipeline builder.
pub struct EffectContext<'a> {
    pub device: &'a Device,
    pub queue: &'a Queue,
    pub scene: &'a Scene,
    pub post_process: &'a PostProcess,
    pub projection_bgl: &'a BindGroupLayout,
    pub has_immediates: bool,
    pub has_subgroup: bool,
}

/// The source texture an effect chain reads from, plus its dimensions.
pub struct EffectTarget<'a> {
    pub view: &'a TextureView,
    pub width: u32,
    pub height: u32,
}

/// Texture inputs for a single effect bind group.
pub struct EffectTextures<'a> {
    pub source_view: &'a TextureView,
    pub mask_view: Option<&'a TextureView>,
    pub noise_view: Option<&'a TextureView>,
    pub mask_tex: Option<Texture>,
    pub noise_tex: Option<Texture>,
}
