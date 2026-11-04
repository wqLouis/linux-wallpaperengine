//! The engine façade.
//!
//! Everything a host application needs to show a wallpaper and to browse the
//! installed ones, without knowing anything about the internals:
//!
//! * [metadata] — what wallpapers exist (titles, kinds, thumbnails).
//! * [runtime] — how a wallpaper is rendered and how it is controlled.
//! * [source] — where a wallpaper comes from (an archive or a folder).
//!
//! The façade never panics: every failure is reported as an [EngineError]
//! that carries its cause, and the host (the CLI or the GUI) decides what
//! to do with it.

pub mod metadata;
pub mod runtime;
pub mod source;

use std::path::PathBuf;

use crate::scene::adapters::FitMode;

pub use crate::scene::error::EngineError;
pub use source::WallpaperSource;

/// How a wallpaper is rendered.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// How the wallpaper is fitted to the output.
    pub fit_mode: FitMode,
    /// The rate the wallpaper is rendered at; [None] is "as fast as
    /// possible" and [Some](0) the static image mode.
    pub target_fps: Option<u32>,
    /// Render the wallpaper without its post-process effects.
    pub no_effects: bool,
    /// Render the MDL puppets as flat quads.
    pub no_mdl: bool,
    /// The Wallpaper Engine assets directory, used as a lazy-loading fallback.
    pub assets_path: Option<PathBuf>,
    /// Whether the loading progress is shown.
    pub show_progress: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            fit_mode: FitMode::Cover,
            target_fps: None,
            no_effects: false,
            no_mdl: false,
            assets_path: None,
            show_progress: false,
        }
    }
}
