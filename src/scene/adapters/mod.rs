//! Display adapters for rendering wallpapers.
//!
//! Two adapters are provided:
//!
//! * **`winit_adapter`** — Creates an always-on-bottom window using winit.
//!   Works on both X11 and Wayland (via XWayland).  Supports cursor tracking
//!   for depth-parallax effects.
//! * **`wlr_app`** — Uses the wlr-layer-shell Wayland protocol to render on
//!   a `Layer::Background` surface behind all windows.  No cursor tracking
//!   (Wayland's security model does not allow it for background surfaces).

/// How the wallpaper is fitted to the output.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    /// Scale wallpaper to fill entire output, cropping if aspect ratios differ.
    Cover,
    /// Scale wallpaper to fit within output, letterboxing if aspect ratios differ.
    Contain,
    /// Stretch wallpaper to exactly match output (ignores aspect ratio).
    Stretch,
}

pub mod winit_adapter;
pub mod wlr_app;
