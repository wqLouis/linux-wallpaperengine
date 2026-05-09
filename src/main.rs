mod scene;

use std::path::Path;

use clap::Parser;

use crate::scene::adapters::FitMode;
use crate::scene::adapters::{winit_adapter, wlr_app};

/// Command-line arguments for the wallpaper engine.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to a `.pkg` wallpaper file.
    #[arg(short, default_value = "./scene.pkg")]
    path: String,

    /// Display backend: `"wlr"` (Wayland layer-shell) or `"winit"` (standalone window).
    #[arg(short, default_value = "wlr")]
    modes: String,

    /// How to fit the wallpaper to the output: `"cover"`, `"contain"`, or `"stretch"`.
    #[arg(long, default_value = "cover")]
    fit_mode: String,

    /// Skip all post-process effects and render as a static image.
    #[arg(long, default_value_t = false)]
    no_effects: bool,
}

/// Maximum number of textures that can be bound per shader stage.
pub const MAX_TEXTURE: u32 = 512;
/// Maximum vertex count in the global vertex buffer (= `MAX_TEXTURE * 4`).
pub const MAX_VERTEX: u32 = MAX_TEXTURE * 4;
/// Maximum index count in the global index buffer (= `MAX_TEXTURE * 6`).
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

    // Retry loop: if wallpaper engine crashes (panic, GPU error, Wayland error),
    // wait a moment and restart automatically.
    loop {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match args.modes.as_str() {
                "winit" => winit_adapter::start(args.path.clone(), args.no_effects),
                "wlr" => wlr_app::start(args.path.clone(), fit_mode, args.no_effects),
                _ => {}
            }
        }));
        match result {
            Ok(_) => eprintln!("[main] wallpaper engine exited normally, restarting..."),
            Err(e) => eprintln!("[main] wallpaper engine panicked: {:?}, restarting...", e),
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}
