//! The previews of the wallpapers.
//!
//! The engine hands the preview of a wallpaper over as bytes: an encoded
//! image or the raw pixels of a video frame.  gpui shows an image from a
//! file, so the bytes are written into the cache folder of the
//! application and the file is handed to gpui.
//!
//! The previews of a workshop folder are read over and over again — every
//! time the picker is opened — so they are worth keeping on the disk.
//!
//! Every preview is written as a **PNG**: the previews of the wallpapers of
//! the workshop are `preview.gif` and `preview.jpg`, and gpui cannot
//! decode a jpeg at all.  The preview is decoded here and written again,
//! so the cache holds PNGs only and gpui always shows them.

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
    let base = format!("{}-{}", sanitize(id), hash(source));
    let path = folder.join(format!("{base}.{extension}"));

    if let Err(error) = std::fs::write(&path, &bytes) {
        log::warn!(
            "the preview '{}' could not be written: {error}",
            path.display()
        );
        return None;
    }

    // The cache holds PNGs only: the previews that older versions wrote
    // are of no use any more.
    for old in OLD_EXTENSIONS {
        let _ = std::fs::remove_file(folder.join(format!("{base}.{old}")));
    }

    Some(path)
}

/// The extensions the previews had before they were written as PNGs.
const OLD_EXTENSIONS: &[&str] = &["jpg", "jpeg", "gif", "webp", "bmp"];

/// The bytes of a preview and the extension of the file they are written
/// to.
///
/// The preview is decoded and written again as a PNG: gpui cannot show a
/// jpeg, and the previews of the wallpapers of the workshop are gifs and
/// jpegs.  A preview that cannot be decoded is no preview: the function
/// returns [`None`] instead of failing.
fn encode(thumbnail: &Thumbnail) -> Option<(Vec<u8>, &'static str)> {
    match thumbnail {
        Thumbnail::Image { bytes, .. } => {
            let picture = image::load_from_memory(bytes).ok()?;
            let mut encoded = Vec::new();
            picture
                .write_to(&mut Cursor::new(&mut encoded), image::ImageFormat::Png)
                .ok()?;
            Some((encoded, "png"))
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

    /// A one by one pixel gif: the previews of the wallpapers of the
    /// workshop are gifs.
    const GIF: &[u8] = &[
        b'G', b'I', b'F', b'8', b'9', b'a', // the header
        0x01, 0x00, 0x01, 0x00, // one pixel wide, one pixel high
        0x80, 0x00, 0x00, // the global color table holds two colors
        0xff, 0xff, 0xff, 0x00, 0x00, 0x00, // white and black
        0x21, 0xf9, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, // the graphic control
        0x2c, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, // the image
        0x02, 0x02, 0x44, 0x01, 0x00, // its data
        0x3b, // the trailer
    ];

    /// The bytes of the header of a png.
    const PNG_HEADER: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

    /// A jpeg: the previews of the wallpapers of the workshop are gifs and
    /// jpegs, and gpui cannot show a jpeg at all.
    const JPEG: &[u8] = &[
        0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46, 0x49, 0x46, 0x00, 0x01, 0x02, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x00, 0xff, 0xfe, 0x00, 0x0f, 0x4c, 0x61, 0x76, 0x63, 0x36, 0x33,
        0x2e, 0x31, 0x2e, 0x31, 0x30, 0x32, 0x00, 0xff, 0xdb, 0x00, 0x43, 0x00, 0x08, 0x0a, 0x0a,
        0x0b, 0x0a, 0x0b, 0x0d, 0x0d, 0x0d, 0x0d, 0x0d, 0x0d, 0x10, 0x0f, 0x10, 0x10, 0x10, 0x10,
        0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x12, 0x12, 0x12, 0x15, 0x15, 0x15, 0x12, 0x12,
        0x12, 0x10, 0x10, 0x12, 0x12, 0x14, 0x14, 0x15, 0x15, 0x17, 0x17, 0x17, 0x15, 0x15, 0x15,
        0x15, 0x17, 0x17, 0x19, 0x19, 0x19, 0x1e, 0x1e, 0x1c, 0x1c, 0x23, 0x23, 0x24, 0x2b, 0x2b,
        0x33, 0xff, 0xc4, 0x00, 0x4c, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x07, 0x10, 0x01,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x11, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x02, 0x00, 0x02, 0x03, 0x01,
        0x22, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xda, 0x00, 0x0c, 0x03, 0x01, 0x00,
        0x02, 0x11, 0x03, 0x11, 0x00, 0x3f, 0x00, 0x8b, 0x00, 0x4d, 0x7f, 0x7f, 0xff, 0xd9,
    ];

    /// A preview that is a gif comes out as a png: gpui cannot show a
    /// jpeg, and the cache holds PNGs only.
    #[test]
    fn a_gif_comes_out_as_a_png() {
        let thumbnail = Thumbnail::Image {
            bytes: GIF.to_vec(),
            format: "gif".into(),
        };

        let (bytes, extension) = encode(&thumbnail).expect("the preview must be encoded");

        assert_eq!(extension, "png");
        assert_eq!(&bytes[..8], PNG_HEADER);

        // The picture itself is the one of the gif.
        let picture = image::load_from_memory(&bytes).expect("the png must be readable");
        assert_eq!((picture.width(), picture.height()), (1, 1));
    }

    /// A jpeg comes out as a png too.
    #[test]
    fn a_jpeg_comes_out_as_a_png() {
        let thumbnail = Thumbnail::Image {
            bytes: JPEG.to_vec(),
            format: "jpg".into(),
        };

        let (bytes, extension) = encode(&thumbnail).expect("the preview must be encoded");

        assert_eq!(extension, "png");
        assert_eq!(&bytes[..8], PNG_HEADER);

        // The picture itself is the one of the jpeg.
        let picture = image::load_from_memory(&bytes).expect("the png must be readable");
        assert_eq!((picture.width(), picture.height()), (2, 2));
    }

    /// A preview that cannot be decoded is no preview.
    #[test]
    fn a_broken_preview_is_no_preview() {
        let thumbnail = Thumbnail::Image {
            bytes: b"this is not an image".to_vec(),
            format: "gif".into(),
        };

        assert!(encode(&thumbnail).is_none());
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
        assert_eq!(&bytes[..8], PNG_HEADER);
    }
}
