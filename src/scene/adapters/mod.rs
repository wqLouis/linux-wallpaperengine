pub mod winit_adapter;
pub mod wlr_app;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    /// Scale wallpaper to fill entire output, cropping if aspect ratios differ
    Cover,
    /// Scale wallpaper to fit within output, letterboxing if aspect ratios differ
    Contain,
    /// Stretch wallpaper to exactly match output (ignores aspect ratio)
    Stretch,
}
