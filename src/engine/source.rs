//! Where a wallpaper comes from.

use std::path::{Path, PathBuf};

use crate::collections::Map;
use crate::scene::error::EngineError;

/// Where a wallpaper comes from.
///
/// Wallpaper Engine ships two shapes: a `.pkg` archive and a folder with a
/// `project.json` and the wallpaper's files next to it (the Steam workshop
/// layout).  Both are first class here.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WallpaperSource {
    /// A `.pkg` archive.
    Package(PathBuf),
    /// A folder that holds `project.json` and the files of one wallpaper.
    Folder(PathBuf),
}

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
            WallpaperSource::Package(path) => {
                let package =
                    pkg_parser::pkg_parser::parser::Pkg::new(path).map_err(EngineError::Package)?;
                Ok(package.files.into_iter().collect())
            }
            WallpaperSource::Folder(path) => read_folder(path),
        }
    }
}

/// An io error that says which path it happened at.
fn io_error(path: &Path, error: std::io::Error) -> std::io::Error {
    std::io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

/// Reads a folder of wallpaper files, keyed by their path inside it.
///
/// The paths are relative to the folder and use `/` as the separator, so
/// they look exactly like the paths of an archive.
fn read_folder(root: &Path) -> Result<Map<String, Vec<u8>>, EngineError> {
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
