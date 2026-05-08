//! Draw queue construction: batches scene objects into GPU draw calls.
//!
//! Each [`TextureObject`] from the scene loader is converted into a
//! [`DrawObject`] with its texture bind group, effect bind groups, and
//! optional ping-pong intermediate textures for post-processing.

use std::{collections::BTreeMap, rc::Rc};

use wgpu::*;

use crate::scene::{
    loader::{object_loader::TextureObject, scene_loader::Scene},
    renderer::{
        buffer::Buffers,
        effect_bindgroup::EffectBindGroup,
        ping_pong::PingPongTextures,
        post_process::PostProcess,
        post_processor::pipeline_handler::{self, get_or_create_pipeline, load_mask_texture},
    },
};

pub struct DrawObject {
    // Owns the source TextureObject (kept alive for the struct's lifetime)
    #[allow(dead_code)]
    pub texture_object: TextureObject,
    pub index_range: [u32; 2],
    pub bindgroup: BindGroup,
    // Rc handles keeping effect pipelines alive; pipelines accessed via effect_bindgroups
    #[allow(dead_code)]
    pub pipelines: Vec<Rc<RenderPipeline>>,
    pub effect_bindgroups: Vec<EffectBindGroup>,
    pub intermediates: Option<PingPongTextures>,
}

pub struct DrawQueue {
    pub queue: Rc<Vec<DrawObject>>,
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

        let pipeline_rcs: Vec<Rc<RenderPipeline>> = if no_effects {
            Vec::new()
        } else {
            texture_object
                .effects
                .iter()
                .filter_map(|effect| {
                    let pass = effect.passes.first()?;
                    get_or_create_pipeline(
                        device,
                        effect.file.clone(),
                        &pass.textures,
                        pipelines,
                        scene,
                        projection_bgl,
                    )
                })
                .collect()
        };

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

        let effect_bindgroups = Self::build_effect_bindgroups(
            device,
            queue,
            scene,
            post_process,
            pipelines,
            &texture_object,
            &pipeline_rcs,
            &source_view,
        );

        let intermediates = if !effect_bindgroups.is_empty() {
            Some(PingPongTextures::new(
                device,
                queue,
                post_process,
                texture_object.texture.dimension[0],
                texture_object.texture.dimension[1],
            ))
        } else {
            None
        };

        buffers.draw_texture(
            queue,
            texture_object.origin,
            texture_object.angles,
            texture_object.scale,
            texture_object.size,
        );

        Self {
            texture_object,
            index_range: [index_start, buffers.index_len],
            bindgroup,
            pipelines: pipeline_rcs,
            effect_bindgroups,
            intermediates,
        }
    }

    fn upload_texture(device: &Device, queue: &Queue, tex_obj: &TextureObject) -> Texture {
        let texture = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: tex_obj.texture.dimension[0],
                height: tex_obj.texture.dimension[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
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
                bytes_per_row: Some(tex_obj.texture.dimension[0] * 4),
                rows_per_image: None,
            },
            Extent3d {
                width: tex_obj.texture.dimension[0],
                height: tex_obj.texture.dimension[1],
                depth_or_array_layers: 1,
            },
        );

        texture
    }

    fn build_effect_bindgroups(
        device: &Device,
        queue: &Queue,
        scene: &Scene,
        post_process: &PostProcess,
        pipelines: &BTreeMap<String, pipeline_handler::EffectPipelineData>,
        texture_object: &TextureObject,
        pipeline_rcs: &[Rc<RenderPipeline>],
        source_view: &TextureView,
    ) -> Vec<EffectBindGroup> {
        texture_object
            .effects
            .iter()
            .zip(pipeline_rcs.iter())
            .filter_map(|(effect, pipeline)| {
                let pipedata = pipelines
                    .values()
                    .find(|d| Rc::ptr_eq(&d.pipeline, pipeline))?;
                let pass = effect.passes.first()?;

                // textures array index = GL texture unit: [0]=source, [1]=g_Texture1, [2]=g_Texture2
                let mask_path = pass.textures.get(1).and_then(|t| t.as_deref());
                let noise_path = pass.textures.get(2).and_then(|t| t.as_deref());

                let (mask_tex, mask_view) = mask_path
                    .and_then(|p| load_mask_texture(device, queue, scene, p))
                    .map(|(t, v)| (Some(t), Some(v)))
                    .unwrap_or((None, None));

                let (noise_tex, noise_view) = noise_path
                    .and_then(|p| load_mask_texture(device, queue, scene, p))
                    .map(|(t, v)| (Some(t), Some(v)))
                    .unwrap_or((None, None));

                let constants = pass.constantshadervalues.clone().unwrap_or_default();
                let material_keys = pipedata.layout.uniform_material_keys.clone();

                let sw = texture_object.texture.dimension[0] as f32;
                let sh = texture_object.texture.dimension[1] as f32;

                // Build tex_resolutions for all sampler slots declared in the shader.
                // Shaders reference g_TextureNResolution for N in sampler_names, and
                // division by zero (unset = 0) causes NaN displacements.
                let mut tex_resolutions = BTreeMap::new();
                for (i, sampler_name) in pipedata.layout.sampler_names.iter().enumerate() {
                    let res_key = format!("{}Resolution", sampler_name);
                    let (w, h) = match i {
                        0 => (sw, sh),
                        1 => mask_tex
                            .as_ref()
                            .map(|t| (t.width() as f32, t.height() as f32))
                            .unwrap_or((sw, sh)),
                        2 => noise_tex
                            .as_ref()
                            .map(|t| (t.width() as f32, t.height() as f32))
                            .unwrap_or((sw, sh)),
                        _ => (sw, sh),
                    };
                    tex_resolutions.insert(res_key, [w, h, w, h]);
                }

                EffectBindGroup::new(
                    device,
                    post_process,
                    pipedata,
                    source_view,
                    mask_view.as_ref(),
                    noise_view.as_ref(),
                    Rc::clone(pipeline),
                    material_keys,
                    constants,
                    tex_resolutions,
                    mask_tex,
                    noise_tex,
                )
            })
            .collect()
    }
}
