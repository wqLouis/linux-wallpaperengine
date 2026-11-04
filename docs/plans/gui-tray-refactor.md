# Decoupled engine + CLI/GUI build options

Branch: `feat/gui`

## Goals

1. The wallpaper engine becomes an **importable library crate** (no binary
   assumptions, no `main.rs`), so other apps can embed it.
2. Two build options:
   * `lwe-cli` — the existing command line app (unchanged UX).
   * `lwe-gui` — a native GUI built with **gpui-ce** (GPUI Community
     Edition), able to run as a **system tray app**.
3. The engine core (scene loading + rendering) must not depend on any
   display backend, so the GUI and the CLI are peers, not parent/child.

## Layout (workspace)

```
Cargo.toml                    # workspace root + the library package
src/lib.rs                    # the library crate "linux-wallpaper-engine"
src/scene/...                 # loader + renderer + adapters (unchanged paths)
src/pkg_parser/               # git submodule (path dependency, unchanged)
crates/lwe-cli/               # binary: the CLI
crates/lwe-gui/               # binary: the gpui-ce tray app
```

Why the library stays at the repository root: every asset is embedded with
`include_str!`, the `pkg_parser` submodule is referenced by a relative
path and the tests address their data through `CARGO_MANIFEST_DIR` — keeping
the root package keeps all of those working.

## Build options

```bash
cargo build -p lwe-cli           # CLI only
cargo build -p lwe-gui           # GUI only (needs gpui-ce + its system deps)
cargo build --workspace          # both
```

## Decisions

* **gpui-ce 0.3.0** (crates.io package `gpui-ce`, lib `gpui`).  It builds
  here in ~37 s and needs no extra system packages (wayland, x11, egl,
  fontconfig, freetype, libdrm, vulkan are all present).  The platform
  crate is `gpui-ce-platform` (lib `gpui_ce_platform`), renamed to
  `gpui_platform` in our manifest so the upstream examples apply as-is.
* **Tray icon**: `ksni` 0.3 — a pure Rust StatusNotifierItem implementation
  on **zbus 5**, the same D-Bus major the project already uses.  It brings
  its own menu support (`ksni::menu`), so no GTK/GLib main loop has to be
  integrated with the gpui event loop.  The tray runs on its own thread;
  the gpui app owns the main thread.
* **Quit mode**: `QuitMode::Explicit` — closing the picker window must not
  kill the tray app.
* **Wallpaper sources**: Wallpaper Engine ships wallpapers as *folders*
  (`steamapps/workshop/content/431960/<id>/`) but the engine only opens
  `.pkg` archives.  The library gets a source abstraction so that a
  folder and an archive are both first class.
* The GUI drives the renderer **in process** (the engine is a library now),
  not by spawning the CLI.

## Status

- [x] branch `feat/gui`
- [x] exploration: engine API map, gpui-ce API, wallpaper discovery
- [ ] library crate split (`src/main.rs` → `src/lib.rs` + `crates/lwe-cli`)
- [ ] engine facade API (`WallpaperSource`, discovery, thumbnails, runner)
- [ ] folder-backed wallpaper sources
- [ ] GUI crate (gpui-ce + ksni tray + picker window)
- [ ] docs + packaging
