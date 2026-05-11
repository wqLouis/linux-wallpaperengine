# Linux Wallpaper Engine

[![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Wgpu](https://img.shields.io/badge/Wgpu-FF5A03?style=for-the-badge&logoColor=white)](https://github.com/gfx-rs/wgpu)
[![Vulkan](https://img.shields.io/badge/Vulkan-AC162C?style=for-the-badge&logo=vulkan&logoColor=white)](https://www.vulkan.org/)
![AUR Version](https://img.shields.io/aur/version/linux-wallpaper-engine-git)

This project is an attempt to bring [Wallpaper Engine](https://www.wallpaperengine.io/en) compatibility to Linux (and potentially macOS). It is written in Rust and uses `wgpu` with Vulkan (and Metal on macOS) to render interactive 3D wallpapers.

> **Status:** The software is functional but still under active development. Many wallpapers with post-processing effects work, but some features (full animation, video playback) are still incomplete.

https://github.com/user-attachments/assets/16891f80-30ca-482c-9f25-17b0b8fdeca5

## Features

### Rendering
- **Hardware-accelerated rendering** via `wgpu` (Vulkan/Metal backends)
- **GLSL shader support** — Wallpaper Engine `.frag`/`.vert` shaders are translated (via a GLSL→WGSL preprocessor) and compiled at runtime for post-processing effects
- **Orthographic camera** from scene.json parameters (look-at + orthographic projection)
- **Alpha blending** with configurable blend modes
- **Post-processing pipeline** with ping-pong multi-pass rendering for effects (bloom, water ripples, etc.)
- **Per-frame uniforms**: `g_Time`, `g_ModelViewProjectionMatrix`, `g_Screen`, `g_ParallaxPosition`, and named material constants
- **Mask and noise texture support** in post-processing effects
- **No-effect mode** (`--no-effects`) to render a static wallpaper image for debugging

### Display Adapters
- **wlr-layer-shell (Wayland):** Renders as a `Layer::Background` surface behind all windows using the `wlr_layer_shell` protocol. Supports fractional scaling via `wp-fractional-scale-v1` and `wp-viewporter`.
- **Winit (X11/Wayland):** Creates an always-on-bottom window with cursor tracking for depth-parallax effects.

### Package Parsing (`pkg_parser`)
- **`.pkg` file extraction and parsing** — reads packaged wallpaper files
- **`.tex` texture parsing** — supports DXT1, DXT5, R8, RG88, PNG, JPEG formats with automatic format detection and LZ4 decompression. Can export to PNG or convert to RGBA for rendering
- **`.mdl` puppet model parsing** — reads MDLV0023 format with control points, triangles, bones (MDLS), and animation (MDLA) sections. Serializes to JSON
- **Video/GIF metadata parsing** — detects MP4, WebM, GIF formats and can extract GIF frames
- **Dry-run mode** (`--dry-run`) to preview extraction without writing files

### Audio
- **Audio playback** via `rodio` with looping support for scene audio tracks

### CLI Features
- Two display modes: `wlr` (default, Wayland background) and `winit` (X11/Wayland window)
- Wallpaper fit modes: `cover`, `contain`, `stretch`
- Extract/parse mode (`-x`): extract and optionally convert `.tex`→PNG, parse videos, parse `.mdl` models to JSON
- Configurable log levels: `verbose`, `debug`, `warning` (default), `errors`

## Requirements

* **Rust** (Latest stable version; edition 2024)
* **Vulkan Drivers:** Ensure your GPU drivers support Vulkan (Mesa for AMD/Intel, proprietary drivers for Nvidia)
* **macOS Support:** Not tested (no access to a Mac), but Metal backend is included
* **Wallpaper Engine Assets:** You must have legal access to the `.pkg` files (e.g., via a purchased copy of Wallpaper Engine on Steam)

### Dependencies
- `wgpu` 28.0 with `glsl` feature (Naga GLSL frontend)
- `winit` 0.30 for the windowed adapter
- `smithay-client-toolkit` 0.20 + `wayland-client` 0.31 for wlr-layer-shell
- `glam` 0.31 for linear algebra
- `clap` 4.5 for CLI argument parsing
- `rodio` 0.21 for audio playback
- `serde` / `serde_json` for scene JSON parsing

## Installation

### From source

1.  Clone the repository:
    ```bash
    git clone https://github.com/wqLouis/linux-wallpaper-engine.git
    cd linux-wallpaper-engine
    ```

2.  Build the project:
    ```bash
    cargo build --profile=release
    ```

3.  Install:
    ```bash
    cargo install --path . --profile=release
    ```

### For Arch users

```bash
paru -S linux-wallpaper-engine-git
```

## Usage

```bash
# Run a wallpaper (default wlr mode)
linux-wallpaper-engine -p path/to/wallpaper.pkg

# Run with winit adapter (window with cursor tracking for parallax)
linux-wallpaper-engine -p path/to/wallpaper.pkg -m winit

# Extract and parse a .pkg file
linux-wallpaper-engine -p path/to/wallpaper.pkg -x [output_dir]

# Extract with texture conversion to PNG
linux-wallpaper-engine -p path/to/wallpaper.pkg -x --parse-tex

# Extract with video/GIF metadata parsing
linux-wallpaper-engine -p path/to/wallpaper.pkg -x --parse-video

# Extract with MDL puppet model JSON export
linux-wallpaper-engine -p path/to/wallpaper.pkg -x --parse-mdl

# Dry run (show what would be extracted)
linux-wallpaper-engine -p path/to/wallpaper.pkg -x --dry-run

# No effects mode (debug, renders static image)
linux-wallpaper-engine -p path/to/wallpaper.pkg --no-effects

# Change wallpaper fit mode
linux-wallpaper-engine -p path/to/wallpaper.pkg --fit-mode contain

# Verbose logging
linux-wallpaper-engine -p path/to/wallpaper.pkg -l verbose
```

### CLI Arguments
| Argument | Description | Default |
|----------|-------------|--------|
| `-p` / `<path>` | Path to `.pkg` file | `./scene.pkg` |
| `-m` / `<modes>` | Display mode: `wlr` or `winit` | `wlr` |
| `--fit-mode` | Wallpaper fit: `cover`, `contain`, `stretch` | `cover` |
| `--no-effects` | Skip post-processing, render static image | `false` |
| `-l` / `--log-level` | `verbose`, `debug`, `warning`, `errors` | `warning` |
| `-x` / `[output]` | Extract mode (optionally specify output dir) | disabled |
| `--parse-tex` | Convert `.tex` textures to PNG (extract mode) | `false` |
| `--parse-video` | Parse video/GIF metadata (extract mode) | `false` |
| `--parse-mdl` | Parse `.mdl` puppet models to JSON (extract mode) | `false` |
| `--dry-run` | Show extracted files without writing | `false` |

## Project Structure

```
src/
├── main.rs                       # CLI entry point with clap argument parsing
├── pkg_parser/                   # (standalone crate) .pkg file parser
│   └── src/pkg_parser/
│       ├── parser.rs             # .pkg file format reading & extraction
│       ├── tex_parser.rs         # .tex texture loading (LZ4, DXT, PNG, JPEG)
│       ├── video_parser.rs       # Video/GIF metadata parsing & frame extraction
│       └── mdl_parser.rs         # MDL puppet model parsing & JSON export
└── scene/
    ├── mod.rs                    # Module declarations
    ├── loader/
    │   ├── scene.rs              # scene.json schema types (Root, Camera, General, etc.)
    │   ├── scene_loader.rs       # Scene loading from .pkg (parallel texture parsing)
    │   ├── object.rs             # Object JSON schema with all WP Engine properties
    │   ├── object_loader.rs      # ObjectMap construction (texture/audio/node hierarchy)
    │   └── model.rs              # Model JSON schema
    ├── renderer/
    │   ├── app.rs                # WgpuApp: main GPU state & render loop
    │   ├── surface.rs            # Surface abstraction (raw handles + winit)
    │   ├── load.rs               # Asset loading & pipeline creation
    │   ├── buffer.rs             # Vertex/index/projection GPU buffers
    │   ├── draw.rs               # DrawQueue & DrawObject construction
    │   ├── vertex.rs             # Vertex type & NDC vertices
    │   ├── projection.rs         # Orthographic camera projection
    │   ├── render_pass.rs        # Final render pass & uniform writing
    │   ├── intermediate_pass.rs  # Ping-pong effect render passes
    │   ├── effect_bindgroup.rs   # Effect bind group construction
    │   ├── ping_pong.rs          # Ping-pong texture pair management
    │   ├── post_process.rs       # Post-process sampler & layout
    │   └── post_processor/       # GLSL→WGSL shader preprocessing
    │       ├── shader_header.rs  # Shader include headers (common.h, etc.)
    │       ├── shader_headers/   # GLSL include files (*.h)
    │       ├── effect_param.rs   # Uniform layout & per-frame parameter population
    │       ├── pipeline_handler.rs # Effect pipeline creation & caching
    │       ├── pipeline_helpers.rs # Defines collection & bind group layout
    │       └── transform/        # Shader source transformations
    │           ├── layout.rs     # EffectLayout generation from GLSL
    │           ├── mod.rs        # Preprocessing pipeline (header injection, varying/attribute translation)
    │           └── replace.rs    # GLSL→GLSL fixes (mul, saturate, texSample2D, etc.)
    └── adapters/
        ├── mod.rs               # FitMode enum
        ├── winit_adapter.rs     # Winit window adapter (always-on-bottom, cursor tracking)
        └── wlr_app/
            ├── mod.rs           # wlr-layer-shell Wayland adapter (Background layer)
            └── scale.rs         # Fractional scale state management
```

## Known Issues & Limitations

* **Video playback:** Video textures (mp4/webm inside .tex files) are detected but not decoded at runtime. They will display as a static frame. GIF textures may work partially.
* **Animation:** Bone animation from `.mdl` puppet models is parsed but not played back.
* **Shader compatibility:** Some Wallpaper Engine shader constructs may not translate correctly. The GLSL→WGSL preprocessing pipeline handles common cases but edge cases exist.
* **Cursor tracking on Wayland:** The wlr-layer-shell adapter cannot receive pointer events on `Layer::Background` surfaces due to Wayland's security model. Depth-parallax effects that depend on cursor position are unavailable in wlr mode.
* **macOS:** Untested.

## Roadmap

- [x] Improve stability and error handling
- [x] Implement audio support
- [x] .mdl file parsing
- [x] Shader preprocessing and effect pipeline
- [x] wlr-layer-shell Wayland adapter
- [x] Multiple wallpaper fit modes
- [x] .pkg extraction and file parsing tools
- [x] Texture format detection and conversion (DXT, R8, RG88, PNG, JPEG)
- [x] Post-processing effects pipeline (ping-pong multi-pass)
- [ ] Video texture playback
- [ ] Puppet model animation
- [ ] Config file support
- [ ] Multi-monitor support

## Contributing

Contributions are welcome!

## License

This project is licensed under the GPLv3 License - see the LICENSE file for details.

## Disclaimer

This project is not affiliated with or endorsed by Wallpaper Engine. Please support the original software by purchasing it on Steam.
