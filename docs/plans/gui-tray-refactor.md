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
src/engine/                   # the façade: sources, metadata, runtime
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
cargo build                # the library + the CLI (the default members)
cargo build-cli            # the CLI only
cargo build-gui            # the GUI only (needs gpui-ce + its system deps)
cargo run-cli              # the CLI, with its arguments
cargo run-gui              # the GUI, the tray application
cargo build --workspace    # everything
cargo test                 # the tests of the library and of the CLI
```

The aliases live in `.cargo/config.toml`; they stand for the package names of
the workspace:

| Package | Directory | Binary |
|---------|-----------|--------|
| `linux-wallpaper-engine` | `.` | — (the library `linux_wallpaper_engine`) |
| `linux-wallpaper-engine-cli` | `crates/lwe-cli` | `lwe-cli` |
| `linux-wallpaper-engine-gui` | `crates/lwe-gui` | `lwe-gui` |

## Decisions

* **gpui-ce 0.3.0** (crates.io package `gpui-ce`, lib `gpui`).  It builds
  here in ~37 s.  Its Linux backend links **fontconfig** and **xkbcommon**, so
  those two packages are needed to build the GUI (`libfontconfig-dev` /
  `fontconfig-devel` / `fontconfig` and `libxkbcommon-dev` /
  `libxkbcommon-devel` / `libxkbcommon`).  The platform crate is
  `gpui-ce-platform` (lib `gpui_ce_platform`), renamed to `gpui_platform` in
  our manifest so the upstream examples apply as-is.
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
* **Errors instead of panics**: the façade returns an `EngineError` — an enum
  with a variant per failure (`Package`, `Scene`, `Adapter`, `Device`,
  `Surface`, `Audio`, `Window`, `Unsupported`, `Io`) — and every variant
  carries the error that caused it, so the chain of causes continues through
  `Error::source` and the failure is reported with its whole trace.  The
  problems that do not stop the engine (a broken asset, an unsupported file, a
  missing audio device) are collected as `EngineProblem` instead of being
  swallowed by a log line.  The CLI prints the trace and dies, the GUI shows
  it in a dialog.
* **Panics stay fatal**: the release profile keeps `panic = "abort"`.  A
  panic is a bug, not an expected outcome, and it aborts the whole
  application — the GUI included.  That is why the façade returns errors
  instead of unwinding: a broken wallpaper must not take the tray app down.

## The final design

### The façade (`src/engine/`)

The façade is the contract between the library and its hosts.  It has three
parts:

* **sources** (`source.rs`) — `WallpaperSource::Package` (a `.pkg` archive)
  and `WallpaperSource::Folder` (the Steam workshop layout).  `read_files()`
  returns the files of the wallpaper keyed by their path *inside* it: the map
  the archive parser produces, or the same map built by walking a folder.
  The loader does not care which of the two it got.
* **metadata** (`metadata.rs`) — `scan_wallpapers()` walks a workshop folder
  and describes every wallpaper it finds: its source, its title, its kind
  (from `project.json`) and its preview image.  The preview is what the
  picker of the GUI shows, so a wallpaper is not loaded to draw its
  thumbnail.
* **runtime** (`runtime.rs`) — `WallpaperHandle::spawn()` starts the renderer
  on a **worker thread** and hands out the handle: the handle takes the
  commands (`EngineCommand`) and reports the status (`EngineStatus`) of the
  wallpaper.  The host never blocks on the renderer.

The façade never panics; see the decisions above.

### The rendering is in-process

The GUI does **not** spawn the CLI: the engine is a library, so the GUI
drives the renderer itself.  The frames stay in one process (no pipe, no copy
of the frame), and the GUI can show the state of the wallpaper (the errors,
the problems, the progress) as it happens.  The rendering happens on a worker
thread, so the interface stays responsive while a wallpaper plays.

### The tray

The GUI is a **system tray application**: a `ksni` StatusNotifierItem on the
D-Bus (the tray icon of the desktop), on its own thread, while the gpui
application owns the main thread.  Clicking the icon opens a picker with the
wallpapers of the workshop folder the user chose; the wallpaper that is
picked is rendered as the desktop background.  On the first start the GUI
asks for the workshop folder.

### The interface

The window has **two pages** and no sidebar: the wallpapers (a grid of
cards, each one a preview, a name and a kind) and the settings (the folder
of the wallpapers, how the wallpaper is fitted, the frame rate, the effects
of the scene, the puppet models and the assets folder of Wallpaper Engine).
The settings live in `~/.config/linux-wallpaper-engine/gui.json` and are
applied at once: the frame rate is a command of the engine, the other
settings need the renderer to be started again.

The look is quiet: greys (no hue at all), hairline borders, soft shadows
grooves, generous rounding.  The tokens live in
`crates/lwe-gui/src/design.rs`; the rounding of nested surfaces follows one
rule: the outer rounding is the inner rounding plus the gap that separates
them, so the corners of two nested rectangles are arcs of the same circle.

## Status

- [x] branch `feat/gui`
- [x] exploration: engine API map, gpui-ce API, wallpaper discovery
- [x] library crate split (`src/main.rs` → `src/lib.rs` + `crates/lwe-cli` + `crates/lwe-gui`), the workspace with its default members and the `build-cli` / `build-gui` aliases
- [x] the error type of the engine: `EngineError` (with the chain of the causes) and `EngineProblem`
- [x] folder-backed wallpaper sources (`WallpaperSource::Package` / `WallpaperSource::Folder`)
- [x] docs + packaging (`README.md`, `docs/modules/*`, `packaging/arch/*`)
- [x] the façade of the runtime: `scan_wallpapers()`, `WallpaperInfo`, `WallpaperHandle::spawn()`, `EngineCommand`, `EngineStatus`
- [x] GUI crate (gpui-ce + ksni tray + picker window)
- [x] the interface: the wallpapers and the settings, the design tokens, the icons, the
  settings that are remembered and applied
