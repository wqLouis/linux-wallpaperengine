# Adapters (`src/scene/adapters/`)

Display backends that create a rendering surface and drive the render loop.

---

## `winit_adapter`

**File:** `winit_adapter.rs`

A standalone window adapter using the [winit](https://crates.io/crates/winit) crate. Useful for debugging or non-Wayland systems.

### Public API

```rust
pub fn start(pkg_path: String, no_effects: bool)
```

Creates a borderless, fullscreen, always-on-bottom window and runs the render loop.

| Parameter | Description |
|-----------|-------------|
| `pkg_path` | Path to the `.pkg` wallpaper file |
| `no_effects` | Bypass post-process effects |

**Behavior:**
- Creates a `WinitApp` that implements `ApplicationHandler`
- On `resumed`: creates window, initializes `WgpuApp` with `InitAppSurface::Winit(window)`, calls `load()`
- On `RedrawRequested`: calls `app.render()`, requests next frame
- On `Resized`: calls `app.resize()`

### `WinitApp` struct

```rust
struct WinitApp {
    app: Arc<Mutex<Option<WgpuApp>>>,
    window: Option<Arc<Window>>,
    pkg_path: String,
    no_effects: bool,
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

---

## `wlr_layer_shell_adapter`

**File:** `wlr_layer_shell_adapter.rs`

Wayland layer-shell adapter. Renders the wallpaper as a background layer in Wayland compositors (sway, Hyprland, river, etc.).

### Public API

```rust
pub fn start(pkg_path: String, resolution: Option<[u32; 2]>, fit_mode: FitMode, no_effects: bool)
```

| Parameter | Description |
|-----------|-------------|
| `pkg_path` | Path to the `.pkg` wallpaper file |
| `resolution` | Optional override `[width, height]`. If `None`, uses the scene's native resolution |
| `fit_mode` | How to fit wallpaper to output (Cover, Contain, Stretch) |
| `no_effects` | Bypass post-process effects |

**Behavior:**
1. Connects to the Wayland display
2. Creates a `wlr_layer_surface` as `Background` layer with full anchor, exclusive zone `-1`
3. Creates raw window/display handles for wgpu (`RawWindowHandle::Wayland`)
4. Initializes `WgpuApp` with `InitAppSurface::Raw(...)`
5. Enters a 16ms fixed-timestep render loop (`dispatch_pending` + `render` + sleep)

---

## `wlr_app`

**File:** `wlr_app.rs`

Wayland protocol state container. Holds all smithay-client-toolkit state and implements the required handler traits.

### `FitMode` enum

```rust
pub enum FitMode {
    Cover,    // Scale to fill output, crop if aspect ratios differ
    Contain,  // Scale to fit output, letterbox if aspect ratios differ
    Stretch,  // Stretch to exactly match output
}
```

### `Wgpu` struct

```rust
pub struct Wgpu {
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub app: WgpuApp,
    pub fit_mode: FitMode,
    pub wp_resolution: [u32; 2],
}
```

### Trait Implementations

| Trait | Purpose |
|-------|---------|
| `CompositorHandler` | Surface lifecycle (scale, transform, frame, enter/leave) — all no-ops |
| `OutputHandler` | Output discovery & state — all no-ops |
| `LayerShellHandler` | Layer surface configure — handles aspect-ratio-correct resizing |
| `SeatHandler` | Input device state — all no-ops |
| `ProvidesRegistryState` | Registry state access for macro delegation |

### `LayerShellHandler::configure`

Handles compositor resize requests with fit-mode-aware calculations:

```rust
impl LayerShellHandler for Wgpu {
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        let (new_width, new_height) = configure.new_size;
        
        // Ignore initial (0, 0) configure from some compositors
        if new_width == 0 && new_height == 0 {
            return;
        }
        
        // Calculate layer size based on fit mode
        let (layer_w, layer_h) = match self.fit_mode {
            FitMode::Stretch => (new_width, new_height),
            _ => {
                let scale = match self.fit_mode {
                    FitMode::Cover => f32::max(new_width / wp_w, new_height / wp_h),
                    FitMode::Contain => f32::min(new_width / wp_w, new_height / wp_h),
                    _ => unreachable!(),
                };
                ((wp_w * scale).round() as u32, (wp_h * scale).round() as u32)
            }
        };
        
        layer.set_size(layer_w, layer_h);
        self.app.resize([layer_w, layer_h]);
        self.app.render().unwrap();
    }
}
```

### Delegates

```rust
delegate_compositor!(Wgpu);
delegate_output!(Wgpu);
delegate_seat!(Wgpu);
delegate_layer!(Wgpu);
delegate_registry!(Wgpu);
```

---

## Surface Initialization

Both adapters use `InitAppSurface` from the renderer's `app` module:

```rust
pub enum InitAppSurface {
    Raw((RawDisplayHandle, RawWindowHandle)),  // Wayland
    Winit(Arc<winit::window::Window>),          // Winit
}
```

Re-exported from `crate::scene::renderer::app::InitAppSurface`.