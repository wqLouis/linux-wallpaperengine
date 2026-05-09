# Core Renderer (`src/scene/renderer/`)

GPU rendering infrastructure: device creation, surface management, buffers, projection, asset loading.

---

## `app` — WgpuApp

**File:** `app.rs`

The main render orchestrator. Owns all GPU resources and drives the render loop.

### `WgpuApp` struct

```rust
pub struct WgpuApp {
    pub surface: AppSurface,                          // Wgpu surface + config
    pub buffers: Buffers,                             // Vertex/index/projection buffers
    pub projection_bindgroup: ProjectionBindGroups,   // Camera matrix bindgroup
    pub scene_path: String,                           // .pkg file path
    pub clear_color: Vec3,                            // Background color
    pub device: Device,                               // GPU device
    pub queue: Queue,                                 // GPU command queue
    pub audio_stream: rodio::OutputStream,            // Audio output stream
    pub draw_queue: Option<DrawQueue>,                // Built draw objects
    pub post_process: Option<PostProcess>,             // Samplers, blank textures
    pub resolution: Option<[u32; 2]>,                 // Scene resolution
    pub start_time: Instant,                           // Render loop start time
    pub elapsed_ms: u64,                               // Accumulated time (ms)
    pub projection_matrix: [[f32; 4]; 4],             // Camera view-projection
    pub no_effects: bool,                             // Bypass effects flag
}
```

### `WgpuApp::new(...) -> Self`

```rust
pub async fn new(
    scene_path: String,
    surface: InitAppSurface,
    size: [u32; 2],
    no_effects: bool,
) -> Self
```

| Parameter | Description |
|-----------|-------------|
| `scene_path` | Path to `.pkg` file |
| `surface` | Window/display surface (Raw or Winit) |
| `size` | Initial surface dimensions `[width, height]` |
| `no_effects` | Bypass all post-process effects |

Creates the wgpu instance, adapter, device, queue, surface, buffers, and projection bindgroup.

**Required GPU features:**
- `TEXTURE_BINDING_ARRAY`
- `SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING`

**Backends:** Vulkan + Metal.

**Limits:** `max_binding_array_elements_per_shader_stage: MAX_TEXTURE` (512)

### `WgpuApp::load(&mut self)`

Called after `new()`. Loads scene assets and builds the draw queue:

1. `Scene::new(scene_path)` — parses `.pkg` file
2. `PostProcess::new(device, size)` — sampler + blank textures
3. Creates the default `image_pipeline` from `shader/image.wgsl`
4. `ObjectMap::new(objects, scene)` — converts to `TextureObject`/`AudioObject`
5. `DrawQueue::new(...)` — builds GPU draw objects
6. `Projection::new(root).create_camera_uniform()` — camera
7. Loads audio via rodio

### `WgpuApp::render(&mut self) -> Option<()>`

Called every frame. Performs:

1. **Time update** — computes delta, wraps `elapsed_ms` at 1 hour for f32 precision
2. **Uniform write** — calls `write_effect_uniforms()` for all draw objects
3. **Intermediate passes** — if any object has effects, runs `render_intermediate_passes()`
4. **Final pass** — `render_final_pass()` draws all objects to swapchain

### `WgpuApp::resize(&mut self, size: [u32; 2])`

Updates `surface.config` dimensions and reconfigures the swapchain.

### `write_effect_uniforms(queue, objects, elapsed, projection, screen_res)`

Public free function. Writes per-effect GPU uniform buffers with time, projection matrix, screen resolution, texture resolutions, and material constants.

---

## `surface` — AppSurface

**File:** `surface.rs`

Wgpu surface wrapper supporting both raw handles (Wayland) and winit windows.

### `InitAppSurface`

```rust
pub enum InitAppSurface {
    Raw((RawDisplayHandle, RawWindowHandle)),  // Wayland
    Winit(Arc<winit::window::Window>),          // Winit
}
```

Re-exported as `crate::scene::renderer::app::InitAppSurface`.

### `AppSurface`

```rust
pub struct AppSurface {
    pub surface: Surface<'static>,
    pub config: SurfaceConfiguration,
}
```

### `AppSurface::new(surface, instance, adapter, size) -> Self`

Creates the wgpu surface from the appropriate handle type. Configures with:
- `RENDER_ATTACHMENT` usage
- `Mailbox` present mode (for vsync)
- `Auto` alpha mode
- First available capability format

---

## `buffer` — GPU Buffers

**File:** `buffer.rs`

### `Buffers`

```rust
pub struct Buffers {
    pub vertex: Buffer,     // Vertex data (COPY_DST | VERTEX)
    pub index: Buffer,     // Index data (COPY_DST | INDEX)
    pub projection: Buffer, // Camera uniform (COPY_DST | UNIFORM)
    pub vertex_len: u32,
    pub index_len: u32,
}
```

### `Buffers::new(device, index_len, vertex_len) -> Self`

Creates pre-allocated GPU buffers sized for maximum texture count.

### Buffer Methods

| Method | Description |
|--------|-------------|
| `Buffers::draw_rect(&mut self, queue, pos: [Vec3; 4])` | Appends a quad (4 vertices, 6 indices) |
| `Buffers::draw_texture(&mut self, queue, origin, angles, scale, size: Vec2)` | Creates rotated quad, delegates to `draw_rect` |

---

## `vertex` — Vertex Type

**File:** `vertex.rs`

```rust
#[repr(C)]
pub struct Vertex {
    pub pos: [f32; 3],  // World-space position (location 0)
    pub uv: [f32; 2],   // Texture coordinates (location 1)
}
```

### `Vertex::create_buffer_layout() -> VertexBufferLayout`

Returns a standard layout: location 0 = `Float32x3`, location 1 = `Float32x2`.

### `NDC_VERTICES`

Pre-defined fullscreen quad vertices in NDC space `[-1,1]`. Used by ping-pong intermediate passes.

---

## `projection` — Camera System

**File:** `projection.rs`

### `Projection`

Camera configuration extracted from scene JSON.

```rust
struct Projection {
    center: Vec3, eye: Vec3, up: Vec3,
    nearz: f32, farz: f32,
    width: f32, height: f32,
}
```

### `Projection::new(root: &Root) -> Self`

Extracts camera parameters from the scene root (center, eye positions, clip planes, orthographic dimensions).

### `Projection::create_camera_uniform(&self) -> CameraUniform`

Computes the view-projection matrix:
- **View:** `Mat4::look_at_rh(eye, center, up)`
- **Projection:** `Mat4::orthographic_rh(0, w, 0, h, nearz, farz)`

### `CameraUniform`

```rust
#[repr(C)]
pub struct CameraUniform {
    pub projection: [[f32; 4]; 4],
}
```

### `ProjectionBindGroups`

```rust
pub struct ProjectionBindGroups {
    pub projection_layout: BindGroupLayout,
    pub projection: Option<BindGroup>,
}
```

| Method | Description |
|--------|-------------|
| `ProjectionBindGroups::new(device) -> Self` | Creates bind group layout (binding 0) |
| `create_projection_bindgroup(...)` | Creates bind group and uploads uniform data |

---

## `post_process` — Samplers & Blank Textures

**File:** `post_process.rs`

```rust
pub struct PostProcess {
    pub sampler: Sampler,          // ClampToEdge, Linear/Nearest
    pub layout: BindGroupLayout,    // Texture(0) + Sampler(1)
    pub blank_texture: Texture,     // Dummy texture for unused slots
    // ... internal fields
}
```

### `PostProcess::new(device, res) -> Self`

Creates:
- **Sampler** with `ClampToEdge` addressing, `Linear` mag / `Nearest` min filtering
- **Bind group layout** with binding 0 (`Texture2D, fragment`) and binding 1 (`Sampler, fragment`)
- **Blank textures** at the specified resolution for fallback usage

---

## `load` — Asset Loading

**File:** `load.rs`

Implements `WgpuApp::load()` as described above. Also contains:

### `load_audios(audio_stream, audios, scene)`

Loads audio files from `scene.misc` using rodio:
- `PlaybackMode::Loop` → `source.repeat_infinite()`
- Playback runs on a spawned thread via `Sink::sleep_until_end()`

### `create_pipeline(app, bindgroup_layout) -> RenderPipeline`

Creates the default image rendering pipeline:
- WGSL shader from `shader/image.wgsl`
- Alpha blending: `SrcAlpha / OneMinusSrcAlpha`
- Back-face culling
- Two bind groups: image (0) + projection (1)

### `create_effect_pipeline(...)` (in `pipeline_handler.rs`)

Creates effect pipelines with:
- GLSL shader sources from `scene.misc`
- Preprocessed via `preprocess_pair()`
- Alpha blending, back-face culling, `Rgba8UnormSrgb` format
- Three bind groups: effect resources (0) + projection (1)

---

## Shader / Image Pipeline

The default rendering pipeline uses a WGSL shader (`shader/image.wgsl`) for simple texturing:

```wgsl
// Expected bindings:
Binding 0: texture2d    // Source texture
Binding 1: sampler      // ClampToEdge, Linear
```

Effects use GLSL shaders from the `.pkg` file, preprocessed to Vulkan-compatible GLSL before compilation.

---

## GPU Requirements

| Feature | Value | Purpose |
|---------|-------|---------|
| `TEXTURE_BINDING_ARRAY` | Required | Texture array indexing |
| `SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING` | Required | Non-uniform texture access |
| `max_binding_array_elements_per_shader_stage` | 512 | Maximum textures per stage |
| Backends | Vulkan + Metal | Cross-platform GPU access |