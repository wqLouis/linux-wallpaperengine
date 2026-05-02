# Core Renderer (`src/scene/renderer/`)

GPU rendering infrastructure: device creation, surface management, buffers, projection, asset loading.

---

## `app` — WgpuApp

**File:** `app.rs`

The main render orchestrator. Owns all GPU resources and drives the render loop.

### Public API

#### `WgpuApp`

```rust
pub struct WgpuApp {
    pub surface: AppSurface,
    pub buffers: Buffers,
    pub projection_bindgroup: ProjectionBindGroups,
    pub clear_color: Vec3,
    pub device: Device,
    pub queue: Queue,
    pub draw_queue: Option<DrawQueue>,
    pub post_process: Option<PostProcess>,
    pub resolution: Option<[u32; 2]>,
    pub projection_matrix: [[f32; 4]; 4],
    // ... internal fields
}
```

#### `WgpuApp::new(scene_path, surface, size) -> Self`

| Parameter | Type | Description |
|-----------|------|-------------|
| `scene_path` | `String` | Path to `.pkg` file |
| `surface` | `InitAppSurface` | Window/display surface |
| `size` | `[u32; 2]` | Initial surface dimensions |

Creates the wgpu instance, adapter, device, queue, surface, buffers, and projection bindgroup.

**Required GPU features:**
- `TEXTURE_BINDING_ARRAY`
- `SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING`

**Backends:** Vulkan + Metal.

#### `WgpuApp::load(&mut self)`

Called after `new()` — loads scene assets and builds the draw queue:

1. `Scene::new(path)` — parses .pkg
2. `PostProcess::new(device, size)` — sampler + blank textures
3. Creates the default `image_pipeline`
4. `ObjectMap::new(objects, scene)` — converts to TextureObject/AudioObject
5. `DrawQueue::new(...)` — builds GPU draw objects
6. `Projection::new(root).create_camera_uniform()` — camera
7. Loads audio via rodio

#### `WgpuApp::render(&mut self) -> Option<()>`

Called every frame:

1. Writes effect uniforms (time, projection, effect parameters)
2. If any objects have effects: runs intermediate ping-pong passes
3. Runs final pass — draws all objects to swapchain, calls `present()`

#### `WgpuApp::resize(&mut self, size: [u32; 2])`

Updates surface config dimensions and reconfigures the swapchain.

#### `write_effect_uniforms(queue, objects, elapsed, projection, screen_res)`

Public free function (used by `intermediate_pass`). Writes per-effect GPU uniform buffers with time, projection matrix, screen resolution, texture resolutions, and material constants.

---

## `surface` — AppSurface

**File:** `surface.rs`

Wgpu surface wrapper supporting both raw handles (Wayland) and winit windows.

#### `InitAppSurface`

```rust
pub enum InitAppSurface {
    Raw((RawDisplayHandle, RawWindowHandle)),  // Wayland
    Winit(Arc<winit::window::Window>),          // Winit
}
```

Re-exported as `crate::scene::renderer::app::InitAppSurface`.

#### `AppSurface`

```rust
pub struct AppSurface {
    pub surface: Surface<'static>,
    pub config: SurfaceConfiguration,
}
```

#### `AppSurface::new(surface, instance, adapter, size) -> Self`

Creates the wgpu surface from the appropriate handle type. Configures with:
- `RENDER_ATTACHMENT` usage
- `Mailbox` present mode
- `Auto` alpha mode
- First available capability format

---

## `buffer` — GPU Buffers

**File:** `buffer.rs`

#### `Buffers`

```rust
pub struct Buffers {
    pub vertex: Buffer,        // Vertex data (COPY_DST | VERTEX)
    pub index: Buffer,         // Index data (COPY_DST | INDEX)
    pub projection: Buffer,    // Camera uniform (COPY_DST | UNIFORM)
    pub vertex_len: u32,
    pub index_len: u32,
}
```

#### `Buffers::new(device, index_len, vertex_len) -> Self`

Creates pre-allocated GPU buffers sized for maximum texture count.

#### `Buffers::draw_rect(&mut self, queue, pos: [Vec3; 4])`

Appends a quad (4 vertices, 6 indices) to the vertex/index buffers. Used during scene loading to build the draw list.

#### `Buffers::draw_texture(&mut self, queue, origin, angles, scale, size: Vec2)`

Computes 2D rotation and offset, then delegates to `draw_rect`. Each texture object gets one quad in the global buffer.

---

## `vertex` — Vertex Type

**File:** `vertex.rs`

```rust
#[repr(C)]
pub struct Vertex {
    pub pos: [f32; 3],     // World-space position
    pub uv: [f32; 2],      // Texture coordinates
}
```

#### `Vertex::create_buffer_layout() -> VertexBufferLayout`

Returns a standard layout: location 0 = Float32x3 (position), location 1 = Float32x2 (UV).

#### `NDC_VERTICES`

Pre-defined fullscreen quad vertices in NDC space `[-1,1]`. Used by ping-pong intermediate passes.

---

## `projection` — Camera System

**File:** `projection.rs`

### `Projection`

```rust
pub struct Projection {
    center: Vec3, eye: Vec3, up: Vec3,
    nearz: f32, farz: f32,
    width: f32, height: f32,
}
```

#### `Projection::new(root: &Root) -> Self`

Extracts camera parameters from the scene root (center, eye positions, clip planes, orthographic dimensions).

#### `Projection::create_camera_uniform(&self) -> CameraUniform`

Computes the view-projection matrix:
- View: `Mat4::look_at_rh(eye, center, up)`
- Projection: `Mat4::orthographic_rh(0, w, 0, h, nearz, farz)`

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

#### `ProjectionBindGroups::new(device) -> Self`

Creates the bind group layout for the projection uniform (binding 0, vertex visibility).

#### `ProjectionBindGroups::create_projection_bindgroup(&mut self, buffers, device, queue, camera_uniform)`

Creates the actual bind group and uploads uniform data.

---

## `post_process` — Samplers & Blank Textures

**File:** `post_process.rs`

```rust
pub struct PostProcess {
    pub sampler: Sampler,              // ClampToEdge, Linear/Nearest
    pub layout: BindGroupLayout,       // Texture + Sampler at bindings 0,1
    pub blank_texture: Texture,        // Dummy texture for unused slots
    // ... internal fields
}
```

#### `PostProcess::new(device, res) -> Self`

Creates:
- A **sampler** with `ClampToEdge` addressing, `Linear` mag / `Nearest` min filtering
- A **bind group layout** with binding 0 (Texture2D, fragment) and binding 1 (Sampler, fragment)
- **Blank textures** at the specified resolution for fallback usage

---

## `load` — Asset Loading

**File:** `load.rs`

Implements `WgpuApp::load()` as described above. Also contains:

#### `load_audios(audio_stream, audios, scene)`

Loads audio files from `scene.misc` using rodio:
- `PlaybackMode::Loop` → `source.repeat_infinite()`
- Playback runs on a spawned thread via `Sink::sleep_until_end()`

#### `create_pipeline(app, bindgroup_layout) -> RenderPipeline`

Creates the default image rendering pipeline:
- WGSL shader from `shader/image.wgsl`
- Alpha blending: `SrcAlpha / OneMinusSrcAlpha`
- Back-face culling
- Two bind groups: image (0) + projection (1)
