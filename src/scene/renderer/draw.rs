//! Draw queue construction: batches scene objects into GPU draw calls.
//!
//! Each [`TextureObject`] from the scene loader is converted into a
//! [`DrawObject`] with its texture bind group, a flattened list of
//! [`EffectStep`]s (unifying single-pass and multi-pass effects), and
//! optional ping-pong intermediate textures for post-processing.

use std::{cell::RefCell, rc::Rc};

use crate::collections::{Map, new_map, new_map_with_capacity};
use pkg_parser::pkg_parser::{
    tex_parser::Tex,
    video_parser::{VideoFrame, VideoPlayer},
};
use wgpu::*;

use crate::scene::{
    loader::{
        assets_loader::VideoAsset,
        mdl::{PuppetAnimation, PuppetMesh},
        mip_loader::MipChainGenerator,
        object_loader::{TextureObject, TextureSource},
    },
    renderer::{
        buffer::Buffers,
        ping_pong::PingPongTextures,
        post_processor::{
            context::{EffectContext, EffectTarget, PipelineMap},
            effect_step::{self, EffectStep, FboTexture},
        },
        vertex::Vertex,
    },
};

pub struct DrawObject {
    pub index_range: [u32; 2],
    pub bindgroup: BindGroup,
    /// All effect steps (single-pass and multi-pass flattened together).
    pub effect_steps: Vec<EffectStep>,
    /// Named FBOs allocated for multi-pass effect chains.
    pub fbos: Map<String, FboTexture>,
    pub intermediates: Option<PingPongTextures>,
    /// Per-frame CPU-skinning state, present only for animated puppets.
    pub skinning: Option<SkinningState>,
    /// Set when the object is attached to a bone socket of a puppet parent.
    pub attachment: Option<AttachmentState>,
    /// The decoder of an animated (video) object, present only when the
    /// object's pixels come from a video.
    pub video: Option<VideoDrawState>,
}

/// The per-object state of an animated (video) texture.
pub struct VideoDrawState {
    /// The GPU texture the frames are uploaded into.
    pub texture: wgpu::Texture,
    /// The playback state (interior mutability: the draw queue is shared).
    state: RefCell<VideoState>,
}

/// The playback state of an animated texture.
///
/// A clip that was decoded eagerly is played back by index: its frames are
/// already in memory, so nothing is decoded while the wallpaper runs.  A
/// clip that does not fit into the memory budget of the parser is streamed:
/// the decoder hands out the frame that covers the current time.
enum VideoState {
    /// The clip was decoded eagerly: the frames are played back by index.
    Eager {
        /// The decoded frames, in presentation order.
        frames: Rc<[VideoFrame]>,
        /// The presentation time of every frame, in increasing order.
        times: Vec<f64>,
        /// The duration of the clip, in seconds.
        duration: f64,
        /// The index of the frame that is currently uploaded.
        uploaded: Option<usize>,
    },
    /// The clip is streamed: the decoder hands out the frames.
    Streaming {
        /// The decoder: it hands out the frame that covers a given time.
        player: VideoPlayer,
        /// The presentation time of the frame that is currently uploaded.
        uploaded: Option<f64>,
    },
}

/// An object that is attached to a bone socket of a puppet parent.
///
/// Wallpaper Engine builds the socket matrix from the puppet's *runtime*
/// bone matrices, so the attached object follows the animation.  The port
/// re-places the object's quad every frame with the animated socket matrix.
pub struct AttachmentState {
    /// The socket name in the parent's MDL (`MDAT` attachment).
    pub socket: String,
    /// The parent's object id (the puppet).
    pub parent_id: i64,
    /// Index of the parent's draw object in the draw queue.
    pub parent: usize,
    /// The object's object-local vertices (positions + UVs).
    pub local: Vec<Vertex>,
    /// The object's own model matrix (origin/rotation/scale/alignment),
    /// i.e. the offset from the socket.
    pub local_model: glam::Mat4,
    /// First vertex of this object's range in the shared vertex buffer.
    pub vertex_offset: u32,
}

/// Everything needed to re-skin one puppet object every frame.
///
/// The rest mesh and index buffer already live in the shared GPU buffers;
/// only the vertex positions are rewritten by
/// [`update_animated_meshes`].
pub struct SkinningState {
    pub mesh: PuppetMesh,
    pub animations: Vec<PuppetAnimation>,
    /// First vertex of this object's range in the shared vertex buffer.
    pub vertex_offset: u32,
    pub model: glam::Mat4,
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
        let mut render_pipelines = new_map();

        let mut draw_objects: Vec<DrawObject> = Vec::with_capacity(texture_objects.len());
        let mut id_to_index: Map<i64, usize> = new_map_with_capacity(texture_objects.len());
        for tex_obj in texture_objects {
            let id = tex_obj.id;
            let obj = DrawObject::build(ctx, buffers, tex_obj, &mut render_pipelines);
            id_to_index.insert(id, draw_objects.len());
            draw_objects.push(obj);
        }

        // Resolve the attachment parents now that every object has an index.
        for obj in draw_objects.iter_mut() {
            if let Some(att) = &mut obj.attachment
                && let Some(&index) = id_to_index.get(&att.parent_id)
            {
                att.parent = index;
            }
        }

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
        mut texture_object: TextureObject,
        pipelines: &mut PipelineMap,
    ) -> Self {
        let effects = &ctx.effects;
        let device = effects.device;
        let queue = effects.queue;
        let post_process = effects.post_process;
        let index_start = buffers.index_len;

        let (texture, video) = Self::upload_texture(device, queue, &texture_object, ctx.mipgen);
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

        let [tex_w, tex_h] = texture_object.source.dimension();

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

        let vertex_offset = buffers.vertex_len;

        // An object attached to a bone socket of its puppet parent: the
        // socket matrix is re-evaluated every frame (see
        // `update_animated_meshes`), so keep the local vertices around.
        let attachment = match (
            texture_object.attachment.as_deref(),
            texture_object.parent,
            texture_object.mesh.is_none(),
        ) {
            (Some(socket), Some(parent_id), true) => Some(AttachmentState {
                socket: socket.to_string(),
                parent_id,
                parent: usize::MAX,
                local: Buffers::quad_vertices(
                    texture_object.size,
                    texture_object.uv_scale,
                )
                .to_vec(),
                local_model: texture_object.local_model,
                vertex_offset,
            }),
            _ => None,
        };

        let (index_range, skinning) = if let Some(mesh) = texture_object.mesh.take() {
            log::debug!(
                "drawing mesh: {} verts, {} indices",
                mesh.vertices.len(),
                mesh.indices.len(),
            );
            let range = buffers.draw_mesh(
                queue,
                &mesh.vertices,
                &mesh.indices,
                texture_object.model,
            );
            let skinning = if texture_object.animations.is_empty() {
                None
            } else {
                Some(SkinningState {
                    mesh,
                    animations: texture_object.animations,
                    vertex_offset,
                    model: texture_object.model,
                })
            };
            (range, skinning)
        } else {
            buffers.draw_texture(
                queue,
                texture_object.model,
                texture_object.size,
                texture_object.uv_scale,
            );
            ([index_start, buffers.index_len], None)
        };

        Self {
            index_range,
            bindgroup,
            effect_steps,
            fbos,
            intermediates,
            skinning,
            attachment,
            video,
        }
    }

    /// Upload the pixels of an object into a GPU texture.
    ///
    /// A static texture is uploaded once, with its full mip chain; a video
    /// gets a single-mip texture that is refreshed every frame (see
    /// [`update_video_textures`]).
    ///
    /// The returned state is the decoder of a video: it is `None` for a
    /// static texture.
    fn upload_texture(
        device: &Device,
        queue: &Queue,
        tex_obj: &TextureObject,
        mipgen: &MipChainGenerator,
    ) -> (Texture, Option<VideoDrawState>) {
        match &tex_obj.source {
            TextureSource::Static(tex) => (
                Self::upload_static_texture(device, queue, tex, mipgen),
                None,
            ),
            TextureSource::Video(video) => Self::upload_video_texture(device, queue, video),
        }
    }

    /// Upload a static texture, with the mip chain the parser extracted.
    fn upload_static_texture(
        device: &Device,
        queue: &Queue,
        tex: &Tex,
        mipgen: &MipChainGenerator,
    ) -> Texture {
        let ext = tex.extension.as_str();
        let is_bcn = matches!(ext, "dxt1" | "dxt5");
        let format = match ext {
            "r8" => TextureFormat::R8Unorm,
            "rg88" => TextureFormat::Rg8Unorm,
            "dxt1" => TextureFormat::Bc1RgbaUnormSrgb,
            "dxt5" => TextureFormat::Bc3RgbaUnormSrgb,
            _ => TextureFormat::Rgba8UnormSrgb,
        };

        let w = tex.dimension[0];
        let h = tex.dimension[1];
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
            tex.actual_mip_count().max(1)
        } else {
            tex.mipmap_count.max(1)
        };

        log::debug!(
            "upload_texture: {}x{} fmt={:?} mips={} (advertised={}, actual={}, missing={})",
            w,
            h,
            format,
            mip_level_count,
            tex.mipmap_count,
            tex.actual_mip_count(),
            tex.missing_mip_count(),
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
            &tex.payload,
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
        for (i, level_data) in tex.mip_levels.iter().enumerate() {
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
        if tex.needs_gpu_mip_generation() && !is_bcn {
            let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
                label: Some("mipgen.encoder"),
            });
            let generated = mipgen.generate(device, &mut encoder, &texture, tex);
            if generated > 0 {
                queue.submit(std::iter::once(encoder.finish()));
            }
        }

        texture
    }

    /// Create the texture of an animated (video) object and its playback
    /// state.
    ///
    /// The texture holds a single mip level: the frames are uploaded as
    /// they are played, so there is no mip chain to maintain.  The first
    /// frame is uploaded right away — its size is the size of the
    /// texture, because the bind group is created once, from that texture.
    ///
    /// A clip that was decoded eagerly is played back by index: its frames
    /// are already in memory and nothing is decoded while the wallpaper
    /// runs.  The other clips are streamed by a [`VideoPlayer`].
    ///
    /// The returned state is the playback state of a video: it is `None`
    /// for a video that cannot be opened at all.
    fn upload_video_texture(
        device: &Device,
        queue: &Queue,
        video: &Rc<VideoAsset>,
    ) -> (Texture, Option<VideoDrawState>) {
        // The clip was decoded eagerly: its frames are in memory and are
        // played back by index.
        if let Some(frames) = video.frames.as_ref().filter(|frames| !frames.is_empty()) {
            return Self::upload_decoded_texture(device, queue, frames, video.info.duration);
        }

        // The clip is too big for the memory budget: it is streamed.
        let mut player = match VideoPlayer::open(&video.bytes) {
            Ok(player) => player,
            Err(error) => {
                log::error!("cannot open the video: {}", error);
                return (Self::blank_texture(device, queue), None);
            }
        };

        let first = player.frame_at(0.0).ok().flatten();
        let (width, height) = match first.as_ref() {
            Some(frame) => (frame.width, frame.height),
            None => (video.info.width, video.info.height),
        };
        let (width, height) = (width.max(1), height.max(1));

        let texture = device.create_texture(&TextureDescriptor {
            label: Some("video texture"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Show the first frame right away: the object must not be black
        // until the first frame of the next render pass.
        let uploaded = first.as_ref().map(|frame| frame.time);
        if let Some(frame) = first.as_ref() {
            Self::write_video_frame(queue, &texture, frame);
        }

        let state = VideoState::Streaming { player, uploaded };

        (
            texture.clone(),
            Some(VideoDrawState {
                texture,
                state: RefCell::new(state),
            }),
        )
    }

    /// Create the texture of an object whose clip was decoded eagerly.
    ///
    /// The frames are already in memory: the texture is filled with the
    /// first frame and the other ones are uploaded when the playback
    /// reaches them.
    fn upload_decoded_texture(
        device: &Device,
        queue: &Queue,
        frames: &Rc<[VideoFrame]>,
        duration: f64,
    ) -> (Texture, Option<VideoDrawState>) {
        let first = &frames[0];
        let (width, height) = (first.width.max(1), first.height.max(1));

        let texture = device.create_texture(&TextureDescriptor {
            label: Some("video texture"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // The presentation times of the frames: the frame that covers a
        // given time is found with a binary search over them.
        let times = frames.iter().map(|frame| frame.time).collect();

        // Show the first frame right away: the object must not be black
        // until the first frame of the next render pass.
        Self::write_video_frame(queue, &texture, first);

        let state = VideoState::Eager {
            frames: Rc::clone(frames),
            times,
            duration,
            uploaded: Some(0),
        };

        (
            texture.clone(),
            Some(VideoDrawState {
                texture,
                state: RefCell::new(state),
            }),
        )
    }

    /// Whether a frame can be written into the texture of an object.
    ///
    /// The texture was sized after the first frame of the clip: a frame of
    /// another size cannot be written into it.  The problem is reported.
    fn frame_fits(texture: &Texture, frame: &VideoFrame) -> bool {
        if frame.width == texture.width() && frame.height == texture.height() {
            return true;
        }

        log::warn!(
            "skipping a {}x{} video frame: the texture is {}x{}",
            frame.width,
            frame.height,
            texture.width(),
            texture.height(),
        );
        false
    }

    /// Write a decoded frame into the texture of an animated object.
    fn write_video_frame(queue: &Queue, texture: &Texture, frame: &VideoFrame) {
        queue.write_texture(
            TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &frame.rgba,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(frame.width * 4),
                rows_per_image: None,
            },
            Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// A 1×1 transparent texture, used when a video cannot be opened.
    fn blank_texture(device: &Device, queue: &Queue) -> Texture {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("blank video texture"),
            size: Extent3d {
                width: 1,
                height: 1,
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
            &[0u8, 0, 0, 0],
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );

        texture
    }
}

/// Decode and upload the animated textures for this frame.
///
/// Called once per frame, next to [`update_animated_meshes`].  Every
/// animated object is asked for the frame that covers the current time;
/// the frame is only uploaded when it changed, so a video that plays
/// slower than the compositor does not cost a texture upload per frame.
///
/// A clip that was decoded eagerly is played back by index: its frames are
/// already in memory, so nothing is decoded here.  The other clips are
/// streamed by their decoder.
///
/// A video that cannot be decoded is reported and skipped: it must not
/// kill the render loop.
pub fn update_video_textures(queue: &Queue, draw_queue: &DrawQueue, elapsed: f32) {
    for draw_object in draw_queue.queue.iter() {
        let Some(video) = &draw_object.video else {
            continue;
        };

        let mut state = video.state.borrow_mut();

        match &mut *state {
            VideoState::Eager {
                frames,
                times,
                duration,
                uploaded,
            } => {
                // The video loops: the time within the stream is the
                // elapsed time modulo the duration of the video.
                let time = loop_time(*duration, elapsed);

                // The frame that is on screen is found by a binary search
                // over the times of the decoded frames.
                let Some(index) = frame_index_at(times.as_slice(), time) else {
                    continue;
                };

                // The frames are already in memory: the texture is only
                // written when the frame that is on screen changed.
                if *uploaded == Some(index) {
                    continue;
                }

                let frame = &frames[index];
                if !DrawObject::frame_fits(&video.texture, frame) {
                    continue;
                }

                DrawObject::write_video_frame(queue, &video.texture, frame);
                *uploaded = Some(index);
            }
            VideoState::Streaming { player, uploaded } => {
                // The video loops: the time within the stream is the
                // elapsed time modulo the duration of the video.
                let duration = player.info().duration;
                let time = loop_time(duration, elapsed);

                let frame = match player.frame_at(time) {
                    Ok(Some(frame)) => frame,
                    Ok(None) => continue,
                    Err(error) => {
                        log::warn!("cannot decode the video frame: {}", error);
                        continue;
                    }
                };

                // The player serves the same frame again when the video
                // plays slower than the compositor: there is nothing to
                // upload then.
                if *uploaded == Some(frame.time) {
                    continue;
                }

                if !DrawObject::frame_fits(&video.texture, &frame) {
                    continue;
                }

                DrawObject::write_video_frame(queue, &video.texture, &frame);
                *uploaded = Some(frame.time);
            }
        }
    }
}

/// The tolerance used when comparing the presentation times of two frames.
///
/// The times come from different sources (the time base of the container,
/// the delays of a GIF) and are compared with floating point arithmetic,
/// so an epsilon keeps the lookup from picking the wrong frame.
const TIME_EPSILON: f64 = 1e-6;

/// The index of the frame that is on screen at `time`.
///
/// `times` holds the presentation times of the frames of a clip, in
/// increasing order.  The function returns the index of the last frame
/// whose time is not after `time`: that frame covers `time`.  The first
/// frame is returned when `time` is before the first frame of the clip,
/// and `None` is returned when the clip has no frame at all.
///
/// The times are sorted, so the frame is found with a binary search: the
/// lookup does not depend on the number of the frames of the clip.
pub fn frame_index_at(times: &[f64], time: f64) -> Option<usize> {
    if times.is_empty() {
        return None;
    }

    // The number of the frames whose time is not after `time`: the frame
    // before them is the one that covers `time`.
    let count = times.partition_point(|frame_time| *frame_time <= time + TIME_EPSILON);
    Some(count.saturating_sub(1))
}

/// The time within a clip that loops.
///
/// The elapsed time is taken modulo the duration of the clip, so that the
/// clip plays again and again.  A clip of an unknown duration is not
/// wrapped.
fn loop_time(duration: f64, elapsed: f32) -> f64 {
    if duration > 0.0 {
        f64::from(elapsed) % duration
    } else {
        f64::from(elapsed)
    }
}

/// CPU-skin every animated puppet and rewrite its vertex range in the
/// shared buffer.
///
/// Called once per frame before the render pass.  `elapsed` is the global
/// wall-clock time in seconds; each object scales it by its own playback
/// rate.  Objects without an animation keep their baked rest mesh.
pub fn update_animated_meshes(
    queue: &Queue,
    buffers: &Buffers,
    draw_queue: &DrawQueue,
    elapsed: f32,
) {
    // The animated bone sockets of every puppet, indexed like the draw
    // queue.  `None` for objects that are not animated puppets.
    let mut sockets: Vec<Vec<(String, glam::Mat4)>> = vec![Vec::new(); draw_queue.queue.len()];

    for (index, draw_object) in draw_queue.queue.iter().enumerate() {
        let Some(skinning) = &draw_object.skinning else {
            continue;
        };
        let vertices = skinning.mesh.skinned_vertices(&skinning.animations, elapsed);
        buffers.update_mesh(queue, skinning.vertex_offset, &vertices, skinning.model);

        sockets[index] = skinning
            .mesh
            .animated_attachment_worlds(&skinning.animations, elapsed);
    }

    // Objects attached to a bone socket follow the puppet's bones.
    for draw_object in draw_queue.queue.iter() {
        let Some(attachment) = &draw_object.attachment else {
            continue;
        };
        if attachment.parent >= sockets.len() {
            continue;
        }
        let Some(parent) = draw_queue.queue[attachment.parent].skinning.as_ref() else {
            // The parent is not an animated puppet: the socket stays in the
            // bind pose (which is where it was placed at load time).
            continue;
        };
        let Some((_, socket)) = sockets[attachment.parent]
            .iter()
            .find(|(name, _)| *name == attachment.socket)
        else {
            continue;
        };

        // The socket matrix is in the puppet's local space — compose it
        // with the puppet's world matrix and the object's own offset from the
        // socket, exactly like the loader does:
        //     M_world(child) = M_world(parent) * M_socket * M_local
        let model = parent.model * (*socket * attachment.local_model);
        buffers.update_mesh(queue, attachment.vertex_offset, &attachment.local, model);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frame that covers a time is the last one before it.
    #[test]
    fn the_frame_of_a_time_is_found_by_its_index() {
        let times = [0.0, 0.1, 0.2, 0.3];

        assert_eq!(frame_index_at(&times, 0.0), Some(0));
        assert_eq!(frame_index_at(&times, 0.05), Some(0));
        assert_eq!(frame_index_at(&times, 0.1), Some(1));
        assert_eq!(frame_index_at(&times, 0.25), Some(2));
        assert_eq!(frame_index_at(&times, 0.3), Some(3));

        // A time before the first frame shows the first one, a time after
        // the last one shows the last one.
        assert_eq!(frame_index_at(&times, -1.0), Some(0));
        assert_eq!(frame_index_at(&times, 10.0), Some(3));

        // A clip without a frame has nothing to show.
        assert_eq!(frame_index_at(&[], 0.0), None);
    }

    /// The lookup does not depend on the number of the frames of the clip.
    #[test]
    fn the_frame_of_a_time_is_found_in_a_long_clip() {
        let times: [f64; 1000] = std::array::from_fn(|index| (index as f64) * 0.1);

        // The binary search finds the same frame as a linear search.
        for step in 0..200 {
            let time = f64::from(step) * 0.53;
            let expected = times
                .iter()
                .rposition(|frame_time| *frame_time <= time + TIME_EPSILON)
                .unwrap_or(0);
            assert_eq!(frame_index_at(&times, time), Some(expected));
        }
    }

    /// A looping clip plays the time within the clip again and again.
    ///
    /// The elapsed time is an `f32`, so the wrapped time is only exact to
    /// the precision of an `f32` (a few 1e-8): the comparisons use a
    /// tolerance that fits an `f32` input.
    #[test]
    fn a_looping_clip_wraps_the_time() {
        const EPSILON: f64 = 1e-6;

        assert!((loop_time(0.4, 0.0) - 0.0).abs() < EPSILON);
        assert!((loop_time(0.4, 0.5) - 0.1).abs() < EPSILON);
        assert!((loop_time(0.4, 1.7) - 0.1).abs() < EPSILON);

        // A clip of an unknown duration is not wrapped.
        assert!((loop_time(0.0, 1.7) - 1.7).abs() < EPSILON);
    }
}
