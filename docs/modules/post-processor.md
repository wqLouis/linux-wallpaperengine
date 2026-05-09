# Post-Processor (`src/scene/renderer/post_processor/`)

Shader effect compilation pipeline: parameter layout, GLSL preprocessing, and effect pipeline creation.

---

## `effect_param` — Uniform Buffer Layout

### `UniformLayout`

Manages byte-level layout of a GPU uniform buffer for effect shaders.

### `UniformLayout::new(decls: &[(String, String)]) -> Self`

| Parameter | Type | Description |
|-----------|------|-------------|
| `decls` | `&[(String, String)]` | `(name, type)` pairs from shader introspection |

Computes offsets and total size using GLSL alignment rules (vec4 → 16-byte, vec2 → 8-byte, float → 4-byte).

### `UniformLayout` methods

| Method | Return | Description |
|--------|--------|-------------|
| `total_size()` | `u64` | Total buffer size (padded to 16, min 16) |
| `write_f32(buf, name, value)` | `bool` | Write `f32`, returns false if name not found |
| `write_vec2(buf, name, value)` | `bool` | Write `[f32; 2]` |
| `write_vec4(buf, name, value)` | `bool` | Write `[f32; 4]` |
| `write_mat4(buf, name, value)` | `bool` | Write `&[[f32; 4]; 4]` (column-major) |

### `UniformLayout::populate_effect_params(buf, constants, material_keys, time, projection, sys)`

Fills the uniform buffer with all system and material values:

| Written Uniform | Source |
|----------------|--------|
| `g_Time` | `time` parameter |
| `g_ModelViewProjectionMatrix` | `projection` matrix |
| `g_Screen` | `[width, height, aspect]` from `sys.screen_resolution` |
| `g_EffectTextureProjectionMatrix` | `projection` |
| `g_EffectTextureProjectionMatrixInverse` | Identity matrix |
| `g_ParallaxPosition` | `[0.0, 0.0]` |
| `g_TextureNResolution` | From `sys.tex_resolutions` |
| Material constants | From `constants` resolved via `material_keys` |

### `SystemUniforms`

| Field | Type | Description |
|-------|------|-------------|
| `screen_resolution` | `[u32; 2]` | Current output dimensions |
| `tex_resolutions` | `BTreeMap<String, [f32; 4]>` | Per-texture resolution (`g_TextureNResolution`) |
| `cursor_position` | `[f32; 2]` | Normalized cursor in `[0, 1]` |

---

## `pipeline_handler` — Effect Pipeline Creation

### `EffectPipelineData`

| Field | Type | Description |
|-------|------|-------------|
| `pipeline` | `Rc<RenderPipeline>` | Compiled render pipeline |
| `layout` | `EffectLayout` | Shader interface introspection |
| `bindgroup_layout` | `BindGroupLayout` | Bind group layout for effect resources |
| `uniform_layout` | `UniformLayout` | Uniform buffer layout |

### `get_or_create_pipeline(device, effect_path, pass_textures, pipelines, scene, projection_bgl) -> Option<Rc<RenderPipeline>>`

Main entry point. Caches pipelines by effect path + texture combo (`path + "|M1"` for mask, `+ "|T1"` for noise).

Pipeline creation steps:
1. Parse effect JSON → material JSON → shader name
2. Read `.frag` and `.vert` shader sources from `scene.misc`
3. Collect combo defines, apply texture combos
4. Preprocess via `shader_preprocessor::preprocess_pair`
5. Create shader modules (GLSL → Vulkan via naga)
6. Create bind group layout and render pipeline
7. Build `UniformLayout` from shader uniform declarations

### `load_mask_texture(device, queue, scene, path) -> Option<(Texture, TextureView)>`

Loads a mask/noise texture. Tries path candidates `{path}.tex` and `materials/{path}.tex`. GPU format depends on file extension: `R8Unorm` for `.r8`, `Rg8Unorm` for `.rg88`, `Rgba8Unorm` otherwise.

---

## `pipeline_helpers` — Pipeline Utilities

| Function | Description |
|----------|-------------|
| `collect_default_defines(vert_source, frag_source)` | Reads `[COMBO]` JSON annotations for default define values |
| `apply_texture_combos(defines, pass_textures)` | Adds `ENABLEMASK=1` (mask present) or `TIMEOFFSET=1` (noise present) |
| `create_effect_bindgroup_layout(device, layout)` | Creates bind group layout from `EffectLayout`: textures at even bindings, sampler at binding 1, uniform after samplers |

---

## Shader Preprocessing Pipeline

**Files:** `shader_preprocessor.rs`, `shader_header.rs`, `transform/`

### `preprocess_pair(vert, frag) -> (String, String, EffectLayout)`

Preprocesses vertex and fragment shaders for Vulkan/SPIR-V compatibility. Returns transformed sources and combined shader interface layout.

Features:
- Collects samplers, uniforms, varyings, and attributes
- Hoists conditional varyings (inside `#if` blocks) to always be available
- Transforms `texSample2D` → `texture(sampler2D(...))` and other GLSL builtins

### `EffectLayout`

Describes a shader's GPU interface:

| Field | Type | Description |
|-------|------|-------------|
| `sampler_names` | `Vec<String>` | Sampler variable names |
| `sampler_bindings` | `Vec<u32>` | Binding points (0, 2, 4, …) |
| `uniform_decls` | `Vec<(String, String)>` | `(name, type)` pairs |
| `uniform_material_keys` | `BTreeMap<String, String>` | Material key → uniform name |
| `uniform_binding` | `u32` | Uniform binding point |
| `varying_locations` / `varying_types` | | Varying → location/type mapping |

---

## GLSL Builtin Replacements

| GLSL/HLSL → Vulkan GLSL | Description |
|-------------------------|-------------|
| `texSample2D(tex, uv)` → `texture(sampler2D(tex, _wm_sampler), uv)` | Texture sampling |
| `texSample2DLod(tex, uv, lod)` → `textureLod(sampler2D(tex, _wm_sampler), uv, lod)` | Mipmapped sampling |
| `gl_FragColor` → `_fragColor` | Fragment output |
| `mul(a, b)` → `b * a` | Matrix multiply |
| `saturate(x)` → `clamp(x, 0, 1)` | Clamp |
| `frac(x)` → `fract(x)` | Fractional part |
| `ddx` / `ddy` → `dFdx` / `dFdy` | Derivatives |

---

## Built-in Shader Headers

Headers from `shader_header.rs` that are available for inclusion in effect shaders:

| Header | Contents |
|--------|----------|
| `common.h` | `hsv2rgb`, `rgb2hsv`, `rotateVec2`, `greyscale`, math constants |
| `common_perspective.h` | `squareToQuad` |
| `common_blending.h` | Blending utilities |
| `common_composite.h` | Compositing utilities |
| `common_blur.h` | Blur functions |
| `common_fragment.h` | Fragment utilities |
| `common_vertex.h` | Vertex utilities |

### Constants

```rust
pub const WM_SAMPLER_BINDING: u32 = 1;
```
