//! Where a wallpaper comes from.

use std::path::{Path, PathBuf};

use crate::collections::Map;
use crate::scene::error::EngineError;

/// Where a wallpaper comes from.
///
/// Wallpaper Engine ships two shapes: a `.pkg` archive and a folder with a
/// `project.json` and the files of one wallpaper next to it (the Steam
/// workshop layout).  Both are first class here.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WallpaperSource {
    /// A `.pkg` archive.
    Package(PathBuf),
    /// A folder that holds the files of one wallpaper.
    ///
    /// The folder of a wallpaper of the Steam workshop holds the package
    /// of the wallpaper (`scene.pkg`) next to its manifest
    /// (`project.json`) and its preview: the files of the wallpaper are
    /// the files of the package and the files that lie next to it.
    Folder(PathBuf),
}

/// The name of the package of a scene.
const SCENE_PACKAGE: &str = "scene.pkg";

/// The extension of a package.
const PACKAGE_EXTENSION: &str = "pkg";

impl WallpaperSource {
    /// The path of the archive or of the folder.
    pub fn path(&self) -> &Path {
        match self {
            WallpaperSource::Package(path) | WallpaperSource::Folder(path) => path,
        }
    }

    /// Picks the shape of the source by looking at the path: a directory is
    /// a folder, everything else is an archive.
    pub fn auto(path: PathBuf) -> Self {
        if path.is_dir() {
            WallpaperSource::Folder(path)
        } else {
            WallpaperSource::Package(path)
        }
    }

    /// Reads the files of the wallpaper, keyed by their path inside it.
    ///
    /// This is what the loader consumes: the very same map the archive
    /// parser produces, built from a folder walk for the folder sources.
    pub fn read_files(&self) -> Result<Map<String, Vec<u8>>, EngineError> {
        match self {
            WallpaperSource::Package(path) => Ok(read_package(path)?),
            WallpaperSource::Folder(path) => read_folder(path),
        }
    }
}

/// An io error that says which path it happened at.
fn io_error(path: &Path, error: std::io::Error) -> std::io::Error {
    std::io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

/// The files of a package.
fn read_package(path: &Path) -> Result<Map<String, Vec<u8>>, EngineError> {
    let package = pkg_parser::pkg_parser::parser::Pkg::new(path).map_err(EngineError::Package)?;
    Ok(package.files.into_iter().collect())
}

/// Reads the files of a folder of a wallpaper.
///
/// The folder of a wallpaper of the Steam workshop holds the package of
/// the wallpaper next to its manifest and its preview: the files of the
/// wallpaper are then the files of the package and the files that lie
/// next to it.  The manifest is needed to know what kind of wallpaper it
/// is, the preview is needed to show it.
///
/// A folder without a package - a video or a web wallpaper - holds the
/// files of the wallpaper itself, and they are read as they are.
fn read_folder(root: &Path) -> Result<Map<String, Vec<u8>>, EngineError> {
    let Some(package) = find_package(root)? else {
        return read_tree(root);
    };

    let mut files = read_package(&package)?;

    // The manifest and the preview lie next to the package.
    for (name, bytes) in read_next_to(root)? {
        // The package is the wallpaper: a file of the package is never
        // overwritten by a file that lies next to it.
        files.entry(name).or_insert(bytes);
    }

    Ok(files)
}

/// The package of a folder, when the folder holds one.
///
/// The package of a scene is `scene.pkg`; a wallpaper that is not a scene
/// names its package differently (`gifscene.pkg`).  When there is more
/// than one package, the one whose name comes first alphabetically is
/// used, so the same folder always gives the same package.
fn find_package(root: &Path) -> Result<Option<PathBuf>, EngineError> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        // A folder that cannot be read holds no package: the failure is
        // reported when the files of the folder are read.
        Err(_) => return Ok(None),
    };

    let mut packages: Vec<PathBuf> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => return Err(EngineError::Io(io_error(&path, error))),
        };

        if !file_type.is_file() || !is_package(&path) {
            continue;
        }

        // The package of a scene has a name of its own: it wins.
        if path
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(SCENE_PACKAGE))
        {
            return Ok(Some(path));
        }

        packages.push(path);
    }

    packages.sort();
    Ok(packages.into_iter().next())
}

/// Whether a file is a package.
fn is_package(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(PACKAGE_EXTENSION))
}

/// The files that lie next to the package: the manifest and the preview.
///
/// The files of the subfolders are not read: the folder of a wallpaper of
/// the workshop holds the cache of the shaders of the Windows version of
/// the wallpaper, and the files of a wallpaper that has no package are
/// read by [`read_tree`].
fn read_next_to(root: &Path) -> Result<Vec<(String, Vec<u8>)>, EngineError> {
    let entries =
        std::fs::read_dir(root).map_err(|error| EngineError::Io(io_error(root, error)))?;

    let mut files = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| EngineError::Io(io_error(root, error)))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| EngineError::Io(io_error(&path, error)))?;

        if !file_type.is_file() {
            continue;
        }

        // The package itself is not read a second time: it is 25 MB and
        // there are forty of them.
        if is_package(&path) {
            continue;
        }

        let Some(name) = path.file_name() else {
            continue;
        };

        let bytes =
            std::fs::read(&path).map_err(|error| EngineError::Io(io_error(&path, error)))?;
        files.push((name.to_string_lossy().into_owned(), bytes));
    }

    Ok(files)
}

/// Reads a folder of wallpaper files, keyed by their path inside it.
///
/// The paths are relative to the folder and use `/` as the separator, so
/// they look exactly like the paths of an archive.
fn read_tree(root: &Path) -> Result<Map<String, Vec<u8>>, EngineError> {
    let mut files = crate::collections::new_map();
    let mut stack = vec![root.to_path_buf()];

    while let Some(directory) = stack.pop() {
        let entries = std::fs::read_dir(&directory)
            .map_err(|error| EngineError::Io(io_error(&directory, error)))?;

        for entry in entries {
            let entry = entry.map_err(|error| EngineError::Io(io_error(&directory, error)))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| EngineError::Io(io_error(&path, error)))?;

            if file_type.is_dir() {
                stack.push(path);
                continue;
            }

            if !file_type.is_file() {
                // A symlink or a socket: not a wallpaper file.
                continue;
            }

            let relative = path
                .strip_prefix(root)
                .map_err(|error| EngineError::Io(std::io::Error::other(error)))?
                .to_string_lossy()
                .replace('\\', "/");

            let bytes =
                std::fs::read(&path).map_err(|error| EngineError::Io(io_error(&path, error)))?;
            files.insert(relative, bytes);
        }
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
                "linux-wallpaperengine-source-{}-{}-{}",
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
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// Writes a package that holds the given files.
    ///
    /// The format of a package is simple: the version string, the number
    /// of the files, the table of the files (their path, their place in
    /// the data and their size) and then the data itself.
    fn write_package(path: &Path, files: &[(&str, &[u8])]) {
        const VERSION: &[u8] = b"PKGV0023";

        let mut archive = Vec::new();
        archive.extend_from_slice(&(VERSION.len() as u32).to_le_bytes());
        archive.extend_from_slice(VERSION);
        archive.extend_from_slice(&(files.len() as u32).to_le_bytes());

        let mut table = Vec::new();
        let mut data = Vec::new();

        for (name, bytes) in files {
            table.extend_from_slice(&(name.len() as u32).to_le_bytes());
            table.extend_from_slice(name.as_bytes());
            table.extend_from_slice(&(data.len() as u32).to_le_bytes());
            table.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            data.extend_from_slice(bytes);
        }

        archive.extend_from_slice(&table);
        archive.extend_from_slice(&data);

        std::fs::write(path, archive).expect("the package must be written");
    }

    /// The folder of a wallpaper of the workshop holds the package of the
    /// wallpaper: its files come out of the package, and the manifest that
    /// lies next to it comes with them.
    #[test]
    fn a_folder_with_a_package_holds_the_files_of_the_package_and_the_manifest() {
        let tree = TempTree::new("package");
        tree.write("project.json", br#"{"title":"Aurora","type":"Scene"}"#);
        tree.write("preview.gif", b"GIF89a");
        write_package(
            &tree.path().join("scene.pkg"),
            &[("scene.json", b"{}"), ("materials/wood.json", b"{}")],
        );

        let files = WallpaperSource::Folder(tree.path().to_path_buf())
            .read_files()
            .expect("the folder must be read");

        // The scene and its assets come out of the package…
        assert_eq!(
            files.get("scene.json").map(|bytes| bytes.as_slice()),
            Some(&b"{}"[..])
        );
        assert_eq!(
            files
                .get("materials/wood.json")
                .map(|bytes| bytes.as_slice()),
            Some(&b"{}"[..])
        );

        // …and the manifest and the preview from next to it.
        assert_eq!(
            files.get("project.json").map(|bytes| bytes.as_slice()),
            Some(&br#"{"title":"Aurora","type":"Scene"}"#[..])
        );
        assert_eq!(
            files.get("preview.gif").map(|bytes| bytes.as_slice()),
            Some(&b"GIF89a"[..])
        );

        // The package itself is not a file of the wallpaper: it is read
        // once, not twice.
        assert!(!files.keys().any(|name| name.ends_with(".pkg")));
    }

    /// The package of a wallpaper does not have to be named `scene.pkg`.
    #[test]
    fn a_package_with_another_name_is_used_too() {
        let tree = TempTree::new("gifscene");
        tree.write("project.json", br#"{"title":"Aurora","type":"Scene"}"#);
        write_package(&tree.path().join("gifscene.pkg"), &[("scene.json", b"{}")]);

        let files = WallpaperSource::Folder(tree.path().to_path_buf())
            .read_files()
            .expect("the folder must be read");

        assert!(files.contains_key("scene.json"));
        assert!(files.contains_key("project.json"));
    }

    /// The files of the subfolders of a wallpaper with a package are not
    /// read: they are the cache of the shaders of the Windows version of
    /// the wallpaper.
    #[test]
    fn the_files_of_the_subfolders_are_not_read() {
        let tree = TempTree::new("shaders");
        tree.write("project.json", b"{}");
        tree.write("shaders/blobsSM40/abc.dxs", b"shader");
        write_package(&tree.path().join("scene.pkg"), &[("scene.json", b"{}")]);

        let files = WallpaperSource::Folder(tree.path().to_path_buf())
            .read_files()
            .expect("the folder must be read");

        assert!(!files.keys().any(|name| name.contains("shaders")));
    }

    /// A folder without a package holds the files of the wallpaper itself:
    /// this is the case of a video wallpaper.
    #[test]
    fn a_folder_without_a_package_holds_its_own_files() {
        let tree = TempTree::new("video");
        tree.write(
            "project.json",
            br#"{"title":"Clip","type":"Video","file":"clip.mp4"}"#,
        );
        tree.write("clip.mp4", b"video");
        tree.write("preview.jpg", b"jpeg");

        let files = WallpaperSource::Folder(tree.path().to_path_buf())
            .read_files()
            .expect("the folder must be read");

        assert_eq!(files.len(), 3);
        assert_eq!(
            files.get("project.json").map(|bytes| bytes.as_slice()),
            Some(&br#"{"title":"Clip","type":"Video","file":"clip.mp4"}"#[..])
        );
        assert_eq!(
            files.get("clip.mp4").map(|bytes| bytes.as_slice()),
            Some(&b"video"[..])
        );
        assert_eq!(
            files.get("preview.jpg").map(|bytes| bytes.as_slice()),
            Some(&b"jpeg"[..])
        );
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

    /// A folder that does not exist is an error, not a panic.
    #[test]
    fn a_missing_folder_is_an_error() {
        let tree = TempTree::new("missing-folder");
        let missing = tree.path().join("nowhere");

        let result = WallpaperSource::Folder(missing).read_files();

        assert!(result.is_err());
    }
}
