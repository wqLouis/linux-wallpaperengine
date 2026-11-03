# Adapters (`src/scene/adapters/`)

Display backends that create a rendering surface and drive the render loop.

---

## `mod.rs` — FitMode Enum

**File:** `mod.rs`

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    Cover,    // Scale to fill output, crop if aspect ratios differ
    Contain,  // Scale to fit output, letterbox if aspect ratios differ
    Stretch,  // Stretch to exactly match output (ignores aspect ratio)
}
```

Module-level documentation describes the four adapters:
- **`winit_adapter`** — Creates an always-on-bottom window using winit. Works on both X11 and Wayland (via XWayland). Supports cursor tracking for depth-parallax effects.
- **`wlr_app`** — Uses the wlr-layer-shell Wayland protocol to render on a `Layer::Background` surface behind all windows. No cursor tracking (Wayland's security model does not allow it for background surfaces).
- **`gnome_app`** — Renders off-screen and streams the frames to a GNOME Shell extension. GNOME Shell does not implement wlr-layer-shell, so a companion extension is required.
- **`mac_app`** — (macOS, experimental) Creates an AppKit window in the desktop layer of the window server and renders into its `CAMetalLayer`.

---

## `winit_adapter`

**File:** `winit_adapter.rs`

A standalone window adapter using the [winit](https://crates.io/crates/winit) crate. Useful for debugging or non-Wayland systems. Supports cursor tracking for depth-parallax effects.

### Public API

```rust
pub fn start(pkg_path: String, no_effects: bool, assets_path: Option<String>)
```

| Parameter | Description |
|-----------|-------------|
| `pkg_path` | Path to the `.pkg` wallpaper file |
| `no_effects` | Bypass post-process effects |
| `assets_path` | Optional path to Wallpaper Engine assets/ dir for lazy-loading fallback |

**Behavior:**
- Creates a `WinitApp` that implements `ApplicationHandler`
- On `resumed`: creates window, initializes `WgpuApp` with `InitAppSurface::Winit(window)`, calls `load()`
- On `RedrawRequested`: calls `app.render()`, requests next frame
- On `Resized`: calls `app.resize()`
- On `CursorMoved`: updates `app.user_params.cursor_position` (normalized to `[0,1]`) and `cursor_pixel` for depth-parallax effects

### `WinitApp` struct

```rust
struct WinitApp {
    app: Arc<Mutex<Option<WgpuApp>>>,
    window: Option<Arc<Window>>,
    pkg_path: String,
    no_effects: bool,
    assets_path: Option<String>,
}
```

### Window Attributes

```rust
Window::default_attributes()
    .with_decorations(false)                 // Borderless
    .with_fullscreen(Some(Fullscreen::Borderless(None)))
    .with_window_level(WindowLevel::AlwaysOnBottom)
    .with_transparent(true)
    .with_title("Linux wallpaper engine")
```

### Cursor Tracking

The winit adapter receives `CursorMoved` events and normalizes the cursor position to the `[0, 1]` range (top-left origin), updating `WgpuApp.user_params.cursor_position`. This is used by the parallax system (`g_ParallaxPosition` uniform). On Wayland (wlr adapter), cursor tracking is unavailable, so `user_params` stays at the default center `[0.5, 0.5]`.

---

## `wlr_app`

**File:** `wlr_app/mod.rs`

Wayland wlr-layer-shell adapter using the [smithay-client-toolkit](https://crates.io/crates/smithay-client-toolkit) crate. Renders the wallpaper as a `Layer::Background` surface behind all windows.

### Public API

```rust
pub fn start(
    pkg_path: String,
    fit_mode: FitMode,
    no_effects: bool,
    assets_path: Option<String>,
)
```

| Parameter | Description |
|-----------|-------------|
| `pkg_path` | Path to the `.pkg` wallpaper file |
| `fit_mode` | How to fit wallpaper to output (Cover, Contain, Stretch) |
| `no_effects` | Bypass post-process effects |
| `assets_path` | Optional path to Wallpaper Engine assets/ dir for lazy-loading fallback |

**Behavior:**
1. Connects to the Wayland display
2. Binds required globals: compositor, wlr-layer-shell, `wp_fractional_scale_manager_v1`, `wp_viewporter`
3. Creates a `wlr_layer_surface` as `Background` layer with full anchor, exclusive zone `-1`
4. Creates raw window/display handles for wgpu (`RawWindowHandle::Wayland`)
5. Initializes `WgpuApp` with `InitAppSurface::Raw(...)`
6. Enters an event-loop-driven render loop (`dispatch_pending` + `render`)

### `WlrState` struct

```rust
pub struct WlrState {
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub app: WgpuApp,
    pub fit_mode: FitMode,
    pub wp_resolution: [u32; 2],
    pub scale: ScaleState,
    last_logical: Option<(u32, u32)>,
    last_layer: Option<LayerSurface>,
    last_applied_logical: Option<(u32, u32)>,
}
```

### Trait Implementations

| Trait | Purpose |
|-------|---------|
| `CompositorHandler` | Surface lifecycle — all no-ops |
| `OutputHandler` | Output discovery — triggers `reconfigure()` for scale fallback |
| `LayerShellHandler` | Layer surface `configure` — handles fit-mode-aware resizing, fractional scale, viewport |
| `SeatHandler` | Input device state — all no-ops |
| `Dispatch<WpFractionalScaleV1, ...>` | Handles `preferred_scale` events |
| `Dispatch<WpViewporter, ...>` / `Dispatch<WpViewport, ...>` | Placeholder dispatch handlers |
| `ProvidesRegistryState` | Registry state access for macro delegation |

### `LayerShellHandler::configure`

Handles compositor resize requests with fit-mode-aware calculations and fractional-scale:

```rust
impl LayerShellHandler for WlrState {
    fn configure(&mut self, conn, qh, layer, configure, _serial) {
        let (w, h) = configure.new_size;
        if w == 0 && h == 0 { return; }
        self.last_logical = Some((w, h));
        self.last_layer = Some(layer.clone());
        self.reconfigure();  // Computes layer size, applies viewport, resizes swapchain
    }
}
```

### `reconfigure()` method

Recomputes the layer-surface size and WGPU swapchain dimensions:

1. Computes layer-surface size from fit mode + logical size + wallpaper resolution
2. Converts to physical pixels using fractional scale (×120 numerator)
3. Applies viewport destination via `wp_viewport::set_destination()`
4. Calls `app.resize([phys_w, phys_h])`
5. Skips redundant reapplies when nothing changed (`last_applied_scale`, `last_applied_logical`)

### Delegates

```rust
delegate_compositor!(WlrState);
delegate_output!(WlrState);
delegate_seat!(WlrState);
delegate_layer!(WlrState);
delegate_registry!(WlrState);
```

---

## `wlr_app/scale.rs` — ScaleState

**File:** `scale.rs`

Manages the `wp_fractional_scale_v1` and `wp_viewporter` protocol objects for HiDPI support.

### `FractionalScaleData`

```rust
#[derive(Debug)]
pub struct FractionalScaleData;  // Opaque data tag for dispatch
```

### `ScaleState`

```rust
pub struct ScaleState {
    pub mgr: Option<WpFractionalScaleManagerV1>,
    pub fractional: Option<WpFractionalScaleV1>,
    pub viewporter: Option<WpViewporter>,
    pub viewport: Option<WpViewport>,
    pub scale_num: u32,           // Preferred scale numerator (×1/120). Default 120 = 1.0×
    pub scale_received: bool,     // True once a scale has been received or computed
    pub last_applied_scale: u32,
}
```

#### `ScaleState::new(mgr, fractional, viewporter, viewport) -> Self`

Creates a new state with scale starting at 120 (1.0×).

#### `handle_preferred_scale(&mut self, scale: u32)`

Handles a `preferred_scale` event from the compositor (numerator ×120).

#### `compute_from_output(&mut self, output_state, output, fallback_logical) -> bool`

Fallback: computes a scale factor from output mode vs. logical size when the compositor doesn't advertise `wp_fractional_scale_manager_v1`. Returns `true` if a scale was set.

---

## `gnome_app`

**File:** `gnome_app/mod.rs`

GNOME Shell does not implement the `wlr-layer-shell` protocol, so a wallpaper
cannot be rendered into a compositor surface.  This adapter renders the scene
into an **off-screen texture**, copies every frame back to the CPU and hands it
to a GNOME Shell extension (see `gnome-extension/`), which displays the frame
in the shell's background group.

### Public API

```rust
pub fn start(
    pkg_path: String,
    fit_mode: FitMode,
    no_effects: bool,
    no_mdl: bool,
    assets_path: Option<String>,
    target_fps: Option<u32>,
    show_progress: bool,
)
```

### D-Bus protocol

The extension owns the session bus name
`com.github.wqlouis.linux_wallpaper_engine` and exports the interface
`com.github.wqlouis.linux_wallpaper_engine` on the object
`/com/github/wqlouis/linux_wallpaper_engine`.

| Member          | Direction | Description                                        |
|-----------------|-----------|----------------------------------------------------|
| `GetOutput`     | app → ext | `(width, height, scale)` of the primary monitor     |
| `PushFrame`     | app → ext | `(path, width, height)` of a shared RGBA frame       |
| `SetVisible`    | app → ext | show / hide the background                           |
| `OutputChanged` | ext → app | the output geometry changed                          |
| `CursorMoved`   | ext → app | the pointer position, normalised to `[0, 1]`         |

**Behavior:**
1. Connects to the session bus and waits for the extension to own its bus
   name (30 s timeout, then an error message pointing at `install.sh`).
2. Calls `GetOutput` to learn the logical size and the scale factor of the
   primary monitor.
3. Creates the `WgpuApp` with `InitAppSurface::Offscreen { width, height }`
   (the *physical* resolution of the monitor) and loads the scene.
4. Spawns a listener thread for the `OutputChanged` and `CursorMoved`
   signals; the cursor position feeds the depth-parallax uniforms.
5. Render loop: render → copy the frame back to the CPU → write it into a
   shared file → `PushFrame`.

The frames travel through files in `/dev/shm` (see `gnome_app/frame.rs`)
instead of the bus, so the D-Bus traffic stays small even for 4K frames.  Two
files are used alternately, so the extension always reads a complete frame.

### Fit modes

The GNOME backend applies the fit mode to the camera projection instead of
relying on the compositor: the NDC coordinates are scaled so the wallpaper
overflows the viewport (`cover`, cropped), leaves empty borders around it
(`contain`, letterboxed) or fills the viewport exactly (`stretch`).

### `gnome_app/frame.rs` — FrameWriter

**File:** `frame.rs`

Writes the rendered frames into shared files.

| Member | Description |
|--------|-------------|
| `FrameWriter::new(tag)` | creates the two shared files in `/dev/shm` (or the temp dir) |
| `FrameWriter::write(data)` | writes a frame and returns its path |
| `Drop` | removes the files again |

## `mac_app` — macOS (experimental)

**File:** `mac_app/mod.rs`, `mac_app/native/MacDesktop.swift`

macOS has no layer shell either, but the window server has a *desktop* level: a
window placed there is drawn above the desktop picture and below the desktop
icons.  The adapter creates such a window with AppKit, backs it with a
`CAMetalLayer` and renders the wallpaper into it with wgpu's Metal backend.

### The Swift part

The AppKit code is written in Swift (`mac_app/native/MacDesktop.swift`) and
compiled by the `build.rs` of the crate:

```bash
swiftc -O -whole-module-optimization -emit-library -static \
    -o liblwe_mac_desktop.a MacDesktop.swift \
    -framework AppKit -framework Metal -framework QuartzCore
```

The Rust side talks to it through a small C ABI:

| Function | Description |
|----------|-------------|
| `lwe_mac_screen_size(w, h, scale)` | the size of the primary screen (in points) and its scale factor |
| `lwe_mac_create_window(w, h)` | creates the desktop window, returns the `NSView` that hosts the Metal layer |
| `lwe_mac_pump_events()` | processes the pending AppKit events |
| `lwe_mac_destroy_window()` | closes the window again |

### Public API

```rust
pub fn start(
    pkg_path: String,
    fit_mode: FitMode,
    no_effects: bool,
    no_mdl: bool,
    assets_path: Option<String>,
    target_fps: Option<u32>,
    show_progress: bool,
)
```

**Behavior:**
1. Asks the Swift side for the size of the primary screen.
2. Creates the desktop window (level 20 = `kCGDesktopWindowLevel`, above the
   desktop picture and below the desktop icons) and takes its `NSView` as a
   `raw_window_handle::AppKitWindowHandle`.
3. Creates the `WgpuApp` with `InitAppSurface::Raw(...)` — wgpu's Metal
   backend picks the `CAMetalLayer` of the view.
4. Render loop: render → pump the AppKit events.

### Limitations

* Only the primary screen is covered.
* The window is created once and is not moved when the screen layout
  changes.
* The pointer is not tracked, so the depth parallax effects are unavailable.

## Surface Initialization

All adapters use `InitAppSurface` from the renderer's `surface` module:

```rust
pub enum InitAppSurface {
    Raw((RawDisplayHandle, RawWindowHandle)),  // Wayland (wlr adapter)
    Winit(Arc<winit::window::Window>),          // Winit (standalone window)
    Offscreen { width: u32, height: u32 },      // GNOME adapter
}
```

Re-exported from `crate::scene::renderer::app::InitAppSurface`.

The `AppSurface` holds a `SurfaceTarget`:

| Variant | Description |
|---------|-------------|
| `Swapchain(Surface)` | a window swapchain, the frame is presented by the compositor |
| `Offscreen(OffscreenTarget)` | an off-screen texture (sRGB), the frame is copied back to the CPU |

`WgpuApp::read_offscreen_frame(&mut dst)` copies the off-screen frame into a
tightly packed RGBA8 buffer, and `WgpuApp::set_view_scale(sx, sy)` scales the
camera projection in NDC space (used by the fit modes).

## Surface Configuration (AppSurface)

The WGPU surface is created with:
- `RENDER_ATTACHMENT` usage
- `PresentMode::Fifo` (vsync)
- `CompositeAlphaMode::Auto`
- First available capability format
- `desired_maximum_frame_latency: 2`
