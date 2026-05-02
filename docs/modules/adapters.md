# Adapters (`src/scene/adapters/`)

Display backends that create a rendering surface and drive the render loop.

---

## `winit_adapter`

**File:** `winit_adapter.rs`

A standalone window adapter using the [winit](https://crates.io/crates/winit) crate. Useful for debugging or non-Wayland systems.

### Public API

```rust
pub fn start(pkg_path: String)
```

Creates a borderless, fullscreen, always-on-bottom window and runs the render loop.

| Parameter | Description |
|-----------|-------------|
| `pkg_path` | Path to the `.pkg` wallpaper file |

**Behavior:**
- Creates a `WinitApp` that implements `ApplicationHandler`
- On `resumed`: initializes `WgpuApp` with `InitAppSurface::Winit(window)`, calls `load()`
- On `RedrawRequested`: calls `app.render()`, requests next frame
- On `Resized`: calls `app.resize()`

### Internal

```rust
struct WinitApp {
    app: Arc<Mutex<Option<WgpuApp>>>,
    window: Option<Arc<Window>>,
    pkg_path: String,
}
```

`WinitApp` implements `ApplicationHandler` from winit.

---

## `wlr_layer_shell_adapter`

**File:** `wlr_layer_shell_adapter.rs`

Wayland layer-shell adapter. Renders the wallpaper as a background layer in Wayland compositors (sway, Hyprland, river, etc.).

### Public API

```rust
pub fn start(pkg_path: String, resolution: Option<[u32; 2]>)
```

| Parameter | Description |
|-----------|-------------|
| `pkg_path` | Path to the `.pkg` wallpaper file |
| `resolution` | Optional override `[width, height]`. If `None`, uses the scene's native resolution |

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

### Struct

```rust
pub struct Wgpu {
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub app: WgpuApp,
    pub resolution: [u32; 2],
}
```

### Trait Implementations

| Trait | Purpose |
|-------|---------|
| `CompositorHandler` | Surface lifecycle (scale, transform, frame, enter/leave) |
| `OutputHandler` | Output discovery & state |
| `LayerShellHandler` | Layer surface configure (resize) — maintains aspect ratio and calls `app.resize()` + `app.render()` |
| `SeatHandler` | Input device state (unused but required) |
| `ProvidesRegistryState` | Registry state access for macro delegation |

All handler methods for `CompositorHandler`, `OutputHandler`, and `SeatHandler` are no-ops — only `LayerShellHandler::configure` has actual logic for aspect-ratio-correct resizing.

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

Both adapters use `InitAppSurface` from the renderer's `surface` module:

```rust
pub enum InitAppSurface {
    Raw((RawDisplayHandle, RawWindowHandle)),  // Wayland
    Winit(Arc<winit::window::Window>),          // Winit
}
```

Re-exported from `crate::scene::renderer::app::InitAppSurface`.
