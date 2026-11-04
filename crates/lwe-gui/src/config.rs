//! The settings the GUI remembers.
//!
//! The GUI is a tray application: it is started by the desktop and it
//! has to know where the wallpapers are without asking every time.  The
//! settings live in a small JSON file in the configuration folder of the
//! XDG base directories, so they survive a restart of the application.
//!
//! A file that cannot be read is not a reason to refuse to start: the
//! settings fall back to their defaults and the user is told about it in
//! the log.  The only thing that is really remembered is the workshop
//! folder — everything else has a sensible default.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The name of the folder the settings and the previews live in.
const APP_DIR: &str = "linux-wallpaper-engine";

/// The name of the file the settings are written to.
const CONFIG_FILE: &str = "gui.json";

/// The settings of the GUI.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuiConfig {
    /// The workshop folder (or the Steam root) the wallpapers are read
    /// from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workshop_dir: Option<PathBuf>,
    /// The rate the wallpaper is rendered at.
    ///
    /// [`None`] renders as fast as the machine can, [`Some`](0) shows the
    /// wallpaper as a still image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_fps: Option<u32>,
}

impl GuiConfig {
    /// Reads the settings of the user.
    ///
    /// Settings that cannot be read are the default settings: the GUI
    /// starts anyway and asks the user for the workshop folder.
    pub fn load() -> Self {
        Self::load_from(&config_path())
    }

    /// Reads the settings from a file.
    pub fn load_from(path: &Path) -> Self {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    log::warn!(
                        "the settings '{}' could not be read: {error}",
                        path.display()
                    );
                }
                return Self::default();
            }
        };

        match serde_json::from_slice(&bytes) {
            Ok(config) => config,
            Err(error) => {
                log::warn!(
                    "the settings '{}' are broken: {error} — the defaults are used",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Writes the settings of the user.
    pub fn save(&self) -> io::Result<()> {
        self.save_to(&config_path())
    }

    /// Writes the settings to a file.
    ///
    /// The folder of the file is created when it is not there yet.
    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }

        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| io::Error::other(format!("the settings do not fit: {error}")))?;
        std::fs::write(path, bytes)
    }
}

/// The file the settings are written to.
///
/// The XDG configuration folder is used, so the settings live where every
/// other application puts them.
pub fn config_path() -> PathBuf {
    base_dir("XDG_CONFIG_HOME", ".config")
        .join(APP_DIR)
        .join(CONFIG_FILE)
}

/// The folder the previews of the wallpapers are cached in.
pub fn cache_dir() -> PathBuf {
    base_dir("XDG_CACHE_HOME", ".cache").join(APP_DIR)
}

/// A folder of the XDG base directories.
///
/// The variable of the XDG specification wins; the fallback is used when it
/// is not set, which is what happens in a bare environment.
fn base_dir(variable: &str, fallback: &str) -> PathBuf {
    if let Some(directory) = std::env::var_os(variable)
        && !directory.is_empty()
    {
        return PathBuf::from(directory);
    }

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temporary folder that is removed when the test is over.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        /// Creates a temporary folder with a unique name.
        fn new(name: &str) -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "linux-wallpaperengine-gui-config-{name}-{}-{unique}",
                std::process::id()
            ));

            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("the temporary folder must be created");

            Self { path }
        }

        /// The path of the folder.
        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// The settings survive the trip to the disk.
    #[test]
    fn the_settings_survive_a_round_trip() {
        let folder = TempDir::new("round-trip");
        let path = folder.path().join("gui.json");

        let config = GuiConfig {
            workshop_dir: Some(PathBuf::from(
                "/home/user/steamapps/workshop/content/431960",
            )),
            target_fps: Some(30),
        };

        config.save_to(&path).expect("the settings must be written");
        let loaded = GuiConfig::load_from(&path);

        assert_eq!(loaded, config);
    }

    /// Settings that are not there are the default settings.
    #[test]
    fn missing_settings_are_the_default_settings() {
        let folder = TempDir::new("missing");

        let config = GuiConfig::load_from(&folder.path().join("nowhere.json"));

        assert_eq!(config, GuiConfig::default());
    }

    /// Settings that are broken are the default settings: the GUI starts
    /// anyway.
    #[test]
    fn broken_settings_are_the_default_settings() {
        let folder = TempDir::new("broken");
        let path = folder.path().join("gui.json");
        std::fs::write(&path, b"{ this is not json").expect("the settings must be written");

        let config = GuiConfig::load_from(&path);

        assert_eq!(config, GuiConfig::default());
    }

    /// The folder of the settings file is created when it is not there.
    #[test]
    fn the_folder_of_the_settings_is_created() {
        let folder = TempDir::new("created");
        let path = folder.path().join("deeply").join("nested").join("gui.json");

        GuiConfig::default()
            .save_to(&path)
            .expect("the settings must be written");

        assert!(path.is_file());
    }
}
