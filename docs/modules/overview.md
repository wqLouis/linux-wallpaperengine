# Architecture Overview

## The workspace

The repository is a cargo workspace with three packages:

| Package | Directory | What it is |
|---------|-----------|------------|
| `linux-wallpaper-engine` | `.` | the library — the engine itself (`lib` target `linux_wallpaper_engine`) |
| `linux-wallpaper-engine-cli` | `crates/lwe-cli` | the command line interface (binary `linux-wallpaper-engine`) |
| `linux-wallpaper-engine-gui` | `crates/lwe-gui` | the graphical interface (binary `linux-wallpaper-engine-gui`) |

The library stays at the root of the repository: every asset is embedded with
`include_str!`, the `pkg_parser` submodule is referenced by a relative path and
the tests address their data through `CARGO_MANIFEST_DIR` — keeping the root
package keeps all of those working.

```bash
cargo build                # the library + the CLI (the default members)
cargo build-cli            # the CLI only
cargo build-gui            # the GUI only
cargo build --workspace    # everything
cargo test                 # the tests of the library and of the CLI
```

`cargo build-cli` and `cargo build-gui` are the aliases of
`.cargo/config.toml`; they stand for
`cargo build -p linux-wallpaper-engine-cli` and
`cargo build -p linux-wallpaper-engine-gui`.

## Project Structure

```
.
├── Cargo.toml                       # the library package + the workspace root
├── .cargo/config.toml               # the build-cli / build-gui aliases
├── src/
│   ├── lib.rs                       # the library: pub mod scene; collections; engine;
│   │                                #              pub use pkg_parser;
│   ├── collections.rs               # Shared hashbrown + ahash map/set aliases
│   ├── engine/                      # The façade a host application talks to
│   │   ├── mod.rs                   # EngineConfig + the re-exports
│   │   ├── source.rs                # WallpaperSource: an archive or a folder
│   │   ├── metadata.rs              # What wallpapers exist (scanning)
│   │   └── runtime.rs               # How a wallpaper is rendered and controlled
│   ├── pkg_parser/                  # Package file parsing (git submodule)
│   │   └── src/pkg_parser/
│   │       ├── parser.rs            # .pkg file reader
│   │       ├── tex_parser.rs        # .tex texture parser
│   │       ├── video_parser.rs      # Video/GIF metadata & frame extraction
│   │       └── mdl_parser/          # .mdl puppet model parser
│   │   └── src/lib.rs
│   └── scene/
│       ├── mod.rs                   # Root module
│       ├── error.rs                 # EngineError / EngineProblem: the errors
│       ├── adapters/                # Windowing / display backends
│       │   ├── mod.rs               # FitMode enum + the NDC fit scale
│       │   ├── winit_adapter.rs     # Standalone window (winit backend)
│       │   ├── wlr_app/
│       │   │   ├── mod.rs           # Wayland wlr-layer-shell adapter & WlrState
│       │   │   └── scale.rs         # Fractional-scale & viewporter management
│       │   ├── gnome_app/
│       │   │   ├── mod.rs           # Off-screen rendering + D-Bus to the extension
│       │   │   └── frame.rs         # The shared frames in /dev/shm
│       │   └── mac_app/             # macOS (experimental), Swift + Metal
│       ├── loader/                  # Scene data loading & parsing
│       │   ├── mod.rs
│       │   ├── scene.rs             # Root/Camera/General/Object data structures
│       │   ├── scene_loader.rs      # .pkg parser → Scene + AssetStore
│       │   ├── object.rs            # Object/Effect/Pass/Combos + ElementType
│       │   ├── object_loader.rs     # Converts Objects → TextureObject/AudioObject/Node
│       │   ├── model.rs             # Material model JSON definition
│       │   ├── assets_loader.rs     # AssetType classification & typed AssetStore
│       │   ├── project.rs           # project.json: the kind of the wallpaper
│       │   ├── mdl.rs, mip_loader.rs
│       │   └── shader/              # Shader preprocessing & parsing
│       │       ├── mod.rs           # The four stages of the preprocessor
│       │       ├── asset.rs         # ShaderProgram: pre-parsed shader pair
│       │       ├── header.rs        # Built-in GLSL headers loading
│       │       ├── layout.rs        # EffectLayout + std140 size helpers
│       │       ├── preprocessor.rs   # The conditionals of a C preprocessor
│       │       ├── c_expr.rs        # The operators of a C expression
│       │       ├── compat.rs        # The constructs without a GLSL equivalent
│       │       ├── dialect.rs       # The dialect of Wallpaper Engine
│       │       ├── glsl/            # The type rules of GLSL
│       │       └── varying.rs       # The varyings of the compiled branch
│       └── renderer/                # GPU rendering
│           ├── mod.rs
│           ├── app.rs               # WgpuApp: main render orchestrator
│           ├── surface.rs           # AppSurface: wgpu surface + config
│           ├── buffer.rs            # Vertex/index/projection GPU buffers
│           ├── vertex.rs            # Vertex: mesh vertex type
│           ├── load.rs              # Asset loading pipeline entry point
│           ├── projection.rs        # Camera projection matrix
│           ├── post_process.rs      # Sampler, bind group layout, blank texture
│           ├── draw.rs              # DrawObject, DrawQueue
│           ├── effect_bindgroup.rs  # EffectBindGroup: per-effect GPU resources
│           ├── ping_pong.rs         # PingPongTextures: double-buffered render targets
│           ├── intermediate_pass.rs # Multi-effect render pass orchestration
│           ├── render_pass.rs       # Final render pass & uniform writing
│           ├── error_scope.rs       # The error scopes of the device
│           ├── transform.rs, yuv.rs
│           ├── post_processor/      # Shader effect pipeline
│           │   ├── mod.rs
│           │   ├── context.rs        # EffectContext: what a pipeline needs
│           │   ├── effect_param.rs   # UniformLayout: GPU uniform buffer layout
│           │   ├── effect_step.rs    # EffectStep model & bind groups
│           │   ├── pipeline_handler.rs # Effect pipeline creation & caching
│           │   └── pipeline_helpers.rs # Bind group layout helpers
│           └── shader/
│               ├── image.wgsl        # Default WGSL image shader
│               └── yuv2rgb.wgsl      # The conversion of the YUV planes
└── crates/
    ├── lwe-cli/src/main.rs          # The CLI: the arguments and the parser
    └── lwe-gui/src/main.rs          # The GUI: the tray app and the picker
```

## Data Flow

```
                     ┌───────────────────────────────┐
                     │ the façade (src/engine/)      │
                     │  scan_wallpapers() → what      │
                     │  exists, WallpaperHandle →     │
                     │  what runs                     │
                     └───────────────┬───────────────┘
                                     │
             ┌───────────────────────┴────────────────────────┐
             ▼                                                ▼
        the CLI (crates/lwe-cli)                       the GUI (crates/lwe-gui)
             │                                                │
             ▼                                                ▼
        an adapter (wlr / winit / gnome / mac)         the renderer, in process
             │                                                │
             └───────────────────────┬────────────────────────┘
                                     ▼
                              WgpuApp::new()    ← Creates GPU device, surface, buffers, audio stream
                                     │
                                     ▼
                              WgpuApp::load()   ← Loads & parses scene
                                     │
                ┌────────────────────┼────────────────────┐
                ▼                    ▼                    ▼
         Scene::new             ObjectMap            PostProcess
         (.pkg or folder)      ::with_clear          ::new()
                    │         _color()
                    ▼                │
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

## The engine façade

The façade ([engine.md](engine.md)) is the contract between the library and
its hosts:

* **sources** — a wallpaper is an archive (`.pkg`) or a folder (the Steam
  workshop layout); both are first class.
* **metadata** — `scan_wallpapers()` walks a workshop folder and describes
  every wallpaper it finds (title, kind, preview).
* **runtime** — `WallpaperHandle::spawn()` starts the renderer on a worker
  thread; the handle takes the commands and reports the status.
* **errors** — the façade never panics: every failure is an `EngineError` that
  carries its cause, and the chain of causes continues through `Error::source`.

## Key Concepts

### Hash Tables

Assets, effects, bind groups and shader symbols are looked up by name in
tight loops, so the crate uses [`hashbrown`](https://docs.rs/hashbrown)'s
SwissTable with [`ahash`](https://docs.rs/ahash)'s fast hasher instead of
the standard library's SipHash-based maps. The aliases live in
`src/collections.rs`:

```rust
pub type Map<K, V> = hashbrown::HashMap<K, V, ahash::RandomState>;
pub type Set<T> = hashbrown::HashSet<T, ahash::RandomState>;
```

Hash maps have no inherent order, so wherever the iteration order is
observable — generated shader source, pipeline cache keys, uniform
declarations — the entries are visited in sorted key order (`sorted_entries`
/ `sorted_keys`) to keep the output deterministic.

### Four Display Modes

| Mode | Adapter | Use Case |
|------|---------|----------|
| `wlr` | `wlr_app` (Wayland wlr-layer-shell) | Wayland compositors — renders as a background layer |
| `winit` | `winit_adapter` | Standalone window — debugging, X11, or non-Wayland systems |
| `gnome` | `gnome_app` | GNOME Shell — off-screen rendering, streamed to the shell extension |
| `mac` | `mac_app` | macOS (experimental) — an AppKit window in the desktop layer |

### CLI Arguments

```bash
linux-wallpaper-engine [OPTIONS]

Options:
  -p, --path <PATH>          Path to .pkg wallpaper file [default: ./scene.pkg]
  -m, --modes <MODES>        Display mode: wlr, winit, gnome or mac [default: wlr]
  --fit-mode <MODE>          Fit mode: cover, contain, stretch [default: cover]
  --no-effects               Bypass post-process effects, render as static image
  --no-mdl                   Ignore MDL puppet models, render as flat quads
  --target-fps <FPS>         Target frames per second (unset: as fast as possible)
  -l, --log-level <LEVEL>    Log level: verbose, debug, warning, errors [default: warning]
  --assets-path <PATH>       Path to Wallpaper Engine assets/ dir for lazy-loading fallback
```

The `parser` subcommand reads a `.pkg` archive instead of running the engine:

```bash
linux-wallpaper-engine parser -p wallpaper.pkg --list      # the files of the package
linux-wallpaper-engine parser -p wallpaper.pkg -x output/  # unpack it
```

### Scene Loading Pipeline

1. **`Scene::new(path, show_progress, no_mdl)`** — Parses the wallpaper (a `.pkg` file, or the files of a folder), classifying every file with `AssetType` and storing parsed `Asset`s (textures, models, JSON documents, shaders, audio, fonts, video, …)
2. **`Scene::prepare()`** — Pre-parses each shader pair into a `ShaderProgram` (interface layout, combo defaults, source macros, resolved headers)
3. **`ObjectMap::with_clear_color(objects, scene, clear_color, no_mdl)`** — Classifies each object via `ElementType`, converts them into `TextureObject`/`AudioObject`/`Node`, resolves parent-child transforms, propagates visibility, builds solid-colour fallback textures
4. **`DrawQueue::new(...)`** — Creates GPU resources (`DrawObject`, `EffectBindGroup`, `PingPongTextures`) for each texture object

### Render Pipeline

1. **Uniform update** — Write elapsed time, projection matrix, cursor position, and effect parameters to GPU buffers via `render_pass::write_effect_uniforms()`
2. **Intermediate passes** (if effects present) — Ping-pong between two textures, applying each effect as a fullscreen quad pass. The projection matrix is temporarily overridden with identity for NDC rendering.
3. **Final pass** — All objects drawn in a single render pass to the swapchain, using either the original texture or the intermediate ping-pong result

### Shader Effect System

Wallpaper Engine effects use GLSL shaders with custom conventions (`[COMBO]` defines, `// {"material":"key"}` annotations, `texSample2D` calls). The preprocessor has four stages:

1. **`preprocessor`** — Resolves the conditionals of a C preprocessor, inlines the headers the shader includes and collects the macros of the shader and of its headers. The conditions are evaluated by `c_expr`, which implements the operators and the precedence of C.
2. **`compat`** — Rewrites the constructs of the dialect that have no GLSL equivalent (`mul`, `saturate`, `gl_FragColor`, `texSample2D`, …).
3. **`glsl`** — Normalises the types of the shader, so that it satisfies the strict typing rules of GLSL.
4. **`dialect`** — Rewrites the rest of the dialect of the engine: the `mul` calls in their row vector form, the precision qualifiers, the sampler types and the macros of the engine.

The layout of a shader — its uniforms, its samplers and its varyings — is
collected with the same preprocessor and the same macros, so that it always
describes the branch that is really compiled.

### Fit Modes

When running with the `wlr` adapter, the wallpaper can be resized to fit the output:

| Mode | Behavior |
|------|----------|
| `cover` | Scale to fill entire output, cropping if aspect ratios differ |
| `contain` | Scale to fit within output, letterboxing if aspect ratios differ |
| `stretch` | Stretch to exactly match output (ignores aspect ratio) |

The `gnome` and `mac` adapters apply the fit mode to the camera projection
instead (`fit_scale()` in `src/scene/adapters/mod.rs`).

### HiDPI Support (Wayland)

The wlr adapter uses `wp_fractional_scale_manager_v1` + `wp_viewporter` protocols to handle HiDPI outputs correctly. When the compositor doesn't support these protocols, a fallback scale is computed from output mode vs. logical size.

### Lazy-Loading Asset Fallback

When `--assets-path` is provided pointing to the Wallpaper Engine `assets/` directory, the `AssetStore` will lazy-load assets from disk that are not found in the `.pkg` file's in-memory map, classifying each one with `AssetType`.

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
| `gnome` | ✅ Full | The extension reports the pointer position over D-Bus |
| `mac` | ❌ None | The pointer is not tracked |

## The modules

| Document | What it describes |
|----------|-------------------|
| [engine.md](engine.md) | the façade: sources, metadata, runtime, errors |
| [loader.md](loader.md) | the scene loader: the packages, the assets, the shaders |
| [renderer.md](renderer.md) | the wgpu renderer: the device, the surface, the buffers |
| [renderer-passes.md](renderer-passes.md) | the passes of the renderer |
| [post-processor.md](post-processor.md) | the post-process effects |
| [adapters.md](adapters.md) | the display backends |
| [pkg-parser.md](pkg-parser.md) | the `.pkg` archive format |
