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

    // Log level: all, warning, errors
    #[arg(short = 'l', long, default_value = "warning")]
    log_level: String,
}

pub const MAX_TEXTURE: u32 = 512;
pub const MAX_VERTEX: u32 = MAX_TEXTURE * 4;
pub const MAX_INDEX: u32 = MAX_TEXTURE * 6;

fn main() {
    let args = Args::parse();

    let level = match args.log_level.as_str() {
        "all" => LevelFilter::Debug,
        "warning" => LevelFilter::Warn,
        "errors" => LevelFilter::Error,
        _ => {
            eprintln!("Unknown log-level '{}'. Valid: all, warning, errors", args.log_level);
            return;
        }
    };

    env_logger::Builder::new()
        .filter_level(level)
        .format_timestamp_millis()
        .init();

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

    match args.modes.as_str() {
        "winit" => winit_adapter::start(args.path, args.no_effects),
        "wlr" => wlr_app::start(args.path, fit_mode, args.no_effects),
        _ => {}
    }
}
