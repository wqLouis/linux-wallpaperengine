//! Lists the wallpapers of a folder.
//!
//! The folder may be the workshop of Steam (every subfolder is a
//! wallpaper), the root of Steam (the workshop below it is scanned
//! instead) or a single wallpaper folder.
//!
//!     cargo run --example scan -- ~/steamapps/workshop/content/431960

use std::path::PathBuf;

use linux_wallpaper_engine::engine::scan_wallpapers;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let wallpapers = scan_wallpapers(&root);
    if wallpapers.is_empty() {
        eprintln!("no wallpaper found in '{}'", root.display());
        return Ok(());
    }

    for wallpaper in wallpapers {
        println!(
            "{:<24} {:<8} {}",
            wallpaper.id,
            format!("{:?}", wallpaper.kind),
            wallpaper.title
        );
    }

    Ok(())
}
