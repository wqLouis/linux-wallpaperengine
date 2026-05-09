# Post-Processor (`src/scene/renderer/post_processor/`)

Shader effect compilation pipeline: parameter layout, GLSL preprocessing, and effect pipeline creation.

---

## `effect_param` — Uniform Buffer Layout

**File:** `effect_param.rs`

Manages GPU uniform buffer layout for effect shaders.

### `UniformLayout`

```rust
pub struct UniformLayout {
    offsets: BTreeMap<String, (u64, u64)>,  // (offset, size)
    total_size: u64,
}
```

#### `UniformLayout::new(decls: &[(String, String)]) -> Self`

Builds a layout from `(name, type)` pairs. Types follow GLSL names: `float`, `vec2`, `vec3`, `vec4`, `mat3`, `mat4`.

Layout algorithm:
- Aligns each field according to its type alignment (vec4/mat4 → 16 bytes, vec2 → 8 bytes, float → 4 bytes)
- Computes field offsets and total buffer size (padded to 16-byte alignment, minimum 16 bytes)

#### `UniformLayout::total_size() -> u64`

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

### `get_or_create_pipeline(...) -> Option<Rc<RenderPipeline>>`

Main entry point for creating effect pipelines. Uses a cache key derived from the effect path + texture presence.

**Cache key computation:**
```
effect_path + "|M1" (if mask texture) + "|T1" (if noise texture)
```

**Pipeline creation** (`create_effect_pipeline`):

1. Parse effect JSON → get material path → get shader name
2. Read `.frag` and `.vert` shader sources from `scene.misc`
3. Collect default defines via `pipeline_helpers::collect_default_defines`
4. Apply texture combos (`ENABLEMASK`, `TIMEOFFSET`) via `pipeline_helpers::apply_texture_combos`
5. Preprocess shaders via `shader_preprocessor::preprocess_pair`
6. Create shader modules with GLSL-to-SPIR-V compilation (naga backend)
7. Create bind group layout via `pipeline_helpers::create_effect_bindgroup_layout`
8. Create render pipeline with alpha blending, back-face culling, `Rgba8UnormSrgb` format
9. Build `UniformLayout` from the shader's uniform declarations

### `load_mask_texture(device, queue, scene, path) -> Option<(Texture, TextureView)>`

Loads a mask/noise texture from `scene.textures` (`.tex` files), uploads as `Rgba8UnormSrgb`, `R8Unorm`, or `Rg8Unorm` based on file extension.

**Path resolution:** Tries multiple candidates:
- `"{path}.tex"`
- `"materials/{path}.tex"`

---

## `pipeline_helpers` — Pipeline Utilities

**File:** `pipeline_helpers.rs`

### `collect_default_defines(vert_source, frag_source) -> BTreeMap<String, String>`

Scans both shader sources for `// [COMBO] {"combo":"NAME","default":N}` JSON annotations and extracts default values.

### `apply_texture_combos(defines, pass_textures)`

Adds combo defines based on additional textures:
- `textures[1]` (mask) present → `ENABLEMASK=1`
- `textures[2]` (noise) present → `TIMEOFFSET=1`

### `create_effect_bindgroup_layout(device, layout) -> BindGroupLayout`

Creates a bind group layout from `EffectLayout`:
- One `Texture2D` binding per sampler (binding = index × 2)
- One `Sampler` at `WM_SAMPLER_BINDING` (binding = 1)
- One `Uniform` buffer binding if uniforms present (binding = sampler_count × 2 + 2)

---

## Shader Preprocessing Pipeline

**Files:** `shader_preprocessor.rs`, `shader_header.rs`, `transform/mod.rs`, `transform/layout.rs`, `transform/replace.rs`

### Public API (`shader_preprocessor.rs`)

#### `preprocess_pair(vert: &str, frag: &str) -> (String, String, EffectLayout)`

Preprocesses both vertex and fragment shaders for Vulkan/SPIR-V compatibility. Returns transformed sources and the combined shader interface layout.

**Features:**
- Hoists conditional varyings (inside `#if` blocks) to be always available as vertex outputs
- Handles `texSample2D` → `texture(sampler2D(...))` transformation

#### `preprocess(source: &str, stage: ShaderStage) -> String`

Preprocesses a single shader source.

---

## `transform/layout.rs` — EffectLayout

Describes a compiled shader's GPU interface:

```rust
pub struct EffectLayout {
    pub sampler_names: Vec<String>,                    // e.g. ["g_Texture0", "g_Texture1"]
    pub sampler_bindings: Vec<u32>,                   // Binding points (0, 2, 4, ...)
    pub uniform_decls: Vec<(String, String)>,          // (name, type) pairs
    pub uniform_material_keys: BTreeMap<String, String>, // material key → uniform name
    pub uniform_binding: u32,                          // Offset after samplers
    pub varying_locations: BTreeMap<String, u32>,     // Varying → location mapping
    pub varying_types: BTreeMap<String, String>,       // e.g. "v_TexCoord" → "vec4"
    pub vertex_varyings: Vec<String>,                  // Varyings in vertex shader source
    pub attribute_locations: BTreeMap<String, u32>,    // Attribute → location mapping
}
```

#### `EffectLayout::sampler_count() -> usize`

---

## `transform/mod.rs` — GLSL → Vulkan Transform

### `preprocess_with_layout(source, stage, layout) -> String`

The main transformation pass:

1. **Adds `#version 450`**
2. **Emits declarations** — samplers, uniform buffer, fragment output
3. **Inlines headers** — `#include` files expanded (stripping duplicate `#define` macros)
4. **Removes original declarations** — uniforms, samplers, attributes, varyings
5. **Transforms statements**:
   - `attribute name` → `layout(location=N) in type name`
   - `varying name` → `layout(location=N) out/in type name` (vertex: out, fragment: in)
6. **Replaces GLSL builtins** (see below)

### `preprocess_with_layout_tracked(...) -> (String, Vec<String>)`

Returns both transformed source and list of unconditionally-emitted varyings. Used for conditional varying hoisting.

---

## `transform/replace.rs` — GLSL Builtin Replacements

| GLSL/HLSL | Vulkan GLSL |
|-----------|-------------|
| `texSample2D(tex, uv)` | `texture(sampler2D(tex, _wm_sampler), uv)` |
| `texSample2DLod(tex, uv, lod)` | `textureLod(sampler2D(tex, _wm_sampler), uv, lod)` |
| `gl_FragColor` | `_fragColor` |
| `mul(a, b)` | `b * a` |
| `saturate(x)` | `clamp(x, 0.0, 1.0)` |
| `frac(x)` | `fract(x)` (avoids naming collision with `frac` variable) |
| `ddx(x)` / `ddy(x)` | `dFdx(x)` / `dFdy(x)` |
| `atan2(y, x)` | `atan(y, x)` |
| `CAST2(x)` | `vec2(x)` |
| `CAST3(x)` | `vec3(x)` |
| `CAST4(x)` | `vec4(x)` |
| `CAST3X3(x)` | `mat3(x)` |
| `sample` (identifier) | `sampleColor` |
| `packed` (identifier) | `packedValue` |

**Implicit truncation fix** (`fix_implicit_truncation`): When a `vec4` varying is assigned from a `vec2` varying, adds explicit swizzle (`.xy`).

---

## `shader_header.rs` — Built-in Headers

#### `get_headers() -> BTreeMap<&'static str, &'static str>`

Returns a map of header file names to their contents:

| Header | Description |
|--------|-------------|
| `common.h` | Utility functions: `hsv2rgb`, `rgb2hsv`, `rotateVec2`, `greyscale`, math constants |
| `common_perspective.h` | `squareToQuad` — maps unit square to arbitrary quadrilateral |
| `common_blending.h` | Blending utilities |
| `common_composite.h` | Compositing utilities |
| `common_blur.h` | Blur functions |
| `common_fragment.h` | Fragment utilities |
| `common_vertex.h` | Vertex utilities |

#### Constants

```rust
pub const WM_SAMPLER_BINDING: u32 = 1;
```

---

## `shader_compiler.rs` — ShaderEffect Parser (Alternative)

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

Parses shader source to extract variables and combo annotations.

### `load(device, shader, stage, defines) -> ShaderModule`

Free function: preprocesses and compiles a GLSL shader to a wgpu `ShaderModule`.

---

## `renderer.rs` — PostProcess WIP

**File:** `renderer.rs`

Contains a stub `PostProcess::process()` method — work in progress. The actual post-processing pipeline is handled by `intermediate_pass.rs`.