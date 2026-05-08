# Architecture Overview

## Project Structure

```
src/
├── main.rs                           # CLI entry point
├── pkg_parser/                       # Package file parsing
│   ├── src/pkg_parser/
│   │   ├── parser.rs                 # .pkg file reader (depkg wrapper)
│   │   └── tex_parser.rs             # .tex texture parser
│   └── src/lib.rs
└── scene/
    ├── mod.rs                        # Root module
    ├── adapters/                     # Windowing / display backends
    │   ├── mod.rs
    │   ├── winit_adapter.rs          # Standalone window (winit backend)
    │   ├── wlr_layer_shell_adapter.rs # Wayland layer shell (wallpaper mode)
    │   └── wlr_app.rs                # Wayland protocol state & trait impls
    ├── loader/                       # Scene data loading & parsing
    │   ├── mod.rs
    │   ├── scene.rs                  # Root/Camera/General data structures
    │   ├── scene_loader.rs           # .pkg file parser
    │   ├── object.rs                # Object/Effect/Pass/Combos definitions
    │   ├── object_loader.rs          # Converts Objects → TextureObject/AudioObject
    │   └── model.rs                 # Material model definition
    └── renderer/                     # GPU rendering
        ├── mod.rs
        ├── app.rs                    # WgpuApp: main render orchestrator
        ├── surface.rs                # AppSurface: wgpu surface + config
        ├── buffer.rs                 # Buffers: vertex/index/projection GPU buffers
        ├── vertex.rs                 # Vertex: mesh vertex type
        ├── load.rs                   # Asset loading pipeline
        ├── projection.rs             # Camera projection matrix
        ├── post_process.rs           # Samplers, blank textures
        ├── draw.rs                   # DrawObject, DrawQueue
        ├── effect_bindgroup.rs       # EffectBindGroup: per-effect GPU resources
        ├── ping_pong.rs              # PingPongTextures: double-buffered render targets
        ├── intermediate_pass.rs      # Multi-effect render pass orchestration
        └── post_processor/          # Shader effect pipeline
            ├── mod.rs
            ├── effect_param.rs       # UniformLayout: GPU uniform buffer layout
            ├── pipeline_handler.rs   # Effect pipeline creation & caching
            ├── pipeline_helpers.rs   # Bindgroup layout helpers
            ├── renderer.rs           # PostProcess stub (WIP)
            ├── shader_compiler.rs    # ShaderEffect parser (alternative)
            ├── shader_preprocessor.rs # Public API: preprocess_pair, preprocess
            ├── shader_header.rs      # Built-in GLSL headers (common.h, etc.)
            ├── transform/
            │   ├── mod.rs            # GLSL → Vulkan transformation
            │   ├── layout.rs         # EffectLayout: shader interface introspection
            │   └── replace.rs        # GLSL builtin → Vulkan builtin replacement
```

## Data Flow

```
CLI args → Adapter (winit/wlr)
              │
              ▼
         WgpuApp::new()    ← Creates GPU device, surface, buffers
              │
              ▼
         WgpuApp::load()   ← Loads scene
              │
   ┌──────────┼──────────┐
   ▼          ▼          ▼
Scene::new  ObjectMap  PostProcess
(.pkg)     .new()      ::new()
              │
              ▼
         DrawQueue::new()  ← Builds DrawObjects, pipelines, effect groups
              │
              ▼
    ┌── render loop ──┐
    │                  │
    │  write_effect_uniforms() │ ← Time, projection, effect params → GPU
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
| `wlr` | `wlr_layer_shell_adapter` | Wayland compositors — renders as a background layer |
| `winit` | `winit_adapter` | Standalone window — debugging or non-Wayland systems |

### CLI Arguments

```bash
linux-wallpaper-engine [OPTIONS]

Options:
  -p, --path <PATH>          Path to .pkg wallpaper file [default: ./scene.pkg]
  -m, --modes <MODES>        Display mode: wlr or winit [default: wlr]
  -d, --dimensions <DIM>     Resolution override (e.g. 1920x1080)
  --fit-mode <MODE>          Fit mode: cover, contain, stretch [default: cover]
  --no-effects               Bypass post-process effects, render as static image
```

### Scene Loading Pipeline

1. **`Scene::new(path)`** — Parses a `.pkg` file into textures (`.tex`), shaders, JSON configs
2. **`ObjectMap::new(objects, scene)`** — Converts raw `Object`/`Effect` definitions into `TextureObject`/`AudioObject`, resolves parent-child transforms
3. **`DrawQueue::new(...)`** — Creates GPU resources (`DrawObject`, `EffectBindGroup`, `PingPongTextures`) for each texture object

### Render Pipeline

1. **Uniform update** — Write elapsed time, projection matrix, and effect parameters to GPU buffers
2. **Intermediate passes** (if effects present) — Ping-pong between two textures, applying each effect as a fullscreen quad pass
3. **Final pass** — All objects drawn in a single render pass to the swapchain

### Shader Effect System

Wallpaper Engine effects use GLSL shaders with custom conventions (`[COMBO]` defines, material key annotations, `texSample2D` calls). The preprocessor:
1. **Collects layout** — Samplers, uniforms, varyings, attributes from both vertex and fragment sources
2. **Transforms GLSL → Vulkan** — Replaces builtins (`mul` → matrix multiply, `texSample2D` → `sampler2D(tex, sampler)`, etc.)
3. **Emits declarations** — Generates proper `layout(binding=N)` declarations for wgpu

### Fit Modes (Wayland)

When running with the `wlr` adapter, the wallpaper can be resized to fit the output:

| Mode | Behavior |
|------|----------|
| `cover` | Scale to fill entire output, cropping if aspect ratios differ |
| `contain` | Scale to fit within output, letterboxing if aspect ratios differ |
| `stretch` | Stretch to exactly match output (ignores aspect ratio) |

### Texture Format Handling

The `.tex` parser automatically handles multiple formats:
- **R8 / RG88** — Single or dual channel, kept as-is (not expanded to RGBA)
- **PNG / JPG / DXT** — RGBA after `parse_to_rgba()`

Mask/noise textures are loaded with their native format: R8Unorm for `.r8`, Rg8Unorm for `.rg88`, Rgba8Unorm otherwise.