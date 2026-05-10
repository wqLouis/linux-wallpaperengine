mod scene;

use std::path::Path;

use clap::Parser;
use log::LevelFilter;

use crate::scene::adapters::FitMode;
use crate::scene::adapters::{winit_adapter, wlr_app};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    // wallpaper .pkg file path
    #[arg(short, default_value = "./scene.pkg")]
    path: String,

    // different display mode [wlr, winit]
    #[arg(short, default_value = "wlr")]
    modes: String,

    // How to fit wallpaper to output: cover, contain, or stretch
    #[arg(long, default_value = "cover")]
    fit_mode: String,

    // Bypass all post-process effects, render as static image
    #[arg(long, default_value_t = false)]
    no_effects: bool,

    // Log level: verbose (everything + wgpu/naga), debug, warning, errors
    #[arg(short = 'l', long, default_value = "warning")]
    log_level: String,

    // --- Extract / parse mode (instead of running the wallpaper engine) ---

    /// Extract and parse the .pkg file instead of running the wallpaper engine.
    /// Optionally specify an output directory (default: ./extracted).
    #[arg(short = 'x', default_value = None, num_args = 0..=1, default_missing_value = "extracted")]
    extract: Option<String>,

    /// Parse .tex textures to PNG images during extraction.
    #[arg(long, default_value_t = false)]
    parse_tex: bool,

    /// Parse video/GIF metadata during extraction.
    #[arg(long, default_value_t = false)]
    parse_video: bool,

    /// Dry run — show what would be extracted without writing files.
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

pub const MAX_TEXTURE: u32 = 512;
pub const MAX_VERTEX: u32 = MAX_TEXTURE * 4;
pub const MAX_INDEX: u32 = MAX_TEXTURE * 6;

fn main() {
    let args = Args::parse();

    let (level, verbose) = match args.log_level.as_str() {
        "verbose" => (LevelFilter::Trace, true),
        "debug" => (LevelFilter::Debug, false),
        "warning" => (LevelFilter::Warn, false),
        "errors" => (LevelFilter::Error, false),
        _ => {
            eprintln!("Unknown log-level '{}'. Valid: verbose, debug, warning, errors", args.log_level);
            return;
        }
    };

    let mut builder = env_logger::Builder::new();
    builder.filter_level(level);
    // Suppress noisy wgpu/naga crate logs at non-verbose levels.
    // At "verbose" we allow everything (trace includes all crate logs).
    if !verbose {
        builder.filter(Some("wgpu"), LevelFilter::Warn);
        builder.filter(Some("naga"), LevelFilter::Warn);
        builder.filter(Some("wgpu_core"), LevelFilter::Warn);
        builder.filter(Some("wgpu_hal"), LevelFilter::Warn);
    }
    builder.format_timestamp_millis().init();

    let path = Path::new(&args.path);
    if path.exists() == false || path.extension().unwrap_or_default() != "pkg" {
        panic!("Path not exist or wrong file extension");
    }

    let fit_mode = match args.fit_mode.as_str() {
        "cover" => FitMode::Cover,
        "contain" | "fit" => FitMode::Contain,
        "stretch" => FitMode::Stretch,
        _ => {
            eprintln!("Unknown fit-mode '{}'. Valid: cover, contain, stretch", args.fit_mode);
            return;
        }
    };

    // Extract mode: parse and extract the .pkg file instead of running.
    if let Some(output) = args.extract {
        let pkg = pkg_parser::pkg_parser::parser::Pkg::new(Path::new(&args.path));
        let target = Path::new(&output);
        log::info!(
            "Extracting {} files to {} (parse_tex={}, parse_video={}, dry_run={})",
            pkg.files.len(),
            target.display(),
            args.parse_tex,
            args.parse_video,
            args.dry_run,
        );
        pkg.save_pkg(&target, args.dry_run, args.parse_tex, args.parse_video);
        return;
    }

    match args.modes.as_str() {
        "winit" => winit_adapter::start(args.path, args.no_effects),
        "wlr" => wlr_app::start(args.path, fit_mode, args.no_effects),
        _ => {}
    }
}
