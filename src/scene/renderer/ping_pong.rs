//! Pair of render targets for ping-pong post-processing.
//!
//! Effects are applied by alternating between two textures: read from
//! one, write to the other, then swap. Also provides NDC vertex/index
//! buffers for rendering full-screen quads in effect passes.

use wgpu::*;

use super::post_process::PostProcess;
use super::vertex::{NDC_VERTICES, Vertex};

pub struct PingPongTextures {
    #[allow(dead_code)]
    tex_a: Texture,
    #[allow(dead_code)]
    tex_b: Texture,
    pub view_a: TextureView,
    pub view_b: TextureView,
    pub ndc_vbuf: Buffer,
    pub ndc_ibuf: Buffer,
}

impl PingPongTextures {
    pub fn new(
        device: &Device,
        queue: &Queue,
        _post_process: &PostProcess,
        width: u32,
        height: u32,
    ) -> Self {
        let make_tex = |device: &Device| {
            device.create_texture(&TextureDescriptor {
                label: None,
                size: Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8UnormSrgb,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };

        let tex_a = make_tex(device);
        let tex_b = make_tex(device);
        let view_a = tex_a.create_view(&Default::default());
        let view_b = tex_b.create_view(&Default::default());

        let ndc_vbuf = device.create_buffer(&BufferDescriptor {
            label: None,
            size: std::mem::size_of::<Vertex>() as u64 * 4,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&ndc_vbuf, 0, bytemuck::bytes_of(&NDC_VERTICES));

        let ndc_ibuf = device.create_buffer(&BufferDescriptor {
            label: None,
            size: std::mem::size_of::<u32>() as u64 * 6,
            usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&ndc_ibuf, 0, bytemuck::bytes_of(&[0u32, 2, 1, 0, 3, 2]));

        Self {
            tex_a,
            tex_b,
            view_a,
            view_b,
            ndc_vbuf,
            ndc_ibuf,
        }
    }

    /// Create a bind group from `view_a` (the final result after all effects).
    pub fn make_bindgroup(
        &self,
        device: &Device,
        layout: &BindGroupLayout,
        sampler: &Sampler,
    ) -> BindGroup {
        Self::make_bg(device, layout, &self.view_a, sampler)
    }

    /// Create a bind group from an arbitrary view (used during intermediate swaps).
    pub fn make_bindgroup_for(
        &self,
        device: &Device,
        layout: &BindGroupLayout,
        sampler: &Sampler,
        view: &TextureView,
    ) -> BindGroup {
        Self::make_bg(device, layout, view, sampler)
    }

    fn make_bg(
        device: &Device,
        layout: &BindGroupLayout,
        view: &TextureView,
        sampler: &Sampler,
    ) -> BindGroup {
        device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(sampler),
                },
            ],
        })
    }
}
