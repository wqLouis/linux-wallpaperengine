mod scene;

use clap::Parser;
use linux_wallpaper_engine::start;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    // wallpaper .pkg file path
    #[arg(default_value = "./scene.pkg")]
    path: String,
}

fn main() {
    let args = Args::parse();

    start(args.path);
}
