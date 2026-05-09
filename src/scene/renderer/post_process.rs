use wgpu::*;

/// Shared sampler, bind group layout, and blank texture for post-processing.
///
/// Created once per wallpaper load and used by all effect bind groups.
pub struct PostProcess {
    pub sampler: Sampler,
    pub layout: BindGroupLayout,
    pub blank_texture: Texture,
}

impl PostProcess {
    pub fn new(device: &Device, queue: &Queue, res: [u32; 2]) -> Self {
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: None,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: None,
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

        let blank_texture = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: res[0],
                height: res[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Initialize blank texture to white (mask=1.0 when no mask texture is bound)
        let blank_data = vec![255u8; (res[0] * res[1] * 4) as usize];
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &blank_texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &blank_data,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(res[0] * 4),
                rows_per_image: None,
            },
            Extent3d {
                width: res[0],
                height: res[1],
                depth_or_array_layers: 1,
            },
        );

        Self {
            sampler,
            layout,
            blank_texture,
        }
    }
}
