//! Display adapters for rendering wallpapers.
//!
//! Four adapters are provided:
//!
//! * **`winit_adapter`** — Creates an always-on-bottom window using winit.
//!   Works on both X11 and Wayland (via XWayland).  Supports cursor tracking
//!   for depth-parallax effects.
//! * **`wlr_app`** — Uses the wlr-layer-shell Wayland protocol to render on
//!   a `Layer::Background` surface behind all windows.  No cursor tracking
//!   (Wayland's security model does not allow it for background surfaces).
//! * **`gnome_app`** — Renders off-screen and streams the frames to a GNOME
//!   Shell extension, which displays them as the desktop background.  GNOME
//!   Shell does not implement wlr-layer-shell, so a companion extension is
//!   required.
//! * **`mac_app`** — (macOS, experimental) Creates an AppKit window at the
//!   desktop window level and renders into its `CAMetalLayer`.
//!
//! Every adapter takes an [AdapterConfig] and returns a
//! [Result](crate::scene::error::EngineError): an adapter that cannot show
//! the wallpaper says why instead of dying on the spot.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use crate::engine::runtime::EngineStatus;
use crate::engine::source::WallpaperSource;

/// How the wallpaper is fitted to the output.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FitMode {
    /// Scale wallpaper to fill entire output, cropping if aspect ratios differ.
    Cover,
    /// Scale wallpaper to fit within output, letterboxing if aspect ratios differ.
    Contain,
    /// Stretch wallpaper to exactly match output (ignores aspect ratio).
    Stretch,
}

/// The NDC scale that implements a fit mode.
///
/// The scene is projected to the full NDC range, which stretches the wallpaper
/// to the aspect ratio of the output.  Scaling the NDC coordinates fixes that:
/// `cover` overflows the viewport (the wallpaper is cropped), `contain`
/// leaves empty borders around it and `stretch` keeps the default behaviour.
///
/// The scale keeps the aspect ratio of the scene: `sx / sy` is always the
/// ratio between the scene's and the target's aspect ratio.
pub fn fit_scale(fit_mode: FitMode, scene: [u32; 2], target: [u32; 2]) -> (f32, f32) {
    let (sw, sh) = (scene[0] as f32, scene[1] as f32);
    let (tw, th) = (target[0] as f32, target[1] as f32);
    if sw <= 0.0 || sh <= 0.0 || tw <= 0.0 || th <= 0.0 {
        return (1.0, 1.0);
    }

    // The ratio between the scene's and the target's aspect ratio.
    let ratio = (sw * th) / (sh * tw);

    match fit_mode {
        FitMode::Stretch => (1.0, 1.0),
        // The scene has to fill the viewport: it overflows in one direction.
        FitMode::Cover => (ratio.max(1.0), (1.0 / ratio).max(1.0)),
        // The scene has to fit into the viewport: it leaves borders in one
        // direction.
        FitMode::Contain => (ratio.min(1.0), (1.0 / ratio).min(1.0)),
    }
}

pub mod failures;
pub mod gnome_app;
#[cfg(target_os = "macos")]
pub mod mac_app;
pub mod winit_adapter;
pub mod wlr_app;

pub use failures::RenderFailures;

/// What an adapter needs to render a wallpaper.
#[derive(Clone, Debug)]
pub struct AdapterConfig {
    /// The wallpaper to render.
    ///
    /// An archive and a folder are the same thing here: the loader reads
    /// the files of the source either way.
    pub source: WallpaperSource,
    /// How the wallpaper is fitted to the output.
    pub fit_mode: FitMode,
    /// Render the wallpaper without its post-process effects.
    pub no_effects: bool,
    /// Render the MDL puppets as flat quads.
    pub no_mdl: bool,
    /// The Wallpaper Engine assets directory (a lazy-loading fallback).
    pub assets_path: Option<String>,
    /// The rate the wallpaper is rendered at.
    ///
    /// [None] is "as fast as possible" and [Some](0) the static image
    /// mode.
    pub target_fps: Option<u32>,
    /// Whether the loading progress is shown.
    pub show_progress: bool,
    /// Where the adapter reports what it is doing.
    ///
    /// The adapter sends the problems it runs into — the files of the
    /// wallpaper that could not be used and the frames that could not be
    /// drawn — to the host, which shows them to the user.  Without a
    /// channel the problems are only written to the log.
    pub status: Option<Sender<EngineStatus>>,
}

/// The source of the wallpaper at `path`.
///
/// The shape of the source is decided by the path itself: a folder is a
/// folder and everything else is an archive, so a folder works exactly
/// like an archive.
pub fn wallpaper_source(path: impl Into<PathBuf>) -> WallpaperSource {
    WallpaperSource::auto(path.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The aspect ratio the scene has on screen after the NDC scale.
    fn screen_aspect(target: [u32; 2], scale: (f32, f32)) -> f32 {
        let (tw, th) = (target[0] as f32, target[1] as f32);
        (tw * scale.0) / (th * scale.1)
    }

    #[test]
    fn matching_aspect_ratios_are_not_scaled() {
        for mode in [FitMode::Cover, FitMode::Contain, FitMode::Stretch] {
            let scale = fit_scale(mode, [1920, 1080], [1920, 1080]);
            assert_eq!(scale, (1.0, 1.0), "{mode:?}");
        }
    }

    #[test]
    fn stretch_fills_the_viewport() {
        let scale = fit_scale(FitMode::Stretch, [1920, 1080], [1024, 1024]);
        assert_eq!(scale, (1.0, 1.0));
    }

    #[test]
    fn cover_keeps_the_aspect_ratio_and_fills_the_viewport() {
        let scene = [1920, 1080];
        let target = [1024, 1024];
        let scale = fit_scale(FitMode::Cover, scene, target);

        // The wallpaper keeps its 16:9 aspect ratio ...
        let aspect = screen_aspect(target, scale);
        assert!((aspect - 16.0 / 9.0).abs() < 1e-4, "{aspect}");

        // ... and it is big enough to cover the viewport.
        assert!(scale.0 >= 1.0 && scale.1 >= 1.0, "{scale:?}");
    }

    #[test]
    fn contain_keeps_the_aspect_ratio_and_fits_into_the_viewport() {
        let scene = [1920, 1080];
        let target = [1024, 1024];
        let scale = fit_scale(FitMode::Contain, scene, target);

        // The wallpaper keeps its 16:9 aspect ratio ...
        let aspect = screen_aspect(target, scale);
        assert!((aspect - 16.0 / 9.0).abs() < 1e-4, "{aspect}");

        // ... and it fits into the viewport (letterboxed).
        assert!(scale.0 <= 1.0 && scale.1 <= 1.0, "{scale:?}");
    }

    #[test]
    fn a_tall_scene_is_covered_by_scaling_the_height() {
        // A portrait scene on a landscape screen: the width matches and the
        // scene overflows vertically (the top and the bottom are cropped).
        let scene = [1080, 1920];
        let target = [1920, 1080];
        let scale = fit_scale(FitMode::Cover, scene, target);

        assert_eq!(scale.0, 1.0, "{scale:?}");
        assert!(scale.1 > 1.0, "{scale:?}");
    }

    /// The shape of the source is decided by the path itself.
    ///
    /// A folder works exactly like an archive: the loader reads the files
    /// of the source either way.
    #[test]
    fn the_shape_of_the_source_is_decided_by_the_path() {
        let folder = wallpaper_source(std::env::temp_dir());
        assert!(matches!(folder, WallpaperSource::Folder(_)), "{folder:?}");

        let archive = wallpaper_source(std::env::temp_dir().join("scene.pkg"));
        assert!(
            matches!(archive, WallpaperSource::Package(_)),
            "{archive:?}"
        );
    }
}
