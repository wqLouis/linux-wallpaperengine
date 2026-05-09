# Architecture Overview

## Project Structure

```
src/
├── main.rs                  # CLI entry point
├── pkg_parser/              # .pkg file format parsing
└── scene/
    ├── adapters/            # Display backends (winit, wlr layer-shell)
    ├── loader/              # Scene data loading & parsing
    └── renderer/            # GPU rendering pipeline
```

## Data Flow

```
CLI args → Adapter (winit/wlr)
              │
              ▼
         WgpuApp::new()    ← GPU device, surface, buffers
              │
              ▼
         WgpuApp::load()   ← Parse .pkg, build draw queue
              │
              ▼
     ┌── render loop ──┐
     │  uniform upload  │ ← Time, projection, effect params → GPU
     │  effect passes   │ ← Ping-pong multi-pass effects (if any)
     │  final pass      │ ← Draw all objects → swapchain
     └──────────────────┘
```

## Display Modes

| Mode | Adapter | Use Case |
|------|---------|----------|
| `wlr`  | `wlr_app`    | Wayland compositors — renders as a layer-shell background |
| `winit` | `winit_adapter` | Standalone window — debugging or non-Wayland |

## CLI Arguments

| Argument | Default | Description |
|----------|---------|-------------|
| `-p, --path` | `./scene.pkg` | Path to the `.pkg` wallpaper file |
| `-m, --modes` | `wlr` | Display backend: `wlr` or `winit` |
| `--fit-mode` | `cover` | How to fit the wallpaper: `cover`, `contain`, or `stretch` |
| `--no-effects` | `false` | Skip all post-process effects, render as static image |

## Fit Modes (Wayland)

| Mode | Behavior |
|------|----------|
| `cover` | Scale to fill output, cropping if aspect ratios differ |
| `contain` | Scale to fit within output, letterboxing if aspect ratios differ |
| `stretch` | Stretch to exactly match output (ignores aspect ratio) |

## Scene Loading Pipeline

1. **`Scene::new(path)`** — Opens `.pkg`, extracts textures, JSON configs, shaders, audio
2. **`ObjectMap::new(objects, scene)`** — Converts raw Object definitions into render-ready TextureObject/AudioObject, resolves parent-child transforms
3. **`DrawQueue::new(...)`** — Creates GPU draw objects, effect pipelines, and ping-pong textures

## Render Pipeline

1. **Uniform update** — Upload elapsed time, projection matrix, and effect parameters to GPU
2. **Intermediate passes** (if effects present) — Ping-pong between two textures, applying each effect as a fullscreen quad
3. **Final pass** — Draw all objects to the swapchain in a single render pass

## Texture Format Handling

The `.tex` parser handles multiple formats:
- **R8 / RG88** — Single or dual channel, kept as-is (not expanded)
- **PNG / JPG / DXT** — Expanded to RGBA by `parse_to_rgba()`

Mask/noise textures use native format: `R8Unorm` for `.r8`, `Rg8Unorm` for `.rg88`, `Rgba8Unorm` otherwise.
