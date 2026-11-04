# Post-Processor (`src/scene/renderer/post_processor/`)

Shader effect compilation pipeline: parameter layout, GLSL preprocessing, effect pipeline creation, and caching.

---

## `mod.rs` — Module Structure

```rust
pub mod context;
pub mod effect_param;
pub mod effect_step;
pub mod pipeline_handler;
pub mod pipeline_helpers;
```

The GLSL preprocessing pipeline itself now lives under
[`src/scene/loader/shader/`](loader.md) so shaders are parsed when the scene is
loaded.

---

## `effect_param` — Uniform Buffer Layout

**File:** `effect_param.rs`

Manages GPU uniform buffer layout for effect shaders.

### `UniformLayout`

```rust
pub struct UniformLayout {
    offsets: Map<String, (u64, u64)>,  // (offset, size)
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
    pub tex_resolutions: Map<String, [f32; 4]>,
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
    pub compute_pipeline: Option<Rc<ComputePipeline>>,
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

**Pipeline creation** (`compile_pipeline`):

1. Parse effect JSON → get `passes[0].material` path → get material JSON → get `passes[0].shader` name
2. Fetch the pre-parsed `ShaderProgram` from `scene.assets.shader(frag, vert)` (sources, headers, layout and defaults were resolved by `Scene::prepare()`)
3. Collect combo defines (priority: shader defaults < material.json combos < scene pass combos):
   - `ShaderProgram::default_defines` — from `// [COMBO] {"combo":"NAME","default":N}` annotations
   - Merges material.json `passes[0].combos`
   - Applies scene pass combos (highest priority)
4. Apply automatic texture combos via `apply_texture_combos()` — sets `MASK=1` if textures[1] present, `TIMEOFFSET=1` if textures[2] present
5. Merge the shader's `source_defines` (combo defines take priority) and preprocess via `shader::preprocess_pair_with_layout(vert, frag, headers, defines, layout, has_subgroup, use_immediates)`
6. Create shader modules with GLSL-to-SPIR-V compilation (naga backend with `ShaderSource::Glsl`)
7. Create bind group layout via `create_effect_bindgroup_layout()`
8. Create render pipeline with additive blending (`One / One`, the destination is cleared to `(0,0,0,0)`), back-face culling, `Rgba8UnormSrgb` format
9. Build `UniformLayout` from the shader's uniform declarations

### `load_mask_texture(device, queue, scene, path) -> Option<(Texture, TextureView)>`

Loads a mask or noise texture from `scene.assets`, uploaded with the correct GPU format:

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

## `effect_step` — The Steps of an Effect

**File:** `effect_step.rs`

The effect of an object is a list of passes: `build_effect_steps` turns them
into `EffectStep`s, one per pass, and allocates the framebuffers they work
on.

- An effect whose `visible` flag is false is not applied at all: it is not
  compiled and it does not cost anything.
- The texture of a pass comes from the material of the pass, the mask and the
  noise come from the textures of the pass.
- The bind group of a step binds the textures of the material to the samplers
  of the shader: the texture a sampler reads is the one its name refers to
  (`g_Texture0`, `g_Texture1`, ...), see
  [The Samplers of a Shader](#the-samplers-of-a-shader).

---

## Shader Preprocessing Pipeline

The preprocessing pipeline lives in `src/scene/loader/shader/` and is driven by
the loader: `Scene::prepare()` parses each shader pair into a `ShaderProgram`,
and the pipeline handler only runs the define-dependent pass.

**Files:** `shader/mod.rs`, `shader/layout.rs`, `shader/varying.rs`,
`shader/asset.rs`, `shader/header.rs`, `shader/preprocessor.rs`,
`shader/c_expr.rs`, `shader/compat.rs`, `shader/dialect.rs`, `shader/glsl/`

### The stages of the pipeline

The pipeline has four stages.  Each of them only fires when it knows what it
does: a rule that cannot be applied is left alone.

1. **The preprocessor** (`shader/preprocessor.rs`, `shader/c_expr.rs`)
   resolves the conditionals of the shader, inlines the headers it includes
   and collects the macros of the shader and of its headers.  The conditions
   are evaluated by `c_expr`, which implements the operators and the
   precedence of C (see [Shader Preprocessor Condition Evaluation](#shader-preprocessor-condition-evaluation)).
   The preprocessor is lenient like the reference engine: a stray `#endif`,
   an unknown directive and a conditional that is never closed are warnings,
   the shader is preprocessed anyway.
2. **The compat stage** (`shader/compat.rs`) rewrites the constructs of the
   Wallpaper Engine dialect that have no GLSL equivalent: `CAST2`, `CAST3`,
   `CAST4`, `saturate`, `frac`, `texSample2D`, `gl_FragColor` and the
   boolean arithmetic.  The `mul` call is rewritten here as well, in the row
   vector form the engine uses: `mul(a, b)` becomes `b * a`.
3. **The type aware stage** (`shader/glsl/`) normalises the types of the
   shader, so that it satisfies the strict typing rules of GLSL.  It is a
   lexer, a type model, a symbol table, a Pratt parser, an inference engine
   and a table of rules.  The rules only fire when the types are known: an
   expression whose type cannot be worked out is left alone.
4. **The dialect stage** (`shader/dialect.rs`) rewrites the rest of the
   dialect of the engine: the `mul` calls (in their row vector form), the
   scalar to vector promotion in the built-in calls, the precision
   qualifiers, the sampler types the engine has and the macros the engine
   provides.

The stages are applied in that order: the type aware stage works on the text
the compat stage produced, and the dialect stage works on the text the type
aware stage produced.

### Public API (`shader/mod.rs`)

#### `preprocess_pair_with_layout(vert: &str, frag: &str, headers, defines, layout, has_subgroup, use_immediates) -> (String, String, EffectLayout)`

Preprocesses both vertex and fragment shaders for Vulkan/SPIR-V compatibility using a layout that was already collected at load time. Returns transformed sources and the combined shader interface layout.

**Features:**
- Uses the precomputed layout from `ShaderProgram` (no re-scan of the sources)
- Evaluates `#if`/`#ifdef`/`#ifndef`/`#elif`/`#else`/`#endif` using provided defines, with the semantics of C (see below)
- Strips or inlines header content based on active branches
- Declares the varyings the vertex shader does not produce: the ones the fragment shader reads are declared and initialised with zero in the vertex shader, so that the program is valid (see [The Varyings of a Shader](#the-varyings-of-a-shader))
- Renames the varyings the fragment shader writes to: Vulkan GLSL does not allow the fragment shader to write to an input
- Performs GLSL builtin replacements (see below)
- Strips `uniform`/`sampler2D`/`attribute`/`varying` declarations, replacing them with proper `layout(binding=...)` / `layout(location=...)` declarations

#### `preprocess_with_layout(source, stage, layout, headers, defines) -> String`

Preprocesses a single shader source with a given pre-computed layout.

#### `preprocess_with_layout_tracked(...) -> (String, Vec<String>)`

Returns both the transformed source and the list of varyings the vertex stage emitted.  The varyings it does not emit are declared by the vertex shader itself.

---

## `shader/layout.rs` — EffectLayout

Describes a compiled shader's GPU interface:

```rust
pub struct EffectLayout {
    pub sampler_names: Vec<String>,                    // e.g. ["g_Texture0", "g_Texture1"]
    pub uniform_decls: Vec<(String, String)>,          // (name, type) pairs
    pub uniform_material_keys: Map<String, String>, // material key → uniform name
    pub uniform_binding: u32,                          // Offset after samplers (= sampler_count * 2 + 2)
    pub varying_locations: Map<String, u32>,     // Varying → location mapping
    pub varying_types: Map<String, String>,       // e.g. "v_TexCoord" → "vec4"
    pub varying_array_sizes: Map<String, u32>,    // The size of the varyings that are arrays
    pub vertex_varyings: Vec<String>,                  // Varyings found in vertex shader source
    pub fragment_varyings: Vec<String>,                // Varyings found in fragment shader source
    pub attribute_locations: Map<String, u32>,    // Attribute → location mapping
}
```

#### `EffectLayout::sampler_count() -> usize`

Returns the number of sampler declarations.

#### `EffectLayout::sampler_texture_slot(index) -> usize`

The texture of the material the sampler at `index` reads.

The samplers of the Wallpaper Engine shaders are named `g_Texture0`,
`g_Texture1`, ...: the number behind the name is the index of the texture of
the material the sampler reads.  The binding of a sampler is its position in
the shader, the texture it reads is the one of its name - the two are not
the same when the shader does not declare its samplers in order.

A sampler whose name does not follow the scheme reads the texture of its
position: that is the behaviour the pipeline had before the names were
understood.

#### `collect_layout(source1, source2, headers) -> EffectLayout`

Introspects both vertex (source1) and fragment (source2) shaders, collecting:
- `sampler2D` declarations → `sampler_names` (in the order of their declaration)
- `uniform type name` declarations → `uniform_decls`
- `varying type name` → `varying_locations`, `varying_types`, `varying_array_sizes`
- `attribute type name` → `attribute_locations`
- `// {"material":"key"}` annotations → `uniform_material_keys`
- Recursively processes `#include`-d headers
- Sorts and deduplicates the collected items; the samplers keep the order of their declaration

---

## `shader/varying.rs` — The Varyings of a Shader

A varying of the fragment shader is an input: Vulkan GLSL does not allow the
fragment shader to write to it.  The shaders of Wallpaper Engine do it
anyway, so the pipeline has to know which varyings the fragment shader
writes to.

#### `written_varyings(source, candidates) -> Vec<String>`

The names of the varyings the source writes to.

The detection uses the parser of the type aware stage (`shader/glsl/`): the
shader is parsed, the statements are walked and the assignments to a varying
are found.  A text search would find the wrong things: the name of a varying
can be the tail of the name of another one (`v_UV` and `my_v_UV`), and a
write can be written without spaces around the `=`.

#### `fix_fragment_writes(source, layout) -> String`

The fragment shader with its writes to the varyings fixed: the declaration
of a varying that is written to is renamed (`v_UV` becomes `_in_v_UV`) and a
local variable of the same type takes its place at the top of `main()`.  The
shader reads the input under the new name and writes to the local variable.

---

## The Varyings of a Shader

The vertex shader has to produce every varying the fragment shader reads: a
program whose fragment inputs have no vertex output is rejected by wgpu.
The pipeline makes that true:

- The fragment shader keeps the varyings it declares.  A varying the vertex
  shader does not produce is **not** dropped: dropping it would leave the
  fragment shader with a variable it does not know (`UnknownVariable`).
- The vertex shader declares the varyings it does not produce and
  initialises them with zero at the top of `main()`.  The declarations go
  after the header of the shader (the `#version`, the `#extension` and the
  `#define` lines stay at the top) and they use the location and the type of
  the layout.
- A varying that is an array keeps its size: the declaration is
  `out TYPE NAME[SIZE];` and the initialisation is the array constructor of
  the zero values.

The preprocessor resolves the conditionals of the shader, so the vertex
shader that reaches this stage has no `#if` block left: there is nothing to
hoist out of a conditional any more.

---

## The Samplers of a Shader

The samplers of a shader are bound to the textures of the material of the
effect.  The samplers of the Wallpaper Engine shaders are named
`g_Texture0`, `g_Texture1`, ...:

| Sampler | Texture of the material |
|---------|-------------------------|
| `g_Texture0` | the source of the pass (the texture the effect works on) |
| `g_Texture1` | the mask of the pass (the second texture of the material) |
| `g_Texture2` | the noise of the pass (the third texture of the material) |

The binding of a sampler is its position in the shader, the texture it reads
is the one its name refers to.  The bind group, the resolution of the
textures (`g_Texture0Resolution` and the like) and the intermediates of a
multi-pass effect all use `EffectLayout::sampler_texture_slot`.

---

## The Built-in Vertex Stage of an Effect

An effect of Wallpaper Engine that has no vertex shader of its own draws a
full screen quad.  The loader gives it the built-in vertex stage of the
reference engine (`src/scene/loader/shader/builtin.vert`), which is written
in the dialect of the engine, so the preprocessing handles it like any other
vertex shader.  A warning names the shader that has no vertex stage.

Without it the shader compiler has no entry point for the vertex stage and
rejects the whole program.

---

## The Visibility of an Effect

The `visible` flag of an effect is the flag of the scene file.  An effect
that is not visible is not applied at all: it is not compiled and it does not
cost anything.  The flag is a boolean, a number or a user property that
holds one of the two.

---

## `shader/compat.rs` — GLSL Builtin Replacements

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

## `shader/header.rs` — Built-in Headers Loading

**File:** `shader/header.rs`

#### `get_headers(load: impl Fn(&str) -> Option<Vec<u8>>) -> Map<String, String>`

Loads shader header files through the supplied byte loader (the scene's `AssetStore`). Headers are stored under `shaders/common*.h` paths.

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

The conditions of the `#if` and `#elif` directives are evaluated the way C
evaluates them (`shader/c_expr.rs`): the operators have the precedence of C
and the parentheses change the result.

| Form | Example |
|------|---------|
| `defined(NAME)` | `#if defined(VERTICAL)` |
| `!defined(NAME)` | `#if !defined(MASK)` |
| `NAME == VALUE` | `#if BLENDMODE == 26` |
| `NAME != VALUE` | `#if MODE != 3` |
| `NAME >= VALUE`, `NAME <= VALUE` | `#if TEX1FORMAT >= FORMAT_ETC1_RGB8` |
| `!NAME` | `#if !MASK` |
| `NAME` (bare) | `#if VERTICAL` — truthy if defined and not `"0"` |
| `\|\|` | `#if A \|\| B` — lower precedence than `&&` |
| `&&` | `#if A && B` — higher precedence than `\|\|` |
| `(...)` | `#if (A \|\| B) && C` — the parentheses change the result |

A macro that is not defined is zero, like in C.  The macros of the shader
and of the headers are visible to the condition, and the macros of a branch
that is not taken do not exist.

The preprocessor is lenient like the reference engine: a stray `#endif`, an
unknown directive (the `#require` of the engine, for example) and a
conditional that is never closed are warnings, the shader is preprocessed
anyway.
