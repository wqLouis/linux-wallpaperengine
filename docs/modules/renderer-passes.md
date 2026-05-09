# Render Passes (`src/scene/renderer/`)

Draw queue, effect bind groups, ping-pong textures, and multi-pass rendering.

---

## `draw` — DrawQueue & DrawObject

**File:** `draw.rs`

### `DrawObject`

A single drawable item in the scene.

```rust
pub struct DrawObject {
    pub texture_object: TextureObject,
    pub index_range: [u32; 2],                   // Range in global index buffer
    pub bindgroup: BindGroup,                    // Texture + sampler (bindings 0, 1)
    pub pipelines: Vec<Rc<RenderPipeline>>,      // Effect pipelines (1 per effect)
    pub effect_bindgroups: Vec<EffectBindGroup>, // Per-effect GPU resources
    pub intermediates: Option<PingPongTextures>,  // For multi-effect rendering
}
```

#### `DrawObject::build(...)` (private)

Called by `DrawQueue::new()` for each texture object:

1. **Resolves effect pipelines** — calls `get_or_create_pipeline` for each effect
2. **Uploads texture** — creates a `Rgba8UnormSrgb` texture from `.tex` payload
3. **Creates source bindgroup** — texture view + sampler (bindings 0, 1)
4. **Builds effect bind groups** — for each effect: loads mask/noise textures, creates uniform buffers, sets up bind groups
5. **Creates ping-pong textures** (if effects present) — two render targets for multi-pass rendering
6. **Appends geometry** — calls `Buffers::draw_texture()` to add quad to global VB/IB

### `DrawQueue`

```rust
pub struct DrawQueue {
    pub queue: Rc<Vec<DrawObject>>,              // Ordered draw list
    pub render_pipelines: BTreeMap<String, EffectPipelineData>, // Effect cache
    pub image_pipeline: RenderPipeline,          // Default image shader
}
```

#### `DrawQueue::new(...) -> Self`

Builds draw objects for all texture objects. The `render_pipelines` map is shared across objects to cache effect pipeline compilation.

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
    pub material_keys: BTreeMap<String, String>, // material key → uniform name
    pub constants: BTreeMap<String, Value>,     // Material constant overrides
    pub tex_resolutions: BTreeMap<String, [f32; 4]>, // g_TextureNResolution values
}
```

### `EffectBindGroup::new(...) -> Option<Self>`

Constructs all GPU resources for an effect:

1. Creates a uniform buffer if the effect has uniforms
2. Creates bind group entries for each sampler (texture 0 = source, 1 = mask, 2 = noise, others = blank)
3. Adds the `_wm_sampler` sampler binding
4. Adds the uniform buffer binding if present

### `make_effect_intermediate_bindgroup(device, pipedata, effect_bg, source_view, sampler) -> BindGroup`

Free function. Creates a temporary bind group for intermediate passes, replacing the source texture view with the ping-pong output. Used during multi-effect ping-pong rendering.

---

## `ping_pong` — PingPongTextures

**File:** `ping_pong.rs`

Double-buffered render targets for multi-effect objects.

```rust
pub struct PingPongTextures {
    pub tex_a: Texture,     // Render target A (Rgba8UnormSrgb)
    pub tex_b: Texture,     // Render target B
    pub view_a: TextureView,
    pub view_b: TextureView,
    pub bindgroup: BindGroup,  // view_a + sampler
    pub ndc_vbuf: Buffer,      // Fullscreen quad vertex buffer
    pub ndc_ibuf: Buffer,      // Fullscreen quad index buffer
}
```

### `PingPongTextures::new(device, queue, post_process, width, height) -> Self`

Creates twin render targets sized to the texture dimensions, pre-filled with NDC quad geometry. The NDC quad is a fullscreen triangle strip covering `[-1,1]`.

### Bindgroup Methods

| Method | Description |
|--------|-------------|
| `make_bindgroup(device, layout, sampler)` | Returns bindgroup referencing `view_a` + sampler |
| `make_bindgroup_for(device, layout, sampler, view)` | Returns bindgroup for a specific view + sampler |
| `make_source_bindgroup(device, layout, view, sampler)` | Low-level: texture (0) + sampler (1) |

---

## `intermediate_pass` — Multi-Effect Rendering

**File:** `intermediate_pass.rs`

Orchestrates the multi-pass rendering for objects with effects.

### `render_intermediate_passes(...)`

Called when any draw object has effects. For each object:

1. **Writes identity projection** — temporarily overrides projection buffer for NDC rendering
2. **Source pass** — renders the original texture to `view_a` using `image_pipeline`
3. **Effect passes** — for each effect in order:
   - Renders to current target (`view_a` or `view_b`)
   - Applies the effect shader (`effect_bg.pipeline`)
   - Uses the previous pass output as source texture
   - Swaps source/target each iteration
4. **Copy-back** (if odd effect count) — copies the final result back to `view_a` for the final pass
5. **Restores projection** — writes original projection back to buffer

---

## Render Pass Flow

```
┌─────────────────────────────────┐
│  write_effect_uniforms()        │ ← Time, projection, effect params → GPU
└─────────────────────────────────┘
              │
     ┌────────┴────────┐
     │ Has effects?    │
     ├─ Yes ───────────┤
     │                 ▼
     │  render_intermediate_passes()
     │  ┌─────────────────────────┐
     │  │ Write identity projection
     │  │ Source → view_a        │
     │  │ Effect0: view_a → view_b│
     │  │ Effect1: view_b → view_a│
     │  │ ...                     │
     │  │ Copy-back if odd count  │
     │  │ Restore original projection
     │  └─────────────────────────┘
     │                 │
     └─ No ────────────┘
              │
              ▼
     ┌───────────────────┐
     │  render_final_pass│ ← Single pass → swapchain
     │  present()        │
     └───────────────────┘
```

### Intermediate Pass Details

The intermediate pass handles the projection matrix specially:

1. **Before intermediate passes:** Projection buffer is overwritten with identity matrix, and `write_effect_uniforms` is called with identity
2. **After intermediate passes:** Original projection is restored, `write_effect_uniforms` is called again with the real matrix

This ensures NDC-space fullscreen quad rendering works correctly during effect passes.

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

The NDC quad vertices (used in intermediate passes):

```rust
const NDC_VERTICES: [[f32; 3]; 4] = [
    [-1.0, -1.0, 0.0],
    [ 1.0, -1.0, 0.0],
    [-1.0,  1.0, 0.0],
    [ 1.0,  1.0, 0.0],
];
```

### Buffer Sizes

```rust
pub const MAX_TEXTURE: u32 = 512;
pub const MAX_VERTEX: u32 = MAX_TEXTURE * 4;   // 2048 vertices
pub const MAX_INDEX: u32 = MAX_TEXTURE * 6;    // 3072 indices
```

These limits determine the maximum number of texture objects that can be loaded.