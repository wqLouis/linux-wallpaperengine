# Post-Processor (`src/scene/renderer/post_processor/`)

Shader effect compilation pipeline: parameter layout, GLSL preprocessing, and effect pipeline creation.

---

## `effect_param` — Uniform Buffer Layout

**File:** `effect_param.rs`

Manages GPU uniform buffer layout for effect shaders.

### `UniformLayout`

```rust
pub struct UniformLayout {
    // internal: offsets: BTreeMap<String, (offset, size)>
    // internal: total_size: u64
}
```

#### `UniformLayout::new(decls: &[(String, String)]) -> Self`

Builds a layout from `(name, type)` pairs. Types follow GLSL names: `float`, `vec2`, `vec3`, `vec4`, `mat3`, `mat4`.

Layout algorithm:
- Aligns each field according to its type alignment (vec4/mat4 → 16 bytes, vec2 → 8 bytes, float → 4 bytes)
- Computes field offsets and total buffer size (padded to 16-byte alignment, minimum 16)

#### `UniformLayout::total_size(&self) -> u64`

Returns the total buffer size in bytes.

#### Write Methods

All return `bool` (true if the named field exists and fits in the buffer).

| Method | Type | Bytes |
|--------|------|-------|
| `write_f32(buf, name, value)` | `f32` | 4 |
| `write_vec2(buf, name, [f32;2])` | `vec2` | 8 |
| `write_vec3(buf, name, [f32;3])` | `vec3` | 12 |
| `write_vec4(buf, name, [f32;4])` | `vec4` | 16 |
| `write_mat4(buf, name, &[[f32;4];4])` | `mat4` | 64 |

#### `UniformLayout::populate_effect_params(buf, constants, material_keys, time, projection, sys)`

Fills a uniform buffer with all system + material values:

| Written Uniform | Source |
|----------------|--------|
| `g_Time` | `time` parameter (elapsed seconds) |
| `g_ModelViewProjectionMatrix` | `projection` matrix |
| `g_Screen` | `sys.screen_resolution` → `[w, h, aspect]` |
| `g_EffectTextureProjectionMatrix` | `projection` |
| `g_EffectTextureProjectionMatrixInverse` | Identity matrix |
| `g_ParallaxPosition` | `[0.0, 0.0]` |
| `g_TextureNResolution` | From `sys.tex_resolutions` |
| Material constants | From `constants` (resolved via `material_keys`) |

### `SystemUniforms`

```rust
pub struct SystemUniforms {
    pub screen_resolution: [u32; 2],
    pub tex_resolutions: BTreeMap<String, [f32; 4]>,
}
```

Passed to `populate_effect_params` to provide resolution data.

---

## `pipeline_handler` — Effect Pipeline Creation

**File:** `pipeline_handler.rs`

### `EffectPipelineData`

```rust
pub struct EffectPipelineData {
    pub pipeline: Rc<RenderPipeline>,
    pub layout: EffectLayout,
    pub bindgroup_layout: BindGroupLayout,
    pub uniform_layout: UniformLayout,
}
```

### `get_or_create_pipeline(device, effect_path, pass_textures, pipelines, scene, projection_bgl) -> Option<Rc<RenderPipeline>>`

Main entry point for creating effect pipelines. Uses a cache key derived from the effect path + texture presence.

**Pipeline creation** (`create_effect_pipeline`):

1. Parse effect JSON → get material path → get shader name
2. Read `.frag` and `.vert` shader sources from `scene.misc`
3. Collect default defines via `pipeline_helpers::collect_default_defines`
4. Apply texture combos (`MASK`, `TIMEOFFSET`) via `pipeline_helpers::apply_texture_combos`
5. Preprocess shaders via `shader_preprocessor::preprocess_pair`
6. Create shader modules with GLSL-to-SPIRV compilation (naga backend)
7. Create bind group layout via `pipeline_helpers::create_effect_bindgroup_layout`
8. Create render pipeline with alpha blending, back-face culling, `Rgba8UnormSrgb` format
9. Build `UniformLayout` from the shader's uniform declarations

### `load_mask_texture(device, queue, scene, path) -> Option<(Texture, TextureView)>`

Loads a mask/noise texture from `scene.textures` (`.tex` files), uploads as `Rgba8UnormSrgb`.

---

## `pipeline_helpers` — Pipeline Utilities

**File:** `pipeline_helpers.rs`

### `collect_default_defines(vert_source, frag_source) -> BTreeMap<String, String>`

Scans both shader sources for `[COMBO]` JSON annotations and extracts default values.

Example shader annotation:
```glsl
// [COMBO] {"combo":"MODE","default":1}
```

Returns `{"MODE": "1", ...}`.

### `apply_texture_combos(defines, pass_textures)`

Adds combo defines based on additional textures:
- Mask texture present → `MASK=1`
- Noise texture present → `TIMEOFFSET=1`

### `create_effect_bindgroup_layout(device, layout) -> BindGroupLayout`

Creates a bind group layout from `EffectLayout`:
- One `Texture2D` binding per sampler (binding = index × 2)
- One `Sampler` at `WM_SAMPLER_BINDING` (binding = 1)
- One `Uniform` buffer binding if uniforms present (binding = sampler_count × 2 + 2)

---

## Shader Preprocessing Pipeline

**Files:** `shader_preprocessor.rs`, `shader_header.rs`, `shader_layout.rs`, `shader_transform.rs`, `shader_replace.rs`

### Public API (`shader_preprocessor.rs`)

#### `preprocess_pair(vert: &str, frag: &str) -> (String, String, EffectLayout)`

Preprocesses both vertex and fragment shaders for Vulkan/SPIR-V compatibility. Returns transformed sources and the combined shader interface layout.

#### `preprocess(source: &str, stage: ShaderStage) -> String`

Preprocesses a single shader source.

### `EffectLayout` (`shader_layout.rs`)

Describes a compiled shader's GPU interface:

```rust
pub struct EffectLayout {
    pub sampler_names: Vec<String>,                    // e.g. ["g_Texture0", "g_Texture1"]
    pub sampler_bindings: Vec<u32>,                    // Binding points (0, 2, 4, ...)
    pub uniform_decls: Vec<(String, String)>,          // (name, type) pairs
    pub uniform_material_keys: BTreeMap<String, String>, // material key → uniform name
    pub uniform_binding: u32,                          // Offset after samplers
    pub varying_names: Vec<String>,
    pub varying_locations: BTreeMap<String, u32>,
    pub varying_types: BTreeMap<String, String>,       // e.g. "v_TexCoord" → "vec4"
    pub attribute_names: Vec<String>,
    pub attribute_locations: BTreeMap<String, u32>,
}
```

#### `EffectLayout::sampler_count() -> usize`

#### `EffectLayout::uniform_count() -> usize`

### Layout Collection (`shader_layout.rs`)

#### `collect_layout(source1, source2) -> EffectLayout`

Parses shader sources to determine:
- **Samplers** — `uniform sampler2D name;`
- **Uniforms** — `uniform float name;` (with optional `// {"material":"key"}` JSON annotations)
- **Varyings** — `varying vec4 name;` (with type tracking for truncation analysis)
- **Attributes** — `attribute vec3 name;`

Resolves `#include "common.h"` and `#include "common_perspective.h"` inline.

### GLSL → Vulkan Transform (`shader_transform.rs`)

#### `preprocess_with_layout(source, stage, layout) -> String`

The main transformation pass:

1. **Adds `#version 450`**
2. **Emits declarations** — samplers, uniform buffer, fragment output
3. **Inlines headers** — `#include` files expanded (stripping duplicate `#define` macros)
4. **Removes original declarations** — uniforms, samplers, attributes, varyings
5. **Transforms statements**:
   - `attribute name` → `layout(location=N) in type name`
   - `varying name` → `layout(location=N) out/in type name` (vertex: out, fragment: in)
6. **Replaces GLSL builtins** (see below)

### GLSL Builtin Replacements (`shader_replace.rs`)

| GLSL/HLSL | Vulkan GLSL |
|-----------|-------------|
| `texSample2D(tex, uv)` | `texture(sampler2D(tex, _wm_sampler), uv)` |
| `texSample2DLod(tex, uv, lod)` | `textureLod(sampler2D(tex, _wm_sampler), uv, lod)` |
| `gl_FragColor` | `_fragColor` |
| `mul(a, b)` | `b * a` |
| `saturate(x)` | `clamp(x, 0.0, 1.0)` |
| `frac(x)` | `fract(x)` |
| `ddx(x)` / `ddy(x)` | `dFdx(x)` / `dFdy(x)` |
| `atan2(y, x)` | `atan(y, x)` |
| `CAST2(x)` | `vec2(x)` |
| `CAST3(x)` | `vec3(x)` |
| `CAST4(x)` | `vec4(x)` |
| `CAST3X3(x)` | `mat3(x)` |

**Implicit truncation fix** (`fix_implicit_truncation`): When a `vec4` varying is assigned from a `vec2` varying, adds explicit swizzle (`.xy`).

### Built-in Headers (`shader_header.rs`)

#### `COMMON_H`
Standard utility functions: `hsv2rgb`, `rgb2hsv`, `rotateVec2`, `greyscale`, and math constants (`M_PI`, `M_PI_2`, `SQRT_2`, `SQRT_3`).

#### `COMMON_PERSPECTIVE_H`
Perspective transformation: `squareToQuad` — maps unit square to arbitrary quadrilateral.

#### Constants
```rust
pub const WM_SAMPLER_BINDING: u32 = 1;
```

---

## `shader_compiler` — ShaderEffect Parser

**File:** `shader_compiler.rs`

Alternative parsing approach (partially implemented, `#[allow(dead_code)]`).

### `ShaderEffect`

```rust
pub struct ShaderEffect {
    pub vars: Vec<ShaderVariable>,
    pub combos: Option<Vec<BTreeMap<String, Value>>>,
    pub source: String,
}
```

#### `ShaderEffect::new(shader: String) -> Self`

Parses a shader source to extract:
- **Variables** — `uniform type name;` declarations
- **Combos** — `// [COMBO] {...}` JSON annotations

#### `ShaderEffect::compile(&self, device, stage) -> ShaderModule`

Compiles the shader with combo defines as preprocessor macros.

### `load(device, shader, stage, defines) -> ShaderModule`

Free function: preprocesses and compiles a GLSL shader to a wgpu `ShaderModule`.

---

## `renderer` — PostProcess WIP

**File:** `renderer.rs`

Contains a stub `PostProcess::process()` method — work in progress. The actual post-processing pipeline is handled by `intermediate_pass.rs`.
