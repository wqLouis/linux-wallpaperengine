//! The previews of the wallpapers.
//!
//! The engine hands the preview of a wallpaper over as bytes: an encoded
//! image or the raw pixels of a video frame.  gpui shows an image from a
//! file, so the bytes are written into the cache folder of the
//! application and the file is handed to gpui.
//!
//! The previews of a workshop folder are read over and over again — every
//! time the picker is opened — so they are worth keeping on the disk.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use linux_wallpaper_engine::engine::Thumbnail;

use crate::config;

/// Writes the preview of a wallpaper into the cache.
///
/// Returns the path of the file the preview was written to, or [`None`]
/// when the preview cannot be shown: an unknown format or a preview that
/// cannot be written is not a reason to stop the application.
pub fn write_thumbnail(id: &str, source: &Path, thumbnail: &Thumbnail) -> Option<PathBuf> {
    let (bytes, extension) = encode(thumbnail)?;

    let folder = config::cache_dir().join("thumbs");
    if let Err(error) = std::fs::create_dir_all(&folder) {
        log::warn!(
            "the preview folder '{}' could not be created: {error}",
            folder.display()
        );
        return None;
    }

    // The name of the file holds the id of the wallpaper and a hash of its
    // path: two wallpapers with the same name (in two different workshop
    // folders) do not overwrite each other, and a preview that changed is
    // not shown from the cache.
    let name = format!("{}-{}.{extension}", sanitize(id), hash(source));
    let path = folder.join(name);

    if let Err(error) = std::fs::write(&path, &bytes) {
        log::warn!(
            "the preview '{}' could not be written: {error}",
            path.display()
        );
        return None;
    }

    Some(path)
}

/// The bytes of a preview and the extension of the file they are written
/// to.
fn encode(thumbnail: &Thumbnail) -> Option<(Vec<u8>, &'static str)> {
    match thumbnail {
        Thumbnail::Image { bytes, format } => {
            let extension = extension_of(format)?;
            Some((bytes.clone(), extension))
        }
        Thumbnail::Rgba {
            width,
            height,
            bytes,
        } => {
            let picture = image::RgbaImage::from_raw(*width, *height, bytes.clone())?;
            let mut encoded = Vec::new();
            image::DynamicImage::ImageRgba8(picture)
                .write_to(&mut Cursor::new(&mut encoded), image::ImageFormat::Png)
                .ok()?;
            Some((encoded, "png"))
        }
    }
}

/// The extension of the file a preview is written to.
///
/// Only the formats gpui can show are supported: a preview in another
/// format is not shown at all, which is better than a broken picture.
fn extension_of(format: &str) -> Option<&'static str> {
    match format.to_ascii_lowercase().as_str() {
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpg"),
        "gif" => Some("gif"),
        "webp" => Some("webp"),
        "bmp" => Some("bmp"),
        _ => None,
    }
}

/// A file name that is safe to write.
///
/// The id of a wallpaper is the name of its folder: it may hold anything,
/// and a file name may not hold a path.
fn sanitize(id: &str) -> String {
    let name: String = id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect();

    let name = name.trim_matches('.').to_string();
    if name.is_empty() {
        "wallpaper".to_string()
    } else {
        name
    }
}

/// A short hash of a path.
///
/// The hash of the FNV-1a is enough here: it is not a security boundary,
/// it only keeps the previews of two wallpapers apart.
fn hash(path: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file name never holds a path.
    #[test]
    fn a_file_name_never_holds_a_path() {
        assert_eq!(sanitize("12345"), "12345");
        assert_eq!(sanitize(".."), "wallpaper");
        assert_eq!(sanitize(""), "wallpaper");

        // The separators of a path are gone: the name cannot leave the
        // folder of the cache.
        let name = sanitize("../../etc/passwd");
        assert!(!name.contains('/'), "{name}");
        assert!(!name.contains('\\'), "{name}");
    }

    /// The same path always gives the same hash, and two paths do not.
    #[test]
    fn the_hash_of_a_path_is_stable() {
        let first = hash(Path::new("/a/b"));
        let second = hash(Path::new("/a/b"));
        let other = hash(Path::new("/a/c"));

        assert_eq!(first, second);
        assert_ne!(first, other);
    }

    /// Only the formats gpui knows are written to the cache.
    #[test]
    fn only_the_known_formats_are_written() {
        assert_eq!(extension_of("png"), Some("png"));
        assert_eq!(extension_of("JPEG"), Some("jpg"));
        assert_eq!(extension_of("gif"), Some("gif"));
        assert_eq!(extension_of("psd"), None);
    }

    /// The raw pixels of a video frame become a PNG.
    #[test]
    fn the_raw_pixels_become_a_png() {
        let thumbnail = Thumbnail::Rgba {
            width: 2,
            height: 2,
            bytes: vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        };

        let (bytes, extension) = encode(&thumbnail).expect("the preview must be encoded");

        assert_eq!(extension, "png");
        assert_eq!(
            &bytes[..8],
            &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
        );
    }
}
