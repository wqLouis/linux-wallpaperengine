mod scene;

use std::path::Path;

use clap::Parser;

use crate::scene::adapters::{winit_adapter, wlr_layer_shell_adapter};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    // wallpaper .pkg file path
    #[arg(short, default_value = "./scene.pkg")]
    path: String,

    // different display mode [wlr, winit]
    #[arg(short, default_value = "wlr")]
    modes: String,

    #[arg(short, default_value = None)]
    dimensions: Option<String>,
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

    let resolution: Option<[u32; 2]>;
    if let Some(resolution_str) = args.dimensions {
        let resolution_slice = resolution_str
            .split("x")
            .map(|dimension| dimension.parse::<u32>().unwrap())
            .collect::<Vec<u32>>();

        match resolution_slice[..] {
            [x, y] => resolution = Some([x, y]),
            _ => {
                panic!()
            }
        }
    } else {
        resolution = None;
    }

    match args.modes.as_str() {
        "winit" => {
            winit_adapter::start(args.path);
        }
        "wlr" => {
            wlr_layer_shell_adapter::start(args.path, resolution);
        }
        _ => {}
    }
}
