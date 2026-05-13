# Core Renderer (`src/scene/renderer/`)

GPU rendering infrastructure: device creation, surface management, buffers, projection, asset loading.

---

## `app` — WgpuApp

**File:** `app.rs`

The main render orchestrator. Owns all GPU resources and drives the render loop.

### `UserParams` struct

```rust
pub struct UserParams {
    /// Normalized cursor position in [0, 1] range (0,0) = top-left, (1,1) = bottom-right
    pub cursor_position: [f32; 2],
    /// Raw pixel cursor position (used internally, dead code for now)
    pub cursor_pixel: [u32; 2],
}
```

Adapters can update these params each frame. Default: centered `[0.5, 0.5]` (no parallax shift on Wayland).

### `WgpuApp` struct

```rust
pub struct WgpuApp {
    pub surface: AppSurface,                          // Wgpu surface + config
    pub buffers: Buffers,                             // Vertex/index/projection buffers
    pub projection_bindgroup: ProjectionBindGroups,   // Camera matrix bindgroup
    pub scene_path: String,                           // .pkg file path
    pub assets_path: Option<String>,                  // Wallpaper Engine assets dir
    pub clear_color: Vec3,                            // Background color
    pub device: Device,                               // GPU device
    pub queue: Queue,                                 // GPU command queue
    pub audio_stream: rodio::OutputStream,            // Audio output stream
    pub draw_queue: Option<DrawQueue>,                // Built draw objects
    pub post_process: Option<PostProcess>,             // Sampler, bindgroup layout, blank texture
    pub resolution: Option<[u32; 2]>,                 // Scene resolution
    pub start_time: Instant,                          // Render loop start time
    pub elapsed_ms: u64,                              // Accumulated time (ms)
    pub projection_matrix: [[f32; 4]; 4],             // Camera view-projection matrix
    pub no_effects: bool,                             // Bypass effects flag
    pub user_params: UserParams,                      // Cursor position for parallax
}
```

### `WgpuApp::new(...) -> Self`

```rust
pub async fn new(
    scene_path: String,
    surface: InitAppSurface,
    size: [u32; 2],
    no_effects: bool,
    assets_path: Option<String>,
) -> Self
```

| Parameter | Description |
|-----------|-------------|
| `scene_path` | Path to `.pkg` file |
| `surface` | Window/display surface (Raw or Winit) |
| `size` | Initial surface dimensions `[width, height]` |
| `no_effects` | Bypass all post-process effects |
| `assets_path` | Optional path to Wallpaper Engine assets/ dir |

Creates the wgpu instance, adapter, device, queue, surface, buffers, and projection bindgroup.

**GPU features required:**
- `TEXTURE_BINDING_ARRAY`
- `SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING`

**Backends:** Vulkan + Metal

**Limits:** `max_binding_array_elements_per_shader_stage: MAX_TEXTURE` (512)

### `WgpuApp::load(&mut self)`

Called after `new()`. Loads scene assets and builds the draw queue:

1. `Scene::new(scene_path)` — parses `.pkg` file (textures, mdls, jsons, misc)
2. Enables lazy-loading fallback if `assets_path` is set
3. `PostProcess::new(device, queue, size)` — sampler + blank texture
4. Creates the default `image_pipeline` from `shader/image.wgsl` (entry points: `vs_main`, `fs_main`)
5. `ObjectMap::with_clear_color(objects, scene, clear_color)` — converts to `TextureObject`/`AudioObject`
6. `DrawQueue::new(...)` — builds GPU draw objects
7. `Projection::new(root).create_camera_uniform()` — camera projection matrix
8. Loads audio via rodio

### `WgpuApp::render(&mut self) -> Option<()>`

Called every frame. Performs:

1. **Time update** — computes delta from `start_time`, wraps `elapsed_ms` at 1 hour for f32 precision
2. **Parallax cursor** — reads `user_params.cursor_position` via `compute_parallax_cursor()`
3. **Uniform write** — calls `render_pass::write_effect_uniforms()` for all draw objects (time, projection, cursor, screen res, texture resolutions, material constants)
4. **Intermediate passes** — if any object has effects, runs `render_intermediate_passes()` (replaces projection with identity for NDC rendering, then restores)
5. **Final pass** — `render_final_pass()` draws all objects to swapchain

### `WgpuApp::resize(&mut self, size: [u32; 2])`

Updates `surface.config` dimensions and reconfigures the swapchain.

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

Creates the wgpu surface from the appropriate handle type:
- `InitAppSurface::Raw` → `unsafe { instance.create_surface_unsafe(SurfaceTargetUnsafe::RawHandle { ... }) }` (wgpu 28.0 API)
- `InitAppSurface::Winit` → `instance.create_surface(window)`

Configures with:
- `RENDER_ATTACHMENT` usage
- `PresentMode::Fifo` (vsync)
- `CompositeAlphaMode::Auto`
- First available capability format
- `desired_maximum_frame_latency: 2`

---

## `buffer` — GPU Buffers

**File:** `buffer.rs`

### `Buffers`

```rust
pub struct Buffers {
    pub vertex: Buffer,     // Vertex data (COPY_DST | VERTEX)
    pub index: Buffer,      // Index data (COPY_DST | INDEX)
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
| `Buffers::draw_rect(&mut self, queue, pos: [Vec3; 4])` | Appends a quad (4 vertices with UV `[0,0]`-`[1,1]`, 6 indices) |
| `Buffers::draw_texture(&mut self, queue, origin, angles, scale, size)` | Creates a rotated quad from transform params (applies rotation via `Mat2::from_angle(angles.z)` around Z), delegates to `draw_rect` |
| `projection` | Buffer for `CameraUniform` struct (projection×view matrix) |

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

Pre-defined fullscreen quad vertices in NDC space `[-1,1]` (top-left UV origin, counter-clockwise winding). Used by ping-pong intermediate passes.

```rust
pub const NDC_VERTICES: [Vertex; 4] = [
    Vertex { pos: [-1.0, 1.0, 0.0], uv: [0.0, 0.0] },  // Top-left
    Vertex { pos: [ 1.0, 1.0, 0.0], uv: [1.0, 0.0] },  // Top-right
    Vertex { pos: [ 1.0, -1.0, 0.0], uv: [1.0, 1.0] }, // Bottom-right
    Vertex { pos: [-1.0, -1.0, 0.0], uv: [0.0, 1.0] }, // Bottom-left
];
```

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
    _fov: f32,
}
```

### `Projection::new(root: &Root) -> Self`

Extracts camera parameters from the scene root (center-eye-target Z, up vector, orthographic clip planes, dimensions).

### `Projection::create_camera_uniform(&self) -> CameraUniform`

Computes the view-projection matrix:
- **View:** `Mat4::look_at_rh(eye, center, up)`
- **Projection:** `Mat4::orthographic_rh(0, w, 0, h, nearz, farz)`
- Result: `(projection × view)` as column-major 4×4 array

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
| `ProjectionBindGroups::new(device) -> Self` | Creates bind group layout (binding 0, VERTEX shader stage, UNIFORM buffer) |
| `create_projection_bindgroup(buffers, device, queue, camera_uniform)` | Creates bind group referencing `buffers.projection` and uploads uniform data |

---

## `post_process` — Sampler & Blank Texture

**File:** `post_process.rs`

```rust
pub struct PostProcess {
    pub sampler: Sampler,           // ClampToEdge, Linear mag, Nearest min
    pub layout: BindGroupLayout,    // Texture(0, fragment) + Sampler(1, fragment)
    pub blank_texture: Texture,     // White/opaque 1.0 dummy texture for unused mask/noise slots
}
```

### `PostProcess::new(device, queue, res) -> Self`

Creates:
- **Sampler** with `ClampToEdge` addressing, `Linear` mag / `Nearest` min filtering
- **Bind group layout** with binding 0 (`Texture2D<f32>, fragment`) and binding 1 (`Sampler, fragment`)
- **Blank texture** at `res` resolution, initialized to white (`0xFF` RGBA) to act as identity mask when no mask texture is bound

---

## `load` — Asset Loading

**File:** `load.rs`

Implements `WgpuApp::load()`. Also contains:

### `load_audios(audio_stream, audios, scene)`

Loads audio files from `scene.misc` using rodio:
- `PlaybackMode::Loop` → `source.repeat_infinite()` added to mixer
- `PlaybackMode::Others` → no playback
- Runs on a spawned thread via `rodio::Sink`

### `create_pipeline(app, bindgroup_layout) -> RenderPipeline`

Creates the default image rendering pipeline:
- WGSL shader from `shader/image.wgsl` (entry points: `vs_main`, `fs_main`)
- Alpha blending: `SrcAlpha / OneMinusSrcAlpha`
- Back-face culling, `Ccw` front face
- Two bind groups: image (0) + projection (1)
- Uses `app.surface.config.format` as the render target format

---

## Shader / Image Pipeline

The default rendering pipeline uses a WGSL shader (`shader/image.wgsl`) for simple texturing:

```wgsl
// Expected bindings:
// Bind group 0, Binding 0: texture2d<f32>    // Source texture
// Bind group 0, Binding 1: sampler            // ClampToEdge, Linear
// Bind group 1, Binding 0: CameraUniform      // Projection×View matrix
```

Effects use GLSL shaders from the `.pkg` file, preprocessed to Vulkan-compatible GLSL before compilation via naga's GLSL frontend.

---

## GPU Requirements

| Feature | Value | Purpose |
|---------|-------|---------|
| `TEXTURE_BINDING_ARRAY` | Required | Texture array indexing |
| `SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING` | Required | Non-uniform texture access |
| `max_binding_array_elements_per_shader_stage` | 512 | Maximum textures per stage |
| Backends | Vulkan + Metal | Cross-platform GPU access |
