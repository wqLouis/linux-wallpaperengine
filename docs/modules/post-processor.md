# Post-Processor (`src/scene/renderer/post_processor/`)

Shader effect compilation pipeline: parameter layout, GLSL preprocessing, effect pipeline creation, and caching.

---

## `mod.rs` — Module Structure

```rust
pub mod effect_param;
pub mod pipeline_handler;
pub mod pipeline_helpers;
pub mod shader_header;
pub mod transform;
```

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

**Layout algorithm:**
- Aligns each field according to its type alignment (vec4/mat4 → 16 bytes, vec3 → 16 bytes, vec2 → 8 bytes, float → 4 bytes)
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
| `write_mat4(buf, name, &[[f32;4];4])` | `mat4` | 64 (column-major) |

#### `UniformLayout::populate_effect_params(buf, constants, material_keys, time, projection, sys)`

Fills a uniform buffer with all system + material values:

| Written Uniform | Source |
|----------------|--------|
| `g_Time` | `time` parameter (elapsed seconds) |
| `g_ModelViewProjectionMatrix` | `projection` matrix |
| `g_Screen` | `sys.screen_resolution` → `[w, h, aspect]` |
| `g_EffectTextureProjectionMatrix` | `projection` |
| `g_EffectTextureProjectionMatrixInverse` | Identity matrix |
| `g_ParallaxPosition` | `sys.cursor_position` (normalized `[0,1]`, top-left origin) |
| `g_TextureNResolution` | From `sys.tex_resolutions` |
| Material constants | From `constants` (resolved via `material_keys` key → uniform name mapping) |

**Property binding resolution:** Values with `{"script": ..., "value": <inner>}` wrappers are automatically unwrapped to `<inner>` before writing.

### `SystemUniforms`

```rust
pub struct SystemUniforms {
    pub screen_resolution: [u32; 2],
    pub tex_resolutions: BTreeMap<String, [f32; 4]>,
    /// Normalized cursor position in [0, 1] range, (0,0) = top-left (UV space)
    pub cursor_position: [f32; 2],
}
```

Passed to `populate_effect_params` to provide resolution and cursor data.

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

Main entry point for creating effect pipelines. Uses a cache key derived from the effect path + texture presence + combo values.

**Cache key computation:**
```
effect_path + "|M1" (if mask texture present) + "|T1" (if noise texture present) + "|COMBO_NAME=value" (for each combo)
```

**Pipeline creation** (`create_effect_pipeline`):

1. Parse effect JSON → get `passes[0].material` path → get material JSON → get `passes[0].shader` name
2. Read `.frag` and `.vert` shader sources from `scene.misc`
3. Collect combo defines (priority: shader defaults < material.json combos < scene pass combos):
   - `collect_default_defines()` — scans both shader sources for `// [COMBO] {"combo":"NAME","default":N}` annotations
   - Merges material.json `passes[0].combos`
   - Applies scene pass combos (highest priority)
4. Apply automatic texture combos via `apply_texture_combos()` — sets `MASK=1` if textures[1] present, `TIMEOFFSET=1` if textures[2] present
5. Load shader headers via `shader_header::get_headers()`
6. Preprocess shaders via `transform::preprocess_pair(vert, frag, headers, defines)`
7. Create shader modules with GLSL-to-SPIR-V compilation (naga backend with `ShaderSource::Glsl`)
8. Create bind group layout via `create_effect_bindgroup_layout()`
9. Create render pipeline with alpha blending (`SrcAlpha / OneMinusSrcAlpha`), back-face culling, `Rgba8UnormSrgb` format
10. Build `UniformLayout` from the shader's uniform declarations

### `load_mask_texture(device, queue, scene, path) -> Option<(Texture, TextureView)>`

Loads a mask or noise texture from `scene.textures`, uploaded with the correct GPU format:

| Extension | GPU Format |
|-----------|-----------|
| `.r8` | `R8Unorm` |
| `.rg88` | `Rg8Unorm` |
| other | `Rgba8Unorm` |

**Path resolution:** Prepends `materials/` and appends `.tex`:
```
"materials/{path}.tex"
```

---

## `pipeline_helpers` — Pipeline Utilities

**File:** `pipeline_helpers.rs`

### `collect_default_defines(vert_source, frag_source) -> BTreeMap<String, String>`

Scans both shader sources for `// [COMBO] {"combo":"NAME","default":N}` JSON annotations and extracts default values.

### `apply_texture_combos(defines, pass_textures)`

Adds combo defines based on additional textures:
- `textures[1]` (mask) present → `MASK=1` (not `ENABLEMASK` — the old name)
- `textures[2]` (noise) present → `TIMEOFFSET=1`

### `create_effect_bindgroup_layout(device, layout) -> BindGroupLayout`

Creates a bind group layout from `EffectLayout`:
- One `Texture2D<f32>` binding per sampler at `binding = index × 2` (`VertexFragment`)
- One `Sampler` at `WM_SAMPLER_BINDING` (binding = 1, `Fragment`)
- One `Uniform` buffer binding if uniforms present, at `binding = sampler_count × 2 + 2` (`VertexFragment`)

---

## Shader Preprocessing Pipeline

**Files:** `transform/mod.rs`, `transform/layout.rs`, `transform/replace.rs`

### Public API (`transform/mod.rs`)

#### `preprocess_pair(vert: &str, frag: &str, headers, defines) -> (String, String, EffectLayout)`

Preprocesses both vertex and fragment shaders for Vulkan/SPIR-V compatibility. Returns transformed sources and the combined shader interface layout.

**Features:**
- Collects layout from both shaders (including transitively `#include`-d headers)
- Evaluates `#if`/`#ifdef`/`#ifndef`/`#elif`/`#else`/`#endif` using provided defines (supports `defined()`, `==`, `!=`, `!`, `||`, `&&`)
- Strips or inlines header content based on active branches
- Hoists conditional varyings: fragment varyings that only appear inside `#if` blocks in the vertex shader are hoisted to unconditional vertex outputs (wgpu requires all fragment inputs to have corresponding vertex outputs). Synthesizes declarations and zero-initializations in `main()` as needed.
- Performs GLSL builtin replacements (see below)
- Strips `uniform`/`sampler2D`/`attribute`/`varying` declarations, replacing them with proper `layout(binding=...)` / `layout(location=...)` declarations

#### `preprocess_with_layout(source, stage, layout, headers, defines) -> String`

Preprocesses a single shader source with a given pre-computed layout.

#### `preprocess_with_layout_tracked(...) -> (String, Vec<String>)`

Returns both the transformed source and the list of varyings that were unconditionally emitted. Used for conditional varying hoisting.

---

## `transform/layout.rs` — EffectLayout

Describes a compiled shader's GPU interface:

```rust
pub struct EffectLayout {
    pub sampler_names: Vec<String>,                    // e.g. ["g_Texture0", "g_Texture1"]
    pub sampler_bindings: Vec<u32>,                   // Binding points (0, 2, 4, ...)
    pub uniform_decls: Vec<(String, String)>,          // (name, type) pairs
    pub uniform_material_keys: BTreeMap<String, String>, // material key → uniform name
    pub uniform_binding: u32,                          // Offset after samplers (= sampler_count * 2 + 2)
    pub varying_locations: BTreeMap<String, u32>,     // Varying → location mapping
    pub varying_types: BTreeMap<String, String>,       // e.g. "v_TexCoord" → "vec4"
    pub vertex_varyings: Vec<String>,                  // Varyings found in vertex shader source
    pub attribute_locations: BTreeMap<String, u32>,    // Attribute → location mapping
}
```

#### `EffectLayout::sampler_count() -> usize`

Returns the number of sampler declarations.

#### `collect_layout(source1, source2, headers) -> EffectLayout`

Introsects both vertex (source1) and fragment (source2) shaders, collecting:
- `sampler2D` declarations → `sampler_names`
- `uniform type name` declarations → `uniform_decls`
- `varying type name` → `varying_locations`, `varying_types`
- `attribute type name` → `attribute_locations`
- `// {"material":"key"}` annotations → `uniform_material_keys`
- Recursively processes `#include`-d headers
- Sorts and deduplicates collected items

---

## `transform/replace.rs` — GLSL Builtin Replacements

| GLSL/HLSL | Vulkan GLSL |
|-----------|-------------|
| `texSample2D(tex, uv)` | `texture(sampler2D(tex, _wm_sampler), uv)` |
| `texSample2DLod(tex, uv, lod)` | `textureLod(sampler2D(tex, _wm_sampler), uv, lod)` |
| `gl_FragColor` | `_fragColor` |
| `mul(a, b)` | `b * a` (with parentheses if followed by `.`) |
| `saturate(x)` | `clamp(x, 0.0, 1.0)` (only standalone `saturate`, not `Desaturate`) |
| `frac(x)` | `fract(x)` (only standalone `frac`, not word-internal) |
| `ddx(x)` / `ddy(x)` | `dFdx(x)` / `dFdy(x)` |
| `atan2(y, x)` | `atan(y, x)` |
| `CAST2(x)` | `vec2(x)` |
| `CAST3(x)` | `vec3(x)` |
| `CAST4(x)` | `vec4(x)` |
| `CAST3X3(x)` | `mat3(x)` |
| `sample` (reserved GLSL identifier) | `sampleColor` |
| `packed` (reserved GLSL identifier) | `packedValue` |

**Implicit truncation fix** (`fix_implicit_truncation`): When a `vec4` varying is assigned from a `vec2` varying, adds explicit swizzle (`.xy`). Detected by comparing LHS and RHS types from `varying_types`.

**Texture call rewrites:** `texture()` and `textureLod()` calls with a first argument matching a sampler name are rewritten to include `sampler2D(name, _wm_sampler)`.

---

## `shader_header.rs` — Built-in Headers Loading

**File:** `shader_header.rs`

#### `get_headers(misc: &MiscBucket) -> BTreeMap<String, String>`

Loads shader header files from the Wallpaper Engine assets bucket. Headers are stored under `shaders/common*.h` paths.

Returns a map of bare filename → header content for these headers:

| Header | Description |
|--------|-------------|
| `common.h` | Utility functions: `hsv2rgb`, `rgb2hsv`, `rotateVec2`, `greyscale`, math constants |
| `common_perspective.h` | `squareToQuad` — maps unit square to arbitrary quadrilateral |
| `common_blending.h` | Blending utilities |
| `common_composite.h` | Compositing utilities |
| `common_blur.h` | Blur functions |
| `common_fragment.h` | Fragment utilities |
| `common_vertex.h` | Vertex utilities |
| `common_fog.h` | Fog effects |
| `common_foliage.h` | Foliage/vegetation effects |
| `common_particles.h` | Particle system utilities |
| `common_pbr.h` | PBR shading |
| `common_pbr_2.h` | PBR shading v2 |

#### Constants

```rust
pub const WM_SAMPLER_BINDING: u32 = 1;
```

---

## Shader Preprocessor Condition Evaluation

The preprocessor handles these condition forms in `#if`/`#elif` directives:

| Form | Example |
|------|---------|
| `defined(NAME)` | `#if defined(VERTICAL)` |
| `!defined(NAME)` | `#if !defined(MASK)` |
| `NAME == VALUE` | `#if BLENDMODE == 26` |
| `NAME != VALUE` | `#if MODE != 3` |
| `!NAME` | `#if !MASK` |
| `NAME` (bare) | `#if VERTICAL` — truthy if defined and not `"0"` |
| `\|\|` | `#if A \|\| B` (left-to-right, no precedence) |
| `&&` | `#if A && B` (left-to-right, no precedence) |

## Varying Hoisting

When fragment shader varyings are referenced unconditionally but the vertex shader only declares them inside `#if` blocks, the preprocessor:

1. **Tracks** which varyings were emitted in the vertex output during preprocessing (`preprocess_with_layout_tracked`)
2. **Computes missing** varyings from the vertex shader's full varying list (`layout.vertex_varyings`)
3. **Hoists** declarations: moves `layout(location=N) out TYPE NAME;` to the top of the vertex shader (outside all `#if` blocks)
4. **Synthesizes** declarations for varyings that were declared only in headers that were entirely excluded by `#if` conditions
5. **Adds zero-initialization** in `main()` for all hoisted varyings

This is required because wgpu/naga validates that all fragment inputs have corresponding vertex outputs at the SPIR-V level, regardless of preprocessor conditions.
