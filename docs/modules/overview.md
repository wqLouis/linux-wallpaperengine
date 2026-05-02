# Architecture Overview

## Project Structure

```
src/
├── main.rs                       # CLI entry point
└── scene/
    ├── mod.rs                    # Root module
    ├── adapters/                 # Windowing / display backends
    │   ├── mod.rs
    │   ├── winit_adapter.rs      # Standalone window (Winit backend)
    │   ├── wlr_layer_shell_adapter.rs  # Wayland layer shell (wallpaper mode)
    │   └── wlr_app.rs            # Wayland protocol state & trait impls
    ├── loader/                   # Scene data loading & parsing
    │   ├── mod.rs
    │   ├── scene.rs              # Root/Camera/General data structures
    │   ├── scene_loader.rs       # .pkg file parser
    │   ├── object.rs             # Object/Effect/Pass definitions
    │   ├── object_loader.rs      # Converts raw Objects → TextureObject/AudioObject
    │   └── model.rs              # Material Model definition
    └── renderer/                 # GPU rendering
        ├── mod.rs
        ├── app.rs                # WgpuApp: main render orchestrator
        ├── surface.rs            # AppSurface: wgpu surface + config
        ├── buffer.rs             # Buffers: vertex/index/projection GPU buffers
        ├── vertex.rs             # Vertex: mesh vertex type
        ├── load.rs               # Asset loading pipeline
        ├── projection.rs         # Camera projection matrix
        ├── post_process.rs       # PostProcess: samplers, blank textures
        ├── draw.rs               # DrawObject, DrawQueue
        ├── effect_bindgroup.rs   # EffectBindGroup: per-effect GPU resources
        ├── ping_pong.rs          # PingPongTextures: double-buffered render targets
        ├── intermediate_pass.rs  # Multi-effect render pass orchestration
        └── post_processor/       # Shader effect pipeline
            ├── mod.rs
            ├── effect_param.rs   # UniformLayout: GPU uniform buffer layout
            ├── pipeline_handler.rs   # Effect pipeline creation & caching
            ├── pipeline_helpers.rs   # Defines collection, bindgroup layout helpers
            ├── renderer.rs       # WIP: effect post-processing
            ├── shader_compiler.rs    # ShaderEffect parser (variables, combos)
            ├── shader_preprocessor.rs # Public API: preprocess_pair, preprocess
            ├── shader_header.rs      # Built-in GLSL headers (common.h, etc.)
            ├── shader_layout.rs      # EffectLayout: shader interface introspection
            ├── shader_transform.rs   # GLSL → Vulkan code transformation
            └── shader_replace.rs     # GLSL builtin → Vulkan builtin replacement
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
    │  write uniforms  │ ← Per-frame: time, projection, effect params
    │       │          │
    │  intermediate    │ ← Multi-effect objects: ping-pong render passes
    │  passes (opt)    │
    │       │          │
    │  final pass      │ ← Single pass: draw all objects → swapchain
    │                  │
    └──────────────────┘
```

## Key Concepts

### Two Display Modes

| Mode | Adapter | Use Case |
|------|---------|----------|
| `wlr` | `wlr_layer_shell_adapter` | Wayland compositors — renders as a background layer |
| `winit` | `winit_adapter` | Standalone window — debugging or non-Wayland systems |

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
