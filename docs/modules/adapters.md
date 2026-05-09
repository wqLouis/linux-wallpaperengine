# Adapters (`src/scene/adapters/`)

Display backends that create a rendering surface and drive the render loop.

---

## `winit_adapter`

A standalone window adapter. Useful for debugging or non-Wayland systems.

### `start(pkg_path, no_effects)`

| Parameter | Type | Description |
|-----------|------|-------------|
| `pkg_path` | `String` | Path to the `.pkg` wallpaper file |
| `no_effects` | `bool` | Skip all post-process effects |

Creates a borderless, always-on-bottom, fullscreen window and runs the event loop. Cursor tracking is available (winit receives pointer events even behind other windows).

---

## `wlr_app`

Wayland layer-shell adapter. Renders the wallpaper as a `Background` layer surface on compositors that support `wlr-layer-shell` (Hyprland, Sway, river, etc.).

### `start(pkg_path, fit_mode, no_effects)`

| Parameter | Type | Description |
|-----------|------|-------------|
| `pkg_path` | `String` | Path to the `.pkg` wallpaper file |
| `fit_mode` | `FitMode` | Cover, Contain, or Stretch |
| `no_effects` | `bool` | Skip all post-process effects |

Connects to the Wayland display, creates a background layer surface, and enters a 16 ms fixed-timestep render loop. Cursor parallax is unavailable due to Wayland's security model (background surfaces do not receive pointer events).

### `Wgpu` struct

Holds smithay-client-toolkit registry/seat/output state plus the [`WgpuApp`](../renderer.md) instance.

### `FitMode` enum

| Variant | Description |
|---------|-------------|
| `Cover` | Scale to fill output, crop if aspect ratios differ |
| `Contain` | Scale to fit output, letterbox if aspect ratios differ |
| `Stretch` | Stretch to exactly match output |

---

## Surface Initialization

Both adapters use `InitAppSurface`:

```rust
pub enum InitAppSurface {
    Raw((RawDisplayHandle, RawWindowHandle)),  // Wayland
    Winit(Arc<winit::window::Window>),          // Winit
}
```

Re-exported from `crate::scene::renderer::app::InitAppSurface`.
