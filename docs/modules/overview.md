# Architecture Overview

## Project Structure

```
src/
├── main.rs                           # CLI entry point
├── pkg_parser/                       # Package file parsing (git submodule)
│   └── src/pkg_parser/
│       ├── parser.rs                 # .pkg file reader
│       ├── tex_parser.rs             # .tex texture parser
│       ├── video_parser.rs           # Video/GIF metadata & frame extraction
│       └── mdl_parser.rs             # .mdl puppet model parser
│   └── src/lib.rs
└── scene/
    ├── mod.rs                        # Root module
    ├── adapters/                     # Windowing / display backends
    │   ├── mod.rs                    # FitMode enum definition
    │   ├── winit_adapter.rs          # Standalone window (winit backend)
    │   └── wlr_app/
    │       ├── mod.rs                # Wayland wlr-layer-shell adapter & WlrState
    │       └── scale.rs              # Fractional-scale & viewporter management
    ├── loader/                       # Scene data loading & parsing
    │   ├── mod.rs
    │   ├── scene.rs                  # Root/Camera/General/Object data structures
    │   ├── scene_loader.rs           # .pkg file parser → Scene struct
    │   ├── object.rs                 # Object/Effect/Pass/Combos definitions
    │   ├── object_loader.rs          # Converts Objects → TextureObject/AudioObject/Node
    │   ├── model.rs                  # Material model JSON definition
    │   └── assets_loader.rs          # Lazy-loading bucket wrappers (disk fallback)
    └── renderer/                     # GPU rendering
        ├── mod.rs
        ├── app.rs                    # WgpuApp: main render orchestrator
        ├── surface.rs                # AppSurface: wgpu surface + config
        ├── buffer.rs                 # Vertex/index/projection GPU buffers
        ├── vertex.rs                 # Vertex: mesh vertex type
        ├── load.rs                   # Asset loading pipeline entry point
        ├── projection.rs             # Camera projection matrix
        ├── post_process.rs           # Sampler, bind group layout, blank texture
        ├── draw.rs                   # DrawObject, DrawQueue
        ├── effect_bindgroup.rs       # EffectBindGroup: per-effect GPU resources
        ├── ping_pong.rs              # PingPongTextures: double-buffered render targets
        ├── intermediate_pass.rs      # Multi-effect render pass orchestration
        ├── render_pass.rs            # Final render pass & uniform writing
        ├── post_processor/           # Shader effect pipeline
        │   ├── mod.rs
        │   ├── effect_param.rs       # UniformLayout: GPU uniform buffer layout
        │   ├── pipeline_handler.rs   # Effect pipeline creation & caching
        │   ├── pipeline_helpers.rs   # Bind group layout helpers
        │   ├── shader_header.rs      # Built-in GLSL headers loading
        │   └── transform/
        │       ├── mod.rs            # GLSL → Vulkan transformation (preprocess_pair)
        │       ├── layout.rs         # EffectLayout: shader interface introspection
        │       └── replace.rs        # GLSL builtin → Vulkan builtin replacement
        └── shader/
            └── image.wgsl            # Default WGSL image shader
```

## Data Flow

```
CLI args → Adapter (winit/wlr)
              │
              ▼
         WgpuApp::new()    ← Creates GPU device, surface, buffers, audio stream
              │
              ▼
         WgpuApp::load()   ← Loads & parses scene
              │
   ┌──────────┼──────────┐
   ▼          ▼          ▼
Scene::new  ObjectMap   PostProcess
(.pkg)     ::with_clear → ::new()
              │  _color()
              ▼
         DrawQueue::new()  ← Builds DrawObjects, pipelines, effect bindgroups
              │
              ▼
    ┌── render loop ──┐
    │                  │
    │  write_effect_uniforms() │ ← Time, projection, cursor, effect params → GPU
    │       │          │
    │  intermediate    │ ← Multi-effect objects: ping-pong render passes
    │  passes (opt)    │
    │       │          │
    │  render_final_pass │ ← Single pass: draw all objects → swapchain
    │                  │
    └──────────────────┘
```

## Key Concepts

### Two Display Modes

| Mode | Adapter | Use Case |
|------|---------|----------|
| `wlr` | `wlr_app` (Wayland wlr-layer-shell) | Wayland compositors — renders as a background layer |
| `winit` | `winit_adapter` | Standalone window — debugging, X11, or non-Wayland systems |

### CLI Arguments

```bash
linux-wallpaperengine [OPTIONS]

Options:
  -p, --path <PATH>          Path to .pkg wallpaper file [default: ./scene.pkg]
  -m, --modes <MODES>        Display mode: wlr or winit [default: wlr]
  --fit-mode <MODE>          Fit mode: cover, contain, stretch [default: cover]
  --no-effects               Bypass post-process effects, render as static image
  -l, --log-level <LEVEL>    Log level: verbose, debug, warning, errors [default: warning]
  -x [<DIR>]                 Extract/parse mode (instead of running the engine)
      --parse-tex            Parse .tex textures to PNG images during extraction
      --parse-video          Parse video/GIF metadata during extraction
      --parse-mdl            Parse .mdl puppet model files to JSON during extraction
      --dry-run              Show what would be extracted without writing files
      --assets-path <PATH>   Path to Wallpaper Engine assets/ dir for lazy-loading fallback
```

### Scene Loading Pipeline

1. **`Scene::new(path)`** — Parses a `.pkg` file into textures (`.tex`), models (`.mdl`), JSON configs, and misc binary files (shaders, audio, etc.)
2. **`ObjectMap::with_clear_color(objects, scene, clear_color)`** — Converts raw `Object`/`Effect` definitions into `TextureObject`/`AudioObject`/`Node`, resolves parent-child transforms, propagates visibility, builds solid-colour fallback textures
3. **`DrawQueue::new(...)`** — Creates GPU resources (`DrawObject`, `EffectBindGroup`, `PingPongTextures`) for each texture object

### Render Pipeline

1. **Uniform update** — Write elapsed time, projection matrix, cursor position, and effect parameters to GPU buffers via `render_pass::write_effect_uniforms()`
2. **Intermediate passes** (if effects present) — Ping-pong between two textures, applying each effect as a fullscreen quad pass. The projection matrix is temporarily overridden with identity for NDC rendering.
3. **Final pass** — All objects drawn in a single render pass to the swapchain, using either the original texture or the intermediate ping-pong result

### Shader Effect System

Wallpaper Engine effects use GLSL shaders with custom conventions (`[COMBO]` defines, `// {"material":"key"}` annotations, `texSample2D` calls). The preprocessor:

1. **Collects `EffectLayout`** — Samplers, uniforms, varyings, attributes from both vertex and fragment sources (including transitively `#include`-d headers)
2. **Evaluates preprocessor conditions** — `#ifdef`, `#ifndef`, `#if | NAME == VALUE | ... || ... && ...`, handles `defined()` and negation
3. **Transforms GLSL → Vulkan** — Replaces `mul(a,b)` → `b*a`, `texSample2D(tex,uv)` → `texture(sampler2D(tex,_wm_sampler), uv)`, `saturate(x)` → `clamp(x,0.0,1.0)`, `frac(x)` → `fract(x)`, etc.
4. **Emits declarations** — Generates proper `layout(binding=N)` declarations for wgpu
5. **Hoists conditional varyings** — Fragment shader varyings that only appear inside `#if` blocks in the vertex shader are hoisted to unconditional vertex outputs
6. **Resolves identifier collisions** — Renames `sample` → `sampleColor`, `packed` → `packedValue` to avoid GLSL reserved keywords

### Fit Modes (Wayland)

When running with the `wlr` adapter, the wallpaper can be resized to fit the output:

| Mode | Behavior |
|------|----------|
| `cover` | Scale to fill entire output, cropping if aspect ratios differ |
| `contain` | Scale to fit within output, letterboxing if aspect ratios differ |
| `stretch` | Stretch to exactly match output (ignores aspect ratio) |

### HiDPI Support (Wayland)

The wlr adapter uses `wp_fractional_scale_manager_v1` + `wp_viewporter` protocols to handle HiDPI outputs correctly. When the compositor doesn't support these protocols, a fallback scale is computed from output mode vs. logical size.

### Lazy-Loading Asset Fallback

When `--assets-path` is provided pointing to the Wallpaper Engine `assets/` directory, the `TextureBucket`, `MdlBucket`, `JsonBucket`, and `MiscBucket` wrappers will lazy-load assets from disk that are not found in the `.pkg` file's in-memory buckets.

### Texture Format Handling

The `.tex` parser automatically handles multiple formats:
- **R8 / RG88** — Single or dual channel, kept as-is (not expanded to RGBA)
- **PNG / JPG / DXT** — RGBA after `parse_to_rgba()`
- **DXT1 / DXT5** — BCn compressed texture decoding via `bcndecode`

Mask/noise textures are loaded with their native format: `R8Unorm` for `.r8`, `Rg8Unorm` for `.rg88`, `Rgba8Unorm` otherwise.

### Cursor Tracking

| Adapter | Cursor Support | Details |
|---------|---------------|---------|
| `winit` | ✅ Full | Receives `CursorMoved` events, updates `user_params.cursor_position` for depth-parallax effects |
| `wlr` | ❌ None | Background surfaces can't receive pointer events per Wayland security model; cursor stays at center (no parallax shift) |
