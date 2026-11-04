# Core Renderer (`src/scene/renderer/`)

GPU rendering infrastructure: device creation, surface management, buffers, projection, asset loading.

This is the machinery *inside* the library.  The contract with a host
application is the façade ([engine.md](engine.md)); the adapters
([adapters.md](adapters.md)) are the only ones that talk to the renderer
directly.

---

## `app` — WgpuApp

**File:** `app.rs`

The main render orchestrator. Owns all GPU resources and drives the render loop.

### `UserParams` struct

```rust
#[derive(Debug, Clone)]
pub struct UserParams {
    /// Normalized cursor position in [0, 1] range (0,0) = top-left, (1,1) = bottom-right
    pub cursor_position: [f32; 2],
}
```

Adapters can update these params each frame. Default: centered `[0.5, 0.5]` (no parallax shift on Wayland).

### `WgpuApp` struct

```rust
pub struct WgpuApp {
    pub surface: AppSurface,                          // Wgpu surface + config
    pub buffers: Option<Buffers>,                     // Vertex/index/projection buffers
    pub projection_bindgroup: ProjectionBindGroups,   // Camera matrix bindgroup
    pub source: WallpaperSource,                      // The wallpaper that is rendered
    pub title: Option<String>,                        // The title of its project.json
    pub problems: Vec<EngineProblem>,                 // The files that could not be used
    pub assets_path: Option<String>,                  // Wallpaper Engine assets dir
    pub clear_color: Vec3,                            // Background color
    pub device: Device,                               // GPU device
    pub queue: Queue,                                 // GPU command queue
    pub audio_stream: rodio::OutputStream,            // Audio output stream
    pub draw_queue: Option<DrawQueue>,                // Built draw objects
    pub post_process: Option<PostProcess>,            // Sampler, bindgroup layout, blank texture
    pub resolution: Option<[u32; 2]>,                 // Scene resolution
    pub start_time: Instant,                          // Render loop start time
    pub elapsed_ms: u64,                              // Accumulated time (ms)
    pub projection_matrix: [[f32; 4]; 4],             // Camera view-projection matrix
    pub no_effects: bool,                             // Bypass effects flag
    pub no_mdl: bool,                                 // Skip the puppet models
    pub user_params: UserParams,                      // Cursor position for parallax
    pub uniform_staging: Vec<u8>,                     // The staging buffer of the uniforms
    pub has_immediates: bool,                         // The GPU has IMMEDIATES (push constants)
    pub has_clear_texture: bool,                      // The GPU has CLEAR_TEXTURE
    pub has_subgroup: bool,                           // The GPU has SUBGROUP
    pub show_progress: bool,                          // The loading progress is shown
    pub target_fps: Option<u32>,                      // The rate the wallpaper is rendered at
    pub view_scale: [f32; 2],                         // The fit mode, in NDC space
    readback_buffer: Option<Buffer>,                  // The staging buffer of the read-back
}
```

`source` is a `WallpaperSource` ([engine.md](engine.md#sources--wallpapersource)):
an archive and a folder are the same thing here — the loader reads the files of
the source either way.  `title` and `problems` are what the app reports to the
host: the title of the wallpaper and the files that could not be used.

### `WgpuApp::new(...)`

```rust
pub async fn new(
    source: WallpaperSource,
    surface: InitAppSurface,
    size: [u32; 2],
    no_effects: bool,
    no_mdl: bool,
    assets_path: Option<String>,
    show_progress: bool,
) -> Result<Self, EngineError>
```

The constructor takes the wallpaper to render, the surface it is rendered into,
the initial size of the surface and the options of the wallpaper (`no_effects`,
`no_mdl`, `assets_path`, `show_progress`).  It creates:

- the wgpu **instance** (the backends `VULKAN` and `METAL`),
- the **adapter** (`PowerPreference::LowPower`),
- the **device** and the **queue**, with the features and the limits below,
- the **surface** (see [surface](#surface--appsurface)),
- the **projection bind group** and the **audio stream**.

The **features** of the device:

| Feature | Kind | Purpose |
|---------|------|---------|
| `TEXTURE_BINDING_ARRAY` | required | the textures of a scene are bound in an array |
| `SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING` | required | non-uniform access into that array |
| `TEXTURE_COMPRESSION_BC` | required | the BCn compressed textures of Wallpaper Engine |
| `IMMEDIATES` | optional | push constants (the immediates of the effects) |
| `PARTIALLY_BOUND_BINDING_ARRAY` | optional | a binding array with holes |
| `SUBGROUP` | optional | the subgroup operations of the effects |
| `CLEAR_TEXTURE` | optional | the GPU-side clear of a texture |

**Limits:** `max_binding_array_elements_per_shader_stage: MAX_TEXTURE` (512)
and `max_immediate_size` (the limit of the adapter, when `IMMEDIATES` is
available).

An **uncaptured error handler** is installed on the device: wgpu treats
uncaptured errors as fatal and panics, and the release profile is built with
`panic = "abort"`, so a single broken shader would take the whole application
down.  The handler only logs the error, so the rest of the wallpaper keeps
running.

Every step that can fail is reported as an `EngineError` that carries its
cause: an adapter that was not found, a device that could not be created, a
surface that could not be opened and an audio device that is not there.  Nothing
here panics: a machine without a GPU must not take the process down.

### `WgpuApp::load(&mut self) -> Result<(), EngineError>`

Called after `new()`. Loads scene assets and builds the draw queue:

1. `self.source.read_files()` — the files of the wallpaper (an archive or a folder, they are the same thing once they are read), then `Scene::try_new_from_files(files, show_progress, no_mdl)` — parses the wallpaper into the typed `AssetStore`
2. Takes the title of the wallpaper and the files that could not be used over from the scene (`WgpuApp::title` and `WgpuApp::problems`)
3. Enables lazy-loading fallback if `assets_path` is set, then `Scene::prepare()` pre-parses shaders
4. `PostProcess::new(device, queue, size, has_clear_texture)` — sampler + blank texture
5. Creates the default `image_pipeline` and the `copy_pipeline` from `shader/image.wgsl` (entry points: `vs_main`, `fs_main`)
6. `ObjectMap::with_clear_color(objects, scene, clear_color, no_mdl)` — converts to `TextureObject`/`AudioObject`
7. Counts the geometry and allocates the GPU buffers once (`Buffers::new`)
8. `DrawQueue::new(...)` — builds GPU draw objects
9. `Projection::new(root).create_camera_uniform()` — camera projection matrix, and the projection bind group
10. Loads audio via rodio

A wallpaper that cannot be loaded is reported: the reason is logged and the
error is returned, so the caller decides what to do — the command line
interface dies with the trace of the error, the runtime tells the host.  Only
the scene is replaced: the device, the surface and the audio stream stay, so
switching to another wallpaper is cheap.

If the built-in image pipeline cannot be created, the loader logs the error and
returns: the application stays alive, it simply has nothing to draw.

### `WgpuApp::render(&mut self) -> Result<(), RenderError>`

Called every frame. Performs:

1. **Time update** — computes delta from `start_time`, wraps `elapsed_ms` at 1 hour for f32 precision
2. **Parallax cursor** — reads `user_params.cursor_position` via `compute_parallax_cursor()`
3. **Puppet skinning** — `draw::update_animated_meshes()` re-skins the animated puppets and rewrites their vertex range
4. **Video frames** — `draw::update_video_textures()` picks the frame that covers the current time for every animated (video) object and uploads it into its texture (only when the frame changed). A clip that was decoded eagerly is played back by index (nothing is decoded), the other ones are streamed by the decoder
5. **Uniform write** — calls `render_pass::write_effect_uniforms()` for all draw objects (time, cursor, screen res, texture resolutions, material constants)
6. **Intermediate passes** — if any object has effects, runs `render_intermediate_passes()` (the effects work in NDC space, so the identity projection bind group is used)
7. **Final pass** — `render_final_pass()` draws all objects to the render target (the swapchain, or the off-screen texture)

The error is what happened: `RenderError::NotReady` when the scene is not
loaded yet (there is nothing to draw) and `RenderError::Surface` when the
surface did not give a frame to draw into.  A frame that cannot be drawn does
not stop the wallpaper: the frame is skipped and the problem is reported to
the host, which shows it to the user.

### The other methods of `WgpuApp`

| Method | Description |
|--------|-------------|
| `resize(&mut self, size: [u32; 2])` | updates `surface.config` and reconfigures the swapchain (or allocates a new off-screen texture) |
| `set_view_scale(&mut self, sx: f32, sy: f32)` | scales the camera projection in NDC space — this is how the fit modes are implemented |
| `read_offscreen_frame(&mut self, dst: &mut [u8]) -> bool` | copies the off-screen frame into a tightly packed RGBA8 buffer |

---

## `surface` — AppSurface

**File:** `surface.rs`

Wgpu surface wrapper supporting raw handles (Wayland), winit windows and
off-screen textures.

### `InitAppSurface`

```rust
pub enum InitAppSurface {
    Raw((RawDisplayHandle, RawWindowHandle)),  // Wayland
    Winit(Arc<winit::window::Window>),          // Winit
    Offscreen { width: u32, height: u32 },      // GNOME
}
```

Re-exported as `crate::scene::renderer::app::InitAppSurface`.

### `AppSurface`

```rust
pub struct AppSurface {
    pub target: SurfaceTarget,
    pub config: SurfaceConfiguration,
}

pub enum SurfaceTarget {
    Swapchain(Surface<'static>),   // a window swapchain
    Offscreen(OffscreenTarget),     // an off-screen texture
}

pub struct OffscreenTarget {
    pub texture: Texture,
    pub view: TextureView,
    pub format: TextureFormat,
    pub usage: TextureUsages,
}
```

### `AppSurface::new(surface, instance, adapter, device, size) -> Result<Self, wgpu::CreateSurfaceError>`

Creates the wgpu surface from the appropriate handle type:
- `InitAppSurface::Raw` → `unsafe { instance.create_surface_unsafe(SurfaceTargetUnsafe::RawHandle { ... }) }` (wgpu 28.0 API)
- `InitAppSurface::Winit` → `instance.create_surface(window)`
- `InitAppSurface::Offscreen` → an sRGB texture (`Rgba8UnormSrgb`) with `RENDER_ATTACHMENT | COPY_SRC`, so that the bytes that are read back can be handed to a compositor as-is

The surface of a window is created by the graphics driver: when it refuses,
the failure is reported and the caller decides what to do.  Nothing here
panics — the error travels up as `wgpu::CreateSurfaceError` and the adapter
hands it back as an `EngineError::Surface`.

A swapchain is configured with:
- `RENDER_ATTACHMENT` usage
- a present mode that minimises the work of an idle wallpaper: `FifoRelaxed`
  first, then `Fifo` (vsync), then `Mailbox` and `Immediate`, and the first
  supported mode otherwise
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

### `Buffers::new(device, index_capacity, vertex_capacity) -> Self`

Creates pre-allocated GPU buffers, sized for the geometry of the scene (the
loader counts the vertices and the indices before it allocates them, so the
buffers are allocated exactly once).

### Buffer Methods

| Method | Description |
|--------|-------------|
| `Buffers::draw_rect(&mut self, queue, pos: [Vec3; 4], uv_max: Vec2)` | Appends a quad (4 vertices, 6 indices) |
| `Buffers::draw_texture(&mut self, queue, model, size, uv_max)` | Creates a quad from the layer's model matrix (position/rotation/scale/alignment anchor), delegates to `draw_rect` |
| `Buffers::quad_vertices(size, uv_max) -> [Vertex; 4]` | The four object-local vertices of a layer's quad (position + UV) |
| `Buffers::draw_mesh(&mut self, queue, vertices, indices, model)` | Uploads a puppet mesh (MDL units, verbatim) transformed by the model matrix; returns the range of the vertices |
| `Buffers::update_mesh(&mut self, queue, vertex_offset, vertices, model)` | Rewrites a puppet's vertex range after CPU skinning (the index buffer is untouched) |
| `projection` | Buffer for `CameraUniform` struct (projection×view matrix) |

Every layer is pushed one unit towards the camera (`Z_BIAS`): the renderer has
no depth buffer, but the clip volume still has a depth range, and the layers of
Wallpaper Engine live on the z = 0 plane.

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
    Vertex { pos: [ 1.0,-1.0, 0.0], uv: [1.0, 1.0] },  // Bottom-right
    Vertex { pos: [-1.0,-1.0, 0.0], uv: [0.0, 1.0] },  // Bottom-left
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
- Result: `(ortho × translate(eye) × view)` as column-major 4×4 array

The eye translation cancels the lookAt translation, so only the camera's
*rotation* affects the result (identity in practice — Wallpaper Engine's
2D scenes use a camera that looks straight down -z).

> **The scene's world space has its origin in the bottom-left corner**: the
> scene spans `[0, w] × [0, h]`, so the orthographic box is *anchored* at the
> origin (`left = 0`, `bottom = 0`) instead of being centred on it.
>
> Verified against real `scene.json` files: the full-screen layer of a
> wallpaper is always at `(w/2, h/2)` with `size = (w, h)` — e.g. `origin
> "1920 1080"` / `size "3840 2160"` for a 4K scene — i.e. the layer covers
> `[0, w] × [0, h]`.  A *centred* box would push the whole scene into the
> top-right quadrant of the screen.

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
    /// Identity matrix bind group for intermediate (NDC-space) passes.
    pub identity: Option<BindGroup>,
    /// Separate buffer holding identity matrix, so intermediate passes
    /// don't need to overwrite the main projection buffer.
    pub identity_buffer: Buffer,
}
```

| Method | Description |
|--------|-------------|
| `ProjectionBindGroups::new(device) -> Self` | Creates bind group layout (binding 0, VERTEX shader stage, UNIFORM buffer) and the identity buffer |
| `create_projection_bindgroup(buffers, device, queue, camera_uniform)` | Creates bind group referencing `buffers.projection` and uploads uniform data |

---

## `post_process` — Sampler & Blank Texture

**File:** `post_process.rs`

```rust
pub struct PostProcess {
    pub sampler: Sampler,           // ClampToEdge, Linear mag / min
    pub layout: BindGroupLayout,    // Texture(0, fragment) + Sampler(1, fragment)
    pub blank_texture: Texture,     // White/opaque 1.0 dummy texture for unused mask/noise slots
    pub has_clear_texture: bool,    // Whether CLEAR_TEXTURE is available (GPU-side texture clear)
}
```

### `PostProcess::new(device, queue, res, has_clear_texture) -> Self`

Creates:
- **Sampler** with `ClampToEdge` addressing, `Linear` mag / min filtering and `Linear` mipmap filtering
- **Bind group layout** with binding 0 (`Texture2D<f32>, fragment`) and binding 1 (`Sampler, fragment`)
- **Blank texture** at `res` resolution, initialized to white (`0xFF` RGBA) to act as identity mask when no mask texture is bound

---

## `load` — Asset Loading

**File:** `load.rs`

Implements `WgpuApp::load()`. Also contains:

### `load_audios(audio_stream, audios, scene)`

Loads audio files from `scene.assets` (via `take_sound`) using rodio:
- `PlaybackMode::Loop` → `source.repeat_infinite()` added to mixer
- `PlaybackMode::Others` → no playback
- Runs on a spawned thread via `rodio::Sink`

### `create_pipelines(app, bindgroup_layout) -> Option<(RenderPipeline, RenderPipeline)>`

Creates the two default pipelines from the WGSL shader `shader/image.wgsl`
(entry points: `vs_main`, `fs_main`):

- the **image pipeline**: alpha blending `SrcAlpha / OneMinusSrcAlpha`,
- the **copy pipeline**: no blending (`One / One`), used to copy a frame into
  the intermediate target.

Both use `app.surface.config.format` as the render target format, the
`TriangleList` topology, the `Ccw` front face, no culling and no depth
stencil.  Their pipeline layout holds two bind groups: the texture (0) and the
projection (1).  The pipeline is built inside a wgpu validation error scope
(see `error_scope.rs`), so a broken shader is reported and the application
stays alive.

---

## Shader / Image Pipeline

The default rendering pipeline uses a WGSL shader (`src/scene/renderer/shader/image.wgsl`) for simple texturing:

```wgsl
// Expected bindings:
// @group(0) @binding(0) var tex: texture_2d<f32>;    // Source texture
// @group(0) @binding(1) var tex_sampler: sampler;    // ClampToEdge, Linear
// @group(1) @binding(0) var<uniform> projection_matrix: mat4x4f;
```

Effects use GLSL shaders from the wallpaper, preprocessed to
Vulkan-compatible GLSL before compilation via naga's GLSL frontend (see
[loader.md](loader.md)).

---

## GPU Requirements

| Feature | Value | Purpose |
|---------|-------|---------|
| `TEXTURE_BINDING_ARRAY` | Required | Texture array indexing |
| `SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING` | Required | Non-uniform texture access |
| `TEXTURE_COMPRESSION_BC` | Required | The BCn compressed textures |
| `IMMEDIATES`, `PARTIALLY_BOUND_BINDING_ARRAY`, `SUBGROUP`, `CLEAR_TEXTURE` | Optional | Used when the adapter has them |
| `max_binding_array_elements_per_shader_stage` | 512 | Maximum textures per stage |
| Backends | Vulkan + Metal | Cross-platform GPU access |
