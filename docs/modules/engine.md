# The engine façade (`src/engine/`)

The façade is the part of the library a host application talks to.  The command
line interface and the graphical interface use nothing else: they ask the
façade what wallpapers exist, they tell it to start one and they control it.

The façade has three parts:

| Module | Question it answers |
|--------|---------------------|
| [`source`](#sources--wallpapersource) | *Where does the wallpaper come from?* |
| [`metadata`](#the-metadata--scanning) | *Which wallpapers exist?* |
| [`runtime`](#the-runtime) | *How is the wallpaper rendered, and how is it controlled?* |

---

## Sources — `WallpaperSource`

**File:** `src/engine/source.rs`

Wallpaper Engine ships wallpapers in two shapes, and both are first class:

```rust
pub enum WallpaperSource {
    Package(PathBuf),  // a .pkg archive
    Folder(PathBuf),   // a folder with project.json and the files next to it
}
```

A folder is what the Steam workshop holds (`…/workshop/content/431960/<id>/`),
an archive is what the CLI has always opened (`scene.pkg`).

| Method | Description |
|--------|-------------|
| `path()` | the path of the archive or of the folder |
| `auto(path)` | picks the shape by looking at the path: a directory is a folder, everything else is an archive |
| `read_files()` | the files of the wallpaper, keyed by their path *inside* it |

`read_files()` is the important one: it returns the very same map the archive
parser produces — a map from the path of a file inside the wallpaper to its
bytes — but for a folder source it is built by walking the folder.  The paths
are relative to the folder and use `/` as the separator, so a folder source and
an archive source look exactly the same to the loader.  A file that cannot be
read is an `EngineError::Io` that says which path it happened at.

The adapter module offers the helper `wallpaper_source(path) ->
WallpaperSource` ([adapters.md](adapters.md#adapterconfig)), which is the
same as `WallpaperSource::auto`.

---

## The metadata — scanning

**File:** `src/engine/metadata.rs`

A host application does not know in advance where the wallpapers are, so the
façade scans a folder for them.  `scan_wallpapers()` walks a workshop folder
and describes every wallpaper it finds.

A workshop folder holds one folder per wallpaper, and every one of them has a
`project.json` at its root.  The manifest is parsed with the parser of the
loader (`src/scene/loader/project.rs`), which is deliberately lenient: unknown
fields are ignored, a missing `type` means a scene and an unknown `type` is
preserved as `WallpaperKind::Unknown` instead of failing the whole scan.

Each wallpaper is described by a `WallpaperInfo`:

| Field | Description |
|-------|-------------|
| `id` | the name of the folder (or of the file) of the wallpaper |
| `title` | the title of `project.json` |
| `kind` | the `WallpaperKind` of the manifest: a scene, a video, a web page, an application … |
| `source` | the `WallpaperSource` of the wallpaper (a folder inside the workshop) |
| `thumbnail` | the preview image of the manifest, when there is one |

The preview is what the picker of the GUI shows, so the GUI does not have to
load a wallpaper to draw its thumbnail.  It is a `Thumbnail`:

| Variant | Description |
|---------|-------------|
| `Image { bytes, format }` | an encoded image (PNG, JPEG, GIF, …) with its format |
| `Rgba { width, height, bytes }` | raw RGBA pixels (the first frame of a video preview) |

The module offers three functions:

| Function | Description |
|----------|-------------|
| `scan_wallpapers(root: &Path) -> Vec<WallpaperInfo>` | walks a folder of wallpapers and describes every one of them (sorted by title) |
| `describe(source: &WallpaperSource) -> Option<WallpaperInfo>` | describes a single wallpaper — `None` when it has no `project.json`, when the manifest is broken or when it cannot be read at all |
| `resolve_thumbnail(source: &WallpaperSource, project: &Project) -> Option<Thumbnail>` | the preview image the manifest names, looked up in the files of the source |

A wallpaper that cannot be described is skipped: the scan never fails and
never panics.

---

## The runtime

**File:** `src/engine/runtime.rs`

### `EngineConfig`

How a wallpaper is rendered.  It is the same set of options the CLI exposes on
the command line:

| Field | Description |
|-------|-------------|
| `fit_mode` | how the wallpaper is fitted to the output (`cover`, `contain` or `stretch`) |
| `target_fps` | the rate the wallpaper is rendered at: `None` is "as fast as possible", `Some(0)` the static image mode |
| `no_effects` | render the wallpaper without its post-process effects |
| `no_mdl` | render the MDL puppets as flat quads |
| `assets_path` | the Wallpaper Engine assets directory (`Option<PathBuf>`), used as a lazy-loading fallback |
| `show_progress` | whether the loading progress is shown |

`EngineConfig::default()` is the documented configuration: `cover`, no rate
limit (`target_fps: None`), the effects and the puppets enabled
(`no_effects: false`, `no_mdl: false`), no assets directory and no progress
bar.

### `EngineCommand` — what the host asks the renderer to do

| Variant | Description |
|---------|-------------|
| `Switch(WallpaperSource)` | show another wallpaper: the scene is loaded again, the device, the surface and the audio stream stay |
| `SetFps(Option<u32>)` | change the rate the wallpaper is rendered at |
| `Pause` | stop rendering — the events are still dispatched, so the wallpaper stays in control |
| `Resume` | render again |
| `Stop` | stop the wallpaper |

### `EngineStatus` — what the renderer tells the host

| Variant | Description |
|---------|-------------|
| `Starting` | the wallpaper is starting |
| `Running { title, problems }` | the wallpaper is up: `title` is the title of its `project.json` (when it has one) and `problems` are the files that could not be used since the previous status |
| `Failed(EngineError)` | the wallpaper could not be started |
| `Stopped` | the wallpaper stopped |

A status carries the problems that appeared since the previous one: a problem
is reported once, never twice.  The host adds them up and shows them to the
user.

### `WallpaperHandle`

```rust
pub fn spawn(config: EngineConfig, source: WallpaperSource) -> Result<Self, EngineError>
```

`WallpaperHandle::spawn()` starts a wallpaper and returns the handle of it.
The rendering happens on a **worker thread**: the thread owns the renderer (the
device, the surface, the scene) and the handle is the only thing the host
touches, so the interface of the GUI stays responsive while a wallpaper plays.

The call waits for the wallpaper to come up (at most ten seconds) and says how
it went: an error comes back when the wallpaper could not be started.

The handle is the control of the wallpaper:

* it takes the **commands** (`EngineCommand`) — switch the wallpaper, change the
  rate it is rendered at, pause it, resume it, stop it — and hands them to the worker
  thread,
* it reports the **status** (`EngineStatus`) of the wallpaper back to the host.

| Method | Description |
|--------|-------------|
| `switch(&self, source: WallpaperSource) -> Result<(), EngineError>` | shows another wallpaper |
| `set_fps(&self, fps: Option<u32>) -> Result<(), EngineError>` | changes the rate the wallpaper is rendered at |
| `pause(&self) -> Result<(), EngineError>` | stops the rendering, the wallpaper stays in control |
| `resume(&self) -> Result<(), EngineError>` | renders again |
| `stop(&self) -> Result<(), EngineError>` | stops the wallpaper |
| `poll_status(&self) -> Option<EngineStatus>` | the next status of the renderer (non-blocking) |

The handle can be kept by several components: the commands are queued, so the
host never blocks on the renderer.  When the handle is dropped, the wallpaper
is asked to stop and the worker thread is waited for, so it cannot keep the
process alive.

### Why the rendering is in-process

The GUI does **not** spawn the CLI: the engine is a library, so the GUI drives
the renderer itself.  That keeps the frames in one process (no pipe, no copy of
the frame) and lets the GUI show the state of the wallpaper (the errors, the
problems, the progress) as it happens.

---

## The errors

**File:** `src/scene/error.rs`

The façade never panics.  Every failure is an `EngineError` that the host
handles, and every variant is specific, so that a caller can match on it: a
missing audio device is not fatal for a silent wallpaper, a missing graphics
device is.

| Variant | The failure |
|---------|-------------|
| `Package` | the wallpaper package could not be read |
| `Scene` | the wallpaper could not be loaded (the cause is a `SceneError`, see [loader.md](loader.md#sceneerror--why-a-wallpaper-could-not-be-loaded)) |
| `Adapter` | no graphics adapter could be found |
| `Device` | the graphics device could not be created |
| `Surface` | the surface of the output could not be created |
| `Audio` | the audio device could not be opened |
| `Window` | the output window could not be created |
| `Unsupported` | the feature is not available on this platform |
| `Io` | a file could not be read |
| `Bus` | the D-Bus connection of the GNOME adapter failed |
| `Failed` | the failure has no error of its own: the string is the message |

Every variant carries the error that caused it, and the chain of causes
continues through `Error::source`, so a failure is reported with its whole
trace:

```text
the scene.json could not be parsed: expected value at line 3
```

`EngineError::trace()` renders that chain, one cause per line.  The CLI
prints the trace and dies; the GUI shows it in a dialog.

### The problems

Not every problem stops the engine.  A broken model, an unsupported file or a
missing audio device are collected as `EngineProblem` instead of being
swallowed by a log line: the wallpaper still plays, but the user is told that
something was skipped.  The CLI prints them, the GUI shows them.

| Variant | The problem |
|---------|-------------|
| `Asset { path, error }` | a file of the wallpaper could not be read or parsed |
| `Unsupported { path, error }` | a file of the wallpaper is of a kind that is not supported |
| `Audio` | the audio device is not available: the wallpaper plays without sound |
| `Render` | the renderer could not draw the wallpaper |

### Panics are bugs

The release profile keeps `panic = "abort"`: a panic is a bug, not an expected
outcome, and it aborts the whole application — the GUI included.  That is why
the façade returns errors instead of unwinding: a broken wallpaper must not
take the tray application down with it.

---

## See also

* [overview.md](overview.md) — the layout of the workspace and the data flow
* [loader.md](loader.md) — how the files of a wallpaper become a scene
* [renderer.md](renderer.md) — how the scene is drawn
* [adapters.md](adapters.md) — how the frames reach the screen
