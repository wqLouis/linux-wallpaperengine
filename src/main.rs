mod scene;

use std::path::Path;

use clap::Parser;

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
}

pub const MAX_TEXTURE: u32 = 512;
pub const MAX_VERTEX: u32 = MAX_TEXTURE * 4;
pub const MAX_INDEX: u32 = MAX_TEXTURE * 6;

fn main() {
    let args = Args::parse();

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

    // Retry loop: if wallpaper engine crashes (GPU error, Wayland error, etc.),
    // wait a moment and restart automatically.
    loop {
        match args.modes.as_str() {
            "winit" => winit_adapter::start(args.path.clone(), args.no_effects),
            "wlr" => wlr_app::start(args.path.clone(), fit_mode, args.no_effects),
            _ => break,
        }
        eprintln!("[main] wallpaper engine exited, restarting in 2 seconds...");
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}
