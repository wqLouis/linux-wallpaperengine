mod scene;

use std::path::Path;

use clap::Parser;

use crate::scene::adapters::winit_adapter;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    // wallpaper .pkg file path
    #[arg(default_value = "./scene.pkg")]
    path: String,
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

    winit_adapter::start(args.path);
}
