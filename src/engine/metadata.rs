//! What wallpapers exist.
//!
//! The engine can load a wallpaper, but a host application wants to know
//! *which* wallpapers are installed before it loads one of them: their
//! title, their kind and their preview image.  This module answers that
//! question without loading anything: it scans a folder of wallpapers and
//! describes the ones it finds.
//!
//! The folder it scans may be
//!
//! * the Steam workshop folder (every subfolder is a wallpaper),
//! * the Steam root (the `steamapps/workshop/content/431960` below it is
//!   scanned instead), or
//! * a folder that holds a single wallpaper (`project.json` next to the
//!   files).
//!
//! The metadata comes from the loader's own parsers, so the titles and the
//! kinds are the ones the loader will use.  A wallpaper that cannot be
//! described (a missing or broken `project.json`) is skipped: a broken
//! wallpaper is never a crash.

use std::path::{Path, PathBuf};

use pkg_parser::pkg_parser::{tex_parser::Tex, video_parser::VideoPlayer};

use crate::collections::Map;
use crate::engine::source::WallpaperSource;
use crate::scene::loader::project::{Project, WallpaperKind, parse_project};

/// The Steam application id of the Wallpaper Engine workshop.
const WORKSHOP_APP_ID: &str = "431960";

/// What is known about one wallpaper without loading it.
#[derive(Clone, Debug)]
pub struct WallpaperInfo {
    /// A stable identifier: the folder name or the file name.
    pub id: String,
    /// The title of the wallpaper (project.json "title").
    pub title: String,
    /// The kind of the wallpaper (scene, site, video).
    pub kind: WallpaperKind,
    /// Where the wallpaper lives.
    pub source: WallpaperSource,
    /// The preview image, when there is one.
    pub thumbnail: Option<Thumbnail>,
}

/// The preview image of a wallpaper.
#[derive(Clone, Debug)]
pub enum Thumbnail {
    /// An encoded image (PNG, JPEG, GIF) with its format.
    Image {
        /// The bytes of the image.
        bytes: Vec<u8>,
        /// The format of the image ("png", "jpeg", "gif", …).
        format: String,
    },
    /// Raw RGBA pixels (a frame of a video preview).
    Rgba {
        /// The width of the picture in pixels.
        width: u32,
        /// The height of the picture in pixels.
        height: u32,
        /// The pixels, tightly packed RGBA8.
        bytes: Vec<u8>,
    },
}

/// Scans a directory for wallpapers.
///
/// The directory may be
/// * the Steam workshop folder (every subfolder is a wallpaper), or
/// * the Steam root (the "steamapps/workshop/content/431960" below it is
///   scanned instead), or
/// * a folder that holds a single wallpaper (project.json next to the files).
///
/// The result is sorted by title.
///
/// A wallpaper whose `project.json` is missing or broken is skipped: the
/// scan never fails and never panics.
pub fn scan_wallpapers(root: &Path) -> Vec<WallpaperInfo> {
    let root = workshop_folder(root).unwrap_or_else(|| root.to_path_buf());
    let mut wallpapers = Vec::new();

    // A folder that holds a single wallpaper: its manifest is next to the
    // files of the wallpaper.
    if root.join("project.json").is_file() {
        if let Some(info) = describe(&WallpaperSource::Folder(root)) {
            wallpapers.push(info);
        }
        return wallpapers;
    }

    // Every other folder holds wallpapers in its subfolders.
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) => {
            log::warn!(
                "cannot read the wallpaper folder '{}': {}",
                root.display(),
                error
            );
            return wallpapers;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(info) = describe(&WallpaperSource::Folder(path)) {
            wallpapers.push(info);
        }
    }

    // The list is sorted by title; the identifier breaks the tie, so the
    // order does not depend on the order of the file system.
    wallpapers.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.id.cmp(&right.id))
    });
    wallpapers
}

/// Describes a single wallpaper.
///
/// Returns [`None`] when the wallpaper has no `project.json`, when the
/// manifest is broken, or when the wallpaper cannot be read at all.
pub fn describe(source: &WallpaperSource) -> Option<WallpaperInfo> {
    let mut files = SourceFiles::new(source);
    let project = read_project(&mut files)?;
    let thumbnail = thumbnail_of_files(&mut files, &project);

    Some(WallpaperInfo {
        id: identifier(source),
        title: project.title.clone(),
        kind: project.kind.clone(),
        source: source.clone(),
        thumbnail,
    })
}

/// The preview image of a wallpaper.
///
/// The preview is named by the `project.json` of the wallpaper and its path
/// is relative to the wallpaper, so it is looked up in the files of the
/// source.
///
/// A preview that does not exist, or that cannot be decoded, is no
/// preview: the function returns [`None`] instead of failing.
pub fn resolve_thumbnail(source: &WallpaperSource, project: &Project) -> Option<Thumbnail> {
    let mut files = SourceFiles::new(source);
    thumbnail_of_files(&mut files, project)
}

/// The preview image of a wallpaper, with the files of the source at hand.
fn thumbnail_of_files(files: &mut SourceFiles, project: &Project) -> Option<Thumbnail> {
    let preview = project.preview.as_deref()?;
    let bytes = files.get(preview)?;
    thumbnail_of(preview, &bytes)
}

/// The workshop folder of a Steam root.
///
/// The workshop lives in `steamapps/workshop/content/431960` below the
/// Steam root; every other folder is scanned as it is.
fn workshop_folder(root: &Path) -> Option<PathBuf> {
    let workshop = root
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(WORKSHOP_APP_ID);

    workshop.is_dir().then_some(workshop)
}

/// The identifier of a wallpaper: the name of its folder or of its file.
fn identifier(source: &WallpaperSource) -> String {
    source
        .path()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| source.path().to_string_lossy().into_owned())
}

/// The manifest of a wallpaper.
fn read_project(files: &mut SourceFiles) -> Option<Project> {
    let bytes = files.get("project.json")?;
    let text = String::from_utf8_lossy(&bytes);
    parse_project(&text)
}

/// The files of a wallpaper source.
///
/// The files of a folder are read one by one, straight from the disk: a
/// scan of a workshop folder must not read every file of every wallpaper
/// just to show its title.  The files of an archive are read once and kept,
/// because the archive is read as a whole anyway.
struct SourceFiles<'a> {
    /// Where the files come from.
    source: &'a WallpaperSource,
    /// The files of the source, once they have been read.
    map: Option<Map<String, Vec<u8>>>,
}

impl<'a> SourceFiles<'a> {
    /// The files of a source that have not been read yet.
    fn new(source: &'a WallpaperSource) -> Self {
        Self { source, map: None }
    }

    /// The bytes of a file of the wallpaper.
    ///
    /// The key is the path of the file inside the wallpaper ("preview.jpg").
    fn get(&mut self, key: &str) -> Option<Vec<u8>> {
        // The manifests of Wallpaper Engine use the separator of the
        // platform they were written on.
        let key = key.replace('\\', "/");

        if let WallpaperSource::Folder(root) = self.source
            && let Ok(bytes) = std::fs::read(root.join(&key))
        {
            return Some(bytes);
        }

        if self.map.is_none() {
            self.map = self.source.read_files().ok();
        }
        lookup(self.map.as_ref()?, &key)
    }
}

/// Find a file in a file map.
///
/// The file is looked up by its path; when it is not there, the file whose
/// name matches is used - but only when there is exactly one of them, so
/// that an ambiguous name never picks the wrong picture.
fn lookup(files: &Map<String, Vec<u8>>, key: &str) -> Option<Vec<u8>> {
    if let Some(bytes) = files.get(key) {
        return Some(bytes.clone());
    }

    let name = key.rsplit('/').next()?;
    let mut found: Option<&Vec<u8>> = None;

    for (path, bytes) in files.iter() {
        let file_name = path.as_str().rsplit('/').next().unwrap_or(path.as_str());
        if !file_name.eq_ignore_ascii_case(name) {
            continue;
        }
        if found.is_some() {
            // The name is ambiguous: there is no preview.
            return None;
        }
        found = Some(bytes);
    }

    found.cloned()
}

/// The preview image of a file of a wallpaper.
fn thumbnail_of(name: &str, bytes: &[u8]) -> Option<Thumbnail> {
    match extension_of(name).as_str() {
        "tex" => {
            // A `.tex` file holds the picture of the preview.
            let tex = Tex::new(bytes)?;
            let (bytes, format) = tex.parse_to_image()?;
            // A `.tex` can hold a video container as well.
            if is_video_format(&format) {
                return video_thumbnail(&bytes);
            }
            Some(Thumbnail::Image { bytes, format })
        }
        "mp4" | "webm" | "mkv" | "avi" => video_thumbnail(bytes),
        "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" => Some(Thumbnail::Image {
            bytes: bytes.to_vec(),
            format: extension_of(name),
        }),
        _ => None,
    }
}

/// The extension of a file name, in lower case and without the dot.
fn extension_of(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Whether a format is a video container.
fn is_video_format(format: &str) -> bool {
    matches!(format, "mp4" | "webm" | "mkv" | "avi")
}

/// The first frame of a video preview.
///
/// The video player decodes the frame (on a worker thread) and hands out
/// its pixels; the other frames of the clip are decoded as well, so a
/// preview of a video is the most expensive of the previews - a wallpaper
/// that is not shown needs none at all.
fn video_thumbnail(bytes: &[u8]) -> Option<Thumbnail> {
    let mut player = VideoPlayer::open(bytes).ok()?;
    let frame = player.frame_at(0.0).ok()??;
    let pixels = frame.rgba().ok()?;

    Some(Thumbnail::Rgba {
        width: frame.width,
        height: frame.height,
        bytes: pixels.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A tiny PNG (the header is enough: the loader hands the bytes of an
    /// encoded image over as they are).
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52,
    ];

    /// A temporary folder that is removed when the test is over.
    struct TempTree {
        path: PathBuf,
    }

    impl TempTree {
        /// Create a temporary folder with a unique name.
        ///
        /// The tests run in parallel and every one of them writes its own
        /// folder, so the name has to be unique.
        fn new(name: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "linux-wallpaperengine-metadata-{}-{}-{}",
                name,
                std::process::id(),
                unique
            ));

            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("the temporary folder must be created");

            Self { path }
        }

        /// The path of the folder.
        fn path(&self) -> &Path {
            &self.path
        }

        /// Write a file of the folder, creating the folders it needs.
        fn write(&self, relative: &str, contents: &[u8]) {
            let path = self.path.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("the folder must be created");
            }
            std::fs::write(&path, contents).expect("the file must be written");
        }

        /// The path of a folder of the tree (it is created when it is not
        /// there yet).
        fn folder(&self, relative: &str) -> PathBuf {
            let path = self.path.join(relative);
            std::fs::create_dir_all(&path).expect("the folder must be created");
            path
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// A workshop folder holds every wallpaper in a subfolder.
    #[test]
    fn a_workshop_folder_holds_every_wallpaper() {
        let tree = TempTree::new("workshop");
        tree.write("111/project.json", br#"{"title":"Zebra","type":"Scene"}"#);
        tree.write(
            "222/project.json",
            br#"{"title":"Alpha","type":"Video","file":"video.mp4"}"#,
        );

        let wallpapers = scan_wallpapers(tree.path());

        assert_eq!(wallpapers.len(), 2);

        // The list is sorted by title…
        assert_eq!(wallpapers[0].title, "Alpha");
        assert_eq!(wallpapers[1].title, "Zebra");

        // …and the kind comes from the manifest.
        assert_eq!(wallpapers[0].kind, WallpaperKind::Video);
        assert_eq!(wallpapers[1].kind, WallpaperKind::Scene);

        // The identifier is the name of the folder.
        assert_eq!(wallpapers[0].id, "222");
        assert_eq!(wallpapers[1].id, "111");

        // The wallpaper knows where it lives.
        assert_eq!(
            wallpapers[0].source,
            WallpaperSource::Folder(tree.path().join("222"))
        );
    }

    /// The Steam root holds the workshop below it.
    #[test]
    fn a_steam_root_holds_the_workshop_below_it() {
        let tree = TempTree::new("steam");
        tree.write(
            "steamapps/workshop/content/431960/12345/project.json",
            br#"{"title":"From Steam"}"#,
        );

        let wallpapers = scan_wallpapers(tree.path());

        assert_eq!(wallpapers.len(), 1);
        assert_eq!(wallpapers[0].title, "From Steam");
        assert_eq!(wallpapers[0].id, "12345");
    }

    /// A folder without wallpapers holds none.
    #[test]
    fn an_empty_folder_holds_no_wallpaper() {
        let tree = TempTree::new("empty");

        assert!(scan_wallpapers(tree.path()).is_empty());
    }

    /// A folder that does not exist holds no wallpaper either.
    #[test]
    fn a_missing_folder_holds_no_wallpaper() {
        let tree = TempTree::new("missing-folder");
        let missing = tree.path().join("nowhere");

        assert!(scan_wallpapers(&missing).is_empty());
    }

    /// A wallpaper without a manifest cannot be described.
    #[test]
    fn a_wallpaper_without_a_manifest_cannot_be_described() {
        let tree = TempTree::new("no-manifest");
        tree.folder("wallpaper");

        let source = WallpaperSource::Folder(tree.path().join("wallpaper"));

        assert!(describe(&source).is_none());
    }

    /// A broken manifest cannot be described either.
    #[test]
    fn a_wallpaper_with_a_broken_manifest_cannot_be_described() {
        let tree = TempTree::new("broken-manifest");
        tree.write("wallpaper/project.json", b"{ this is not json");

        let source = WallpaperSource::Folder(tree.path().join("wallpaper"));

        assert!(describe(&source).is_none());

        // The scan skips it as well.
        let wallpapers = scan_wallpapers(tree.path());
        assert!(wallpapers.is_empty());
    }

    /// The preview of a wallpaper is the file its manifest names.
    #[test]
    fn the_preview_is_found_and_a_missing_one_is_not() {
        let tree = TempTree::new("thumbnail");
        tree.write("preview.png", PNG);

        let source = WallpaperSource::Folder(tree.path().to_path_buf());

        let project =
            parse_project(r#"{"title":"x","preview":"preview.png"}"#).expect("the manifest parses");
        let thumbnail = resolve_thumbnail(&source, &project).expect("the preview must be found");

        match thumbnail {
            Thumbnail::Image { bytes, format } => {
                assert_eq!(format, "png");
                assert_eq!(bytes, PNG);
            }
            other => panic!("unexpected thumbnail: {other:?}"),
        }

        // A preview that is not there is no preview.
        let missing =
            parse_project(r#"{"title":"x","preview":"nowhere.png"}"#).expect("the manifest parses");
        assert!(resolve_thumbnail(&source, &missing).is_none());

        // A wallpaper without a preview has none either.
        let none = parse_project(r#"{"title":"x"}"#).expect("the manifest parses");
        assert!(resolve_thumbnail(&source, &none).is_none());
    }

    /// The preview of a video wallpaper is a frame of the clip.
    #[test]
    fn the_preview_of_a_video_is_a_frame_of_the_clip() {
        // A tiny h264 clip (32x32) from the parser's test data.
        const SAMPLE_MP4: &[u8] = include_bytes!("../pkg_parser/tests/data/sample.mp4");

        let tree = TempTree::new("video-thumbnail");
        tree.write("preview.mp4", SAMPLE_MP4);

        let source = WallpaperSource::Folder(tree.path().to_path_buf());
        let project =
            parse_project(r#"{"title":"x","preview":"preview.mp4"}"#).expect("the manifest parses");

        let thumbnail = resolve_thumbnail(&source, &project).expect("the preview must be found");

        match thumbnail {
            Thumbnail::Rgba {
                width,
                height,
                bytes,
            } => {
                assert_eq!((width, height), (32, 32));
                assert_eq!(bytes.len(), (width * height * 4) as usize);
            }
            other => panic!("unexpected thumbnail: {other:?}"),
        }
    }

    /// The files of a folder are keyed by their path inside it.
    #[test]
    fn the_files_of_a_folder_are_keyed_by_their_path() {
        let tree = TempTree::new("read-files");
        tree.write("project.json", b"{}");
        tree.write("materials/wood.json", b"{}");

        let files = WallpaperSource::Folder(tree.path().to_path_buf())
            .read_files()
            .expect("the folder must be read");

        assert_eq!(files.len(), 2);
        assert_eq!(
            files.get("project.json").map(|bytes| bytes.as_slice()),
            Some(&b"{}"[..])
        );
        assert_eq!(
            files
                .get("materials/wood.json")
                .map(|bytes| bytes.as_slice()),
            Some(&b"{}"[..])
        );
    }

    /// An archive that is not there is an error, not a panic.
    #[test]
    fn a_missing_archive_is_an_error() {
        let tree = TempTree::new("missing-archive");
        let missing = tree.path().join("nothing.pkg");

        let result = WallpaperSource::Package(missing).read_files();

        assert!(result.is_err());
    }
}
