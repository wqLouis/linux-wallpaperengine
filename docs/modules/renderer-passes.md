# Render Passes (`src/scene/renderer/`)

Draw queue, effect bind groups, ping-pong textures, and multi-pass rendering.

---

## `draw` — DrawQueue & DrawObject

### `DrawObject`

| Field | Type | Description |
|-------|------|-------------|
| `texture_object` | `TextureObject` | Source scene object (kept alive) |
| `index_range` | `[u32; 2]` | Range into the global index buffer `[start, end)` |
| `bindgroup` | `BindGroup` | Texture + sampler (bindings 0, 1) |
| `pipelines` | `Vec<Rc<RenderPipeline>>` | Compiled effect pipelines (kept alive) |
| `effect_bindgroups` | `Vec<EffectBindGroup>` | Per-effect GPU resources |
| `intermediates` | `Option<PingPongTextures>` | Ping-pong textures for multi-pass effects |

Internally, `DrawObject::build()`:
1. Resolves effect pipelines via `get_or_create_pipeline`
2. Uploads texture as `Rgba8UnormSrgb`
3. Creates source bind group (texture view + sampler)
4. Builds effect bind groups (mask/noise textures, uniform buffers)
5. Creates ping-pong textures if effects are present
6. Appends quad geometry to global buffers

### `DrawQueue`

| Field | Type | Description |
|-------|------|-------------|
| `queue` | `Rc<Vec<DrawObject>>` | Ordered draw list |
| `render_pipelines` | `BTreeMap<String, EffectPipelineData>` | Effect pipeline cache |
| `image_pipeline` | `RenderPipeline` | Default image shader |

### `DrawQueue::new(device, queue, buffers, scene, texture_objects, image_pipeline, post_process, projection_bgl, no_effects) -> Self`

Builds draw objects for all texture objects. The `render_pipelines` map is shared across objects to cache pipeline compilation.

---

## `effect_bindgroup` — EffectBindGroup

### `EffectBindGroup`

| Field | Type | Description |
|-------|------|-------------|
| `pipeline` | `Rc<RenderPipeline>` | Compiled effect shader |
| `bindgroup` | `BindGroup` | Initial bind group (kept for single-pass use) |
| `uniform_buffer` | `Option<Buffer>` | GPU uniform buffer for effect parameters |
| `uniform_layout` | `UniformLayout` | Layout for uniform writes |
| `material_keys` | `BTreeMap<String, String>` | Material key → uniform name mapping |
| `constants` | `BTreeMap<String, Value>` | Material constant overrides |
| `tex_resolutions` | `BTreeMap<String, [f32; 4]>` | Texture resolution values (`g_TextureNResolution`) |

### `EffectBindGroup::new(device, post_process, pipedata, source_view, mask_view, noise_view, pipeline, material_keys, constants, tex_resolutions, mask_tex, noise_tex) -> Option<Self>`

Creates GPU resources for one effect:
1. Creates uniform buffer if the effect has uniforms
2. Builds bind group entries for each sampler (0=source, 1=mask, 2=noise, others=blank)
3. Adds `_wm_sampler` and uniform buffer bindings

### `make_effect_intermediate_bindgroup(device, pipedata, effect_bg, source_view, sampler) -> BindGroup`

Creates a temporary bind group for intermediate passes, replacing the source view with the current ping-pong output.

---

## `ping_pong` — PingPongTextures

### `PingPongTextures::new(device, queue, post_process, width, height) -> Self`

Creates twin `Rgba8UnormSrgb` render targets at `(width, height)`, pre-filled with NDC fullscreen quad geometry.

| Method | Description |
|--------|-------------|
| `make_bindgroup(device, layout, sampler)` | Bind group from `view_a` + sampler |
| `make_bindgroup_for(device, layout, sampler, view)` | Bind group from an arbitrary view + sampler |

---

## `intermediate_pass` — Multi-Effect Rendering

### `render_intermediate_passes(...)`

Called when any draw object has effects. For each object:

1. **Source pass** — render original texture to `view_a` using `image_pipeline`
2. **Effect passes** — for each effect in order, render to alternating targets (`view_a` ↔ `view_b`), using previous pass output as source
3. **Copy-back** — if odd effect count, copy final result back to `view_a`

The projection buffer is temporarily overwritten with identity for NDC-space rendering and restored afterward.

---

## Render Pass Flow

```
write_effect_uniforms()      ← Time, projection, effect params → GPU
         │
    ┌────┴────┐
    │ Effects?│
    ├─ Yes ───┤
    │         ▼
    │  render_intermediate_passes()
    │  Source → view_a
    │  Effect0: view_a → view_b
    │  Effect1: view_b → view_a  (swap each iteration)
    │  Copy-back if odd count
    │         │
    └─ No ────┘
         │
         ▼
    render_final_pass()     ← Single render pass → swapchain → present()
```

## Buffer Limits

| Constant | Value | Description |
|----------|-------|-------------|
| `MAX_TEXTURE` | 512 | Maximum textures per shader stage |
| `MAX_VERTEX` | 2048 | Max vertices in global buffer (`MAX_TEXTURE × 4`) |
| `MAX_INDEX` | 3072 | Max indices in global buffer (`MAX_TEXTURE × 6`) |
