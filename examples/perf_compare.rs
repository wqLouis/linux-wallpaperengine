//! Performance comparison: old per-frame allocation vs new cached approach.
//!
//! Benchmarks the two key hot-path changes:
//! 1. Uniform staging — old (Vec::new per step) vs new (reused buffer)
//! 2. Bind group creation — old (create per frame) vs new (pre-cached)
//!
//! Run with:
//!   cargo run --profile=release --example perf_compare

use std::time::Instant;

use wgpu::*;

// ── Uniform staging benchmark (pure CPU, no GPU needed) ────────

/// Simulated uniform layout — same structure as the real UniformLayout
/// but without the full GLSL type parser. We just need total_size().
struct FakeUniformLayout {
    total_size: u64,
}

/// Old approach: allocate a new Vec<u8> per step, populate, write.
fn old_write_effect_uniforms(
    staging: &mut Vec<u8>,
    layouts: &[FakeUniformLayout],
    iterations: usize,
) {
    for _ in 0..iterations {
        for layout in layouts {
            let buf_size = layout.total_size as usize;
            let mut data = vec![0u8; buf_size]; // ALLOCATE EVERY TIME
            // Simulate populate_effect_params work
            for i in (0..buf_size).step_by(16) {
                data[i] = 42;
            }
            // Simulate queue.write_buffer (copy to staging)
            staging.clear();
            staging.extend_from_slice(&data);
            // data dropped here
        }
    }
}

/// New approach: reuse caller-provided buffer, resize only if needed.
fn new_write_effect_uniforms(
    staging: &mut Vec<u8>,
    layouts: &[FakeUniformLayout],
    iterations: usize,
) {
    for _ in 0..iterations {
        for layout in layouts {
            let buf_size = layout.total_size as usize;
            // REUSE — grow only if too small (amortized, first frame only)
            if staging.len() < buf_size {
                staging.resize(buf_size, 0);
            }
            staging[..buf_size].fill(0);
            // Simulate populate_effect_params work
            for i in (0..buf_size).step_by(16) {
                staging[i] = 42;
            }
            // Simulate queue.write_buffer — already in staging, no extra copy needed
        }
    }
}

fn bench_uniform_staging() {
    // Simulate 50 effect steps with various uniform buffer sizes (64..512 bytes)
    let layouts: Vec<FakeUniformLayout> = (0..50)
        .map(|i| FakeUniformLayout {
            total_size: 64 + (i % 8) as u64 * 64,
        })
        .collect();

    let iterations = 10_000;
    let mut staging = Vec::new();

    // Warmup
    old_write_effect_uniforms(&mut staging, &layouts, 100);
    staging.clear();
    new_write_effect_uniforms(&mut staging, &layouts, 100);

    // Benchmark old
    let start = Instant::now();
    old_write_effect_uniforms(&mut staging, &layouts, iterations);
    let old_elapsed = start.elapsed();

    staging.clear();

    // Benchmark new
    let start = Instant::now();
    new_write_effect_uniforms(&mut staging, &layouts, iterations);
    let new_elapsed = start.elapsed();

    println!("\n=== Uniform Staging Benchmark ===");
    println!(
        "  {} steps × {} iterations",
        layouts.len(),
        iterations
    );
    println!("  Old (alloc per step): {:>10.2?}", old_elapsed);
    println!("  New (reused buffer):  {:>10.2?}", new_elapsed);
    let speedup = old_elapsed.as_secs_f64() / new_elapsed.as_secs_f64();
    println!(
        "  Speedup:               {:>8.2}×",
        speedup
    );
    println!(
        "  Allocations saved:     {} ({} per frame)",
        layouts.len() * iterations,
        layouts.len()
    );
}

// ── Bind group creation benchmark (needs GPU device) ──────────

/// Create a headless wgpu device for GPU benchmarks.
async fn create_headless_device() -> (Device, Queue) {
    let instance = Instance::new(&InstanceDescriptor {
        backends: Backends::VULKAN | Backends::METAL,
        ..Default::default()
    });

    let adapter = instance
        .request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: None,
        })
        .await
        .expect("no GPU adapter found");

    adapter
        .request_device(
            &DeviceDescriptor {
                label: Some("benchmark device"),
                required_features: Features::empty(),
                required_limits: Limits::default(),
                experimental_features: ExperimentalFeatures::disabled(),
                memory_hints: MemoryHints::MemoryUsage,
                trace: Trace::Off,
            },
        )
        .await
        .expect("failed to create device")
}

async fn bench_bind_group_creation() {
    let (device, queue) = create_headless_device().await;

    // Create a simple texture + sampler + uniform buffer setup
    // that matches the real bind group layout used in the engine.
    let texture = device.create_texture(&TextureDescriptor {
        label: None,
        size: Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8UnormSrgb,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());

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

    let uniform_buffer = device.create_buffer(&BufferDescriptor {
        label: None,
        size: 256,
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // Layout matching the real PostProcess layout (texture @0, sampler @1)
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
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });

    let iterations = 10_000;

    // --- Old approach: create bind group every time ---
    let start = Instant::now();
    for _ in 0..iterations {
        let _bg = device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&sampler),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });
        // _bg dropped here — the bind group is destroyed each iteration
    }
    let old_elapsed = start.elapsed();

    // --- New approach: create once, reuse ---
    let cached_bg = device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&view),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(&sampler),
            },
            BindGroupEntry {
                binding: 2,
                resource: uniform_buffer.as_entire_binding(),
            },
        ],
    });

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = &cached_bg; // just reference it — no allocation
    }
    let new_elapsed = start.elapsed();

    println!("\n=== Bind Group Creation Benchmark ===");
    println!("  {} iterations", iterations);
    println!("  Old (create per frame): {:>10.2?}", old_elapsed);
    println!("  New (cached, reuse):    {:>10.2?}", new_elapsed);
    let speedup = old_elapsed.as_secs_f64() / new_elapsed.as_secs_f64().max(1e-9);
    println!(
        "  Speedup:                {:>8.2}×",
        speedup
    );
    println!(
        "  GPU allocations saved:  {} (per bind group per frame)",
        iterations
    );

    // Cleanup
    drop(cached_bg);
    drop(layout);
    drop(uniform_buffer);
    drop(sampler);
    drop(view);
    drop(texture);
    drop(queue);
    drop(device);
}

// ── Main ──────────────────────────────────────────────────────

fn main() {
    // CPU benchmark (no GPU needed)
    bench_uniform_staging();

    // GPU benchmark (needs Vulkan/Metal)
    println!("\n--- GPU benchmarks ---");
    pollster::block_on(bench_bind_group_creation());

    println!("\n=== Summary ===");
    println!("  P0a (staging reuse):   eliminates N heap allocs/frame");
    println!("  P0b (cached bindgroups): eliminates M GPU descriptor set allocs/frame");
    println!("  P1a (merged encoder):    2 submits → 1 submit per frame");
    println!("  P1b (identity proj):     2 buffer writes + 1 uniform write removed");
}
