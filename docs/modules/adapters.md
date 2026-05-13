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

Module-level documentation describes the two adapters:
- **`winit_adapter`** — Creates an always-on-bottom window using winit. Works on both X11 and Wayland (via XWayland). Supports cursor tracking for depth-parallax effects.
- **`wlr_app`** — Uses the wlr-layer-shell Wayland protocol to render on a `Layer::Background` surface behind all windows. No cursor tracking (Wayland's security model does not allow it for background surfaces).

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

## Surface Initialization

Both adapters use `InitAppSurface` from the renderer's `surface` module:

```rust
pub enum InitAppSurface {
    Raw((RawDisplayHandle, RawWindowHandle)),  // Wayland (wlr adapter)
    Winit(Arc<winit::window::Window>),          // Winit (standalone window)
}
```

Re-exported from `crate::scene::renderer::app::InitAppSurface`.

## Surface Configuration (AppSurface)

The WGPU surface is created with:
- `RENDER_ATTACHMENT` usage
- `PresentMode::Fifo` (vsync)
- `CompositeAlphaMode::Auto`
- First available capability format
- `desired_maximum_frame_latency: 2`
