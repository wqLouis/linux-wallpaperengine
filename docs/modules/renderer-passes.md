# Render Passes (`src/scene/renderer/`)

Draw queue, effect bind groups, ping-pong textures, multi-pass rendering, and the final render pass.

---

## `draw` — DrawQueue & DrawObject

**File:** `draw.rs`

### `DrawObject`

A single drawable item in the scene.

```rust
pub struct DrawObject {
    pub texture_object: TextureObject,                    // Source texture & transform
    pub index_range: [u32; 2],                           // Range in global index buffer
    pub bindgroup: BindGroup,                            // Texture + sampler (bindings 0, 1)
    pub pipelines: Vec<Rc<RenderPipeline>>,               // Effect pipelines (1 per effect)
    pub effect_bindgroups: Vec<EffectBindGroup>,          // Per-effect GPU resources
    pub intermediates: Option<PingPongTextures>,           // For multi-effect rendering
}
```

#### `DrawObject::build(...)` (private)

Called by `DrawQueue::new()` for each texture object:

1. **Resolves effect pipelines** — calls `get_or_create_pipeline` for each effect, using cache key based on effect path + mask/noise texture presence + combo values
2. **Uploads texture** — creates a GPU texture from `.tex` payload, selecting `R8Unorm`, `Rg8Unorm`, or `Rgba8UnormSrgb` based on extension
3. **Creates source bindgroup** — texture view + sampler (bindings 0, 1) using `post_process.layout`
4. **Loads mask & noise textures** — loads from `scene.textures` with `materials/` prefix fallback, uploaded as `R8Unorm`, `Rg8Unorm`, or `Rgba8Unorm`
5. **Builds effect bind groups** — for each effect: creates uniform buffer if needed, builds `tex_resolutions` map for all sampler slots, creates bindgroup
6. **Creates ping-pong textures** (if effects present) — two render targets sized to `max(texture_dim, screen_dim)`
7. **Appends geometry** — calls `Buffers::draw_texture()` to add rotated quad to global VB/IB

### `DrawQueue`

```rust
pub struct DrawQueue {
    pub queue: Rc<Vec<DrawObject>>,                       // Ordered draw list
    pub render_pipelines: BTreeMap<String, EffectPipelineData>, // Effect cache
    pub image_pipeline: RenderPipeline,                   // Default image shader
}
```

#### `DrawQueue::new(...) -> Self`

Builds draw objects for all texture objects. The `render_pipelines` map is shared across objects to cache effect pipeline compilation.

---

## `render_pass` — Final Pass & Uniform Writing

**File:** `render_pass.rs`

### `render_final_pass(...)`

Draws all objects to the swapchain surface.

```rust
pub fn render_final_pass(
    device: &Device, queue: &Queue, surface: &AppSurface,
    buffers: &Buffers, projection_bindgroup: &ProjectionBindGroups,
    draw_queue: &DrawQueue, post_process: &PostProcess,
    clear_color: Vec3,
) -> Option<()>
```

**Behavior:**
1. Acquires the next swapchain texture (handles `Lost`, `Outdated`, `Timeout` errors by reconfiguring)
2. Creates a command encoder and begins a render pass with `clear_color` (divided by 255.0 for GPU)
3. For each `DrawObject`:
   - Uses the intermediate ping-pong result (`view_a`) if effects present, otherwise the original `bindgroup`
   - Sets `image_pipeline` for all objects
   - Sets vertex/index buffers and projection bindgroup
   - Draws indexed geometry (`draw_object.index_range`)
4. Submits and presents

### `write_effect_uniforms(...)`

Writes per-frame uniform data into all effect bind group buffers.

```rust
pub fn write_effect_uniforms(
    queue: &Queue, objects: &[DrawObject],
    elapsed: f32, projection: &[[f32; 4]; 4],
    screen_res: [u32; 2], user_params: &UserParams,
)
```

For each draw object's effects:
1. Allocates a staging buffer sized to `uniform_layout.total_size()`
2. Fills it with `UniformLayout::populate_effect_params(...)`:
   - `g_Time` — elapsed seconds (wrapped to 1 hour)
   - `g_ModelViewProjectionMatrix` — projection matrix
   - `g_Screen` — `[width, height, aspect_ratio]`
   - `g_EffectTextureProjectionMatrix` — projection matrix
   - `g_EffectTextureProjectionMatrixInverse` — identity
   - `g_ParallaxPosition` — cursor position from `user_params`
   - `g_TextureNResolution` — from `tex_resolutions`
   - Material constants — resolved from `constants` map via `material_keys`
3. Uploads to the GPU uniform buffer via `queue.write_buffer()`

---

## `effect_bindgroup` — EffectBindGroup

**File:** `effect_bindgroup.rs`

Per-effect GPU resources for a single effect pass.

```rust
pub struct EffectBindGroup {
    pub pipeline: Rc<RenderPipeline>,           // Compiled effect shader
    pub bindgroup: BindGroup,                   // Textures + sampler + uniforms
    pub uniform_buffer: Option<Buffer>,         // Effect parameter uniform buffer
    pub uniform_layout: UniformLayout,          // Layout for uniform writes
    pub material_keys: BTreeMap<String, String>, // material key → uniform name mapping
    pub constants: BTreeMap<String, Value>,     // Material constant overrides
    pub tex_resolutions: BTreeMap<String, [f32; 4]>, // g_TextureNResolution values
    pub blank_view: TextureView,                // Fallback view for unused sampler slots
    pub mask_view: Option<TextureView>,         // Mask texture view (slot 1)
    pub noise_view: Option<TextureView>,        // Noise texture view (slot 2)
    pub _mask_tex: Option<Texture>,             // Keeps mask texture alive
    pub _noise_tex: Option<Texture>,            // Keeps noise texture alive
}
```

### `EffectBindGroup::new(...) -> Option<Self>`

Constructs all GPU resources for an effect:

1. Creates a uniform buffer if the effect has uniforms (declared in `uniform_decls`)
2. Creates bind group entries:
   - Texture bindings at `binding = i × 2` for each sampler slot
   - Slot selection: `0 = source`, `1 = mask`, `2 = noise`, others = `blank_view`
   - Sampler at `WM_SAMPLER_BINDING` (binding 1)
   - Uniform buffer binding at `sampler_count × 2 + 2` if uniforms exist
3. Creates the bind group from `pipedata.bindgroup_layout`

### `make_effect_intermediate_bindgroup(...)` (free function)

Creates a temporary bind group for intermediate passes, replacing the source texture view with the ping-pong output. Used during multi-effect ping-pong rendering.

---

## `ping_pong` — PingPongTextures

**File:** `ping_pong.rs`

Double-buffered render targets for multi-effect objects.

```rust
pub struct PingPongTextures {
    pub tex_a: Texture,       // Render target A (Rgba8UnormSrgb)
    pub tex_b: Texture,       // Render target B
    pub view_a: TextureView,
    pub view_b: TextureView,
    pub ndc_vbuf: Buffer,     // Fullscreen quad vertex buffer (NDC_VERTICES)
    pub ndc_ibuf: Buffer,     // Fullscreen quad index buffer ([0,2,1, 0,3,2])
}
```

### `PingPongTextures::new(device, queue, post_process, width, height) -> Self`

Creates twin render targets sized to the texture dimensions (scaled up to max texture vs screen), pre-filled with NDC quad geometry.

### Methods

| Method | Description |
|--------|-------------|
| `make_bindgroup(device, layout, sampler, view)` | Creates bindgroup referencing given view + sampler (texture binding 0, sampler binding 1) |

---

## `intermediate_pass` — Multi-Effect Rendering

**File:** `intermediate_pass.rs`

Orchestrates the multi-pass rendering for objects with effects.

### `render_intermediate_passes(...)`

```rust
pub fn render_intermediate_passes(
    device, queue, buffers, projection_bindgroup,
    projection_matrix, draw_queue, post_process,
    elapsed, screen_res, user_params,
)
```

**Flow:**

1. **Writes identity projection** — temporarily overrides the projection buffer with identity matrix for NDC rendering
2. **Uploads uniforms** with identity projection
3. **For each draw object with intermediate textures:**
   - **Source pass** — renders the original texture to `view_a` using `image_pipeline`
   - **Effect passes** — for each effect in order:
     - Renders to current target (`view_a` or `view_b`)
     - Applies the effect shader pipeline
     - Creates intermediate bindgroup via `make_effect_intermediate_bindgroup` (replacing source with previous output)
     - Swaps source/target each iteration
   - **Copy-back** (if odd effect count) — copies the final result back to `view_a` for the final pass
4. **Restores projection** — writes original projection back to buffer
5. **Re-uploads uniforms** with the real projection matrix

---

## Render Pass Flow

```
┌─────────────────────────────────┐
│  write_effect_uniforms()        │ ← Time, projection, cursor, effect params → GPU
└─────────────────────────────────┘
              │
     ┌────────┴────────┐
     │ Has effects?    │
     ├─ Yes ───────────┤
     │                 ▼
     │  render_intermediate_passes()
     │  ┌──────────────────────────────────┐
     │  │ 1. Write identity projection     │
     │  │ 2. Upload uniforms (identity)    │
     │  │ 3. For each object:              │
     │  │    a. Source → view_a            │
     │  │    b. Effect0: view_a → view_b   │
     │  │       Effect1: view_b → view_a   │
     │  │       ...                        │
     │  │    c. Copy-back if odd count     │
     │  │ 4. Restore original projection   │
     │  │ 5. Re-upload uniforms (real)     │
     │  └──────────────────────────────────┘
     │                 │
     └─ No ────────────┘
              │
              ▼
     ┌───────────────────┐
     │  render_final_pass│ ← Single pass → swapchain
     │  present()        │
     └───────────────────┘
```

---

## Vertex Buffer Layout

Each `Vertex` in the global vertex buffer:

```rust
#[repr(C)]
pub struct Vertex {
    pub pos: [f32; 3],  // World-space position (location 0)
    pub uv: [f32; 2],   // Texture coordinates (location 1)
}
```

### NDC Quad (used in intermediate passes)

```rust
pub const NDC_VERTICES: [Vertex; 4] = [
    Vertex { pos: [-1.0,  1.0, 0.0], uv: [0.0, 0.0] },
    Vertex { pos: [ 1.0,  1.0, 0.0], uv: [1.0, 0.0] },
    Vertex { pos: [ 1.0, -1.0, 0.0], uv: [1.0, 1.0] },
    Vertex { pos: [-1.0, -1.0, 0.0], uv: [0.0, 1.0] },
];
```

### Quad indices

```rust
[0, 2, 1, 0, 3, 2]  // Two triangles, counter-clockwise winding
```

### Buffer Sizes

```rust
pub const MAX_TEXTURE: u32 = 512;
pub const MAX_VERTEX: u32 = MAX_TEXTURE * 4;   // 2048 vertices
pub const MAX_INDEX: u32 = MAX_TEXTURE * 6;    // 3072 indices
```

These limits determine the maximum number of texture objects that can be loaded.

### Texture Upload Format Selection

| Extension | GPU Format | BPP |
|-----------|-----------|-----|
| `r8` | `R8Unorm` | 1 |
| `rg88` | `Rg8Unorm` | 2 |
| any other | `Rgba8UnormSrgb` | 4 |
