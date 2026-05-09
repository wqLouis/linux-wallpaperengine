# Core Renderer (`src/scene/renderer/`)

GPU rendering infrastructure: device creation, surface management, buffers, projection, asset loading.

---

## `app` — WgpuApp

The main render orchestrator. Owns all GPU resources and drives the render loop.

### `WgpuApp::new(scene_path, surface, size, no_effects) -> Self`

| Parameter | Type | Description |
|-----------|------|-------------|
| `scene_path` | `String` | Path to the `.pkg` file |
| `surface` | `InitAppSurface` | Window/display surface (Raw or Winit) |
| `size` | `[u32; 2]` | Initial surface dimensions `[width, height]` |
| `no_effects` | `bool` | Skip all post-process effects |

Creates the wgpu instance, adapter, device, queue, surface, GPU buffers, and projection bind group layout.

**GPU requirements:** Vulkan or Metal backend, `TEXTURE_BINDING_ARRAY` and `SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING` features.

### `WgpuApp::load(&mut self)`

Called after `new()`. Loads scene assets and builds the draw queue:

1. `Scene::new(scene_path)` — parse `.pkg`
2. `PostProcess::new(device, size)` — create sampler, bind group layout, blank texture
3. `ObjectMap::new(objects, scene)` — convert to texture/audio objects
4. `DrawQueue::new(...)` — build GPU draw objects and effect pipelines
5. `Projection::new(root).create_camera_uniform()` — compute camera matrix
6. Load audio via rodio

### `WgpuApp::render(&mut self) -> Option<()>`

Called every frame:

1. **Time update** — compute delta, wrap elapsed at 1 hour for f32 precision
2. **Uniform upload** — write time, projection, effect parameters to GPU
3. **Intermediate passes** — multi-pass effect rendering if any object has effects
4. **Final pass** — draw all objects to swapchain

Returns `None` if draw queue or post-process is not yet loaded, or if a recoverable swapchain error occurs (surface lost, timeout).

### `WgpuApp::resize(&mut self, size: [u32; 2])`

Reconfigures the swapchain with new dimensions.

---

## `surface` — AppSurface

Wraps a WGPU surface for both raw handles (Wayland) and winit windows.

### `InitAppSurface`

```rust
pub enum InitAppSurface {
    Raw((RawDisplayHandle, RawWindowHandle)),  // Wayland
    Winit(Arc<winit::window::Window>),          // Winit
}
```

### `AppSurface::new(surface, instance, adapter, size) -> Self`

| Parameter | Type | Description |
|-----------|------|-------------|
| `surface` | `InitAppSurface` | Raw handles or winit window |
| `instance` | `&Instance` | WGPU instance |
| `adapter` | `&Adapter` | WGPU adapter |
| `size` | `[u32; 2]` | Initial dimensions |

Configures with `RENDER_ATTACHMENT` usage, `Mailbox` present mode, `Auto` alpha mode, and the first available format.

---

## `buffer` — GPU Buffers

### `Buffers`

| Field | Type | Usage |
|-------|------|-------|
| `vertex` | `Buffer` | `COPY_DST \| VERTEX` |
| `index` | `Buffer` | `COPY_DST \| INDEX` |
| `projection` | `Buffer` | `COPY_DST \| UNIFORM` |

| Method | Description |
|--------|-------------|
| `Buffers::new(device, index_len, vertex_len)` | Pre-allocates buffers sized for max texture count |
| `Buffers::draw_rect(queue, pos: [Vec3; 4])` | Appends a quad (4 vertices, 6 indices) |
| `Buffers::draw_texture(queue, origin, angles, scale, size)` | Creates rotated quad, delegates to `draw_rect` |

---

## `vertex` — Vertex Type

| Attribute | Format | Location |
|-----------|--------|----------|
| `pos` | `Float32x3` | 0 |
| `uv` | `Float32x2` | 1 |

Fullscreen NDC quad vertices (used by ping-pong passes) are defined as `NDC_VERTICES`.

---

## `projection` — Camera System

### `Projection::new(root) -> Self`

Extracts camera parameters from the scene root (center, eye, up, clip planes, orthographic dimensions).

### `Projection::create_camera_uniform(&self) -> CameraUniform`

Computes view-projection matrix:
- **View:** `look_at_rh(eye, center, up)`
- **Projection:** `orthographic_rh(0, w, 0, h, nearz, farz)`

### `CameraUniform`

| Field | Type | Description |
|-------|------|-------------|
| `projection` | `[[f32; 4]; 4]` | Combined view-projection matrix |

### `ProjectionBindGroups`

| Method | Description |
|--------|-------------|
| `ProjectionBindGroups::new(device)` | Creates bind group layout (binding 0 = uniform) |
| `create_projection_bindgroup(buffers, device, queue, uniform)` | Creates bind group and uploads camera matrix |

---

## `post_process` — Samplers & Blank Textures

### `PostProcess::new(device, res) -> Self`

| Parameter | Type | Description |
|-----------|------|-------------|
| `device` | `&Device` | WGPU device |
| `res` | `[u32; 2]` | Resolution for the blank texture |

Creates:
- **Sampler:** `ClampToEdge` addressing, `Linear` mag / `Nearest` min filter
- **Bind group layout:** texture (binding 0) + sampler (binding 1)
- **Blank texture:** white, used as fallback when no mask/noise texture is bound

---

## `load` — Asset Loading

Implements `WgpuApp::load()` as described above.

### `load_audios(audio_stream, audios, scene)`

Loads audio from scene misc data via rodio. `PlaybackMode::Loop` sources are set to repeat infinitely. Playback runs on a spawned thread.

### `create_pipeline(app, bindgroup_layout) -> RenderPipeline`

Creates the default image pipeline from `shader/image.wgsl` with:
- Alpha blending: `SrcAlpha / OneMinusSrcAlpha`
- Back-face culling
- Two bind groups: image (0) + projection (1)
