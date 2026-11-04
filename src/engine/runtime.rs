//! How a wallpaper is rendered and how it is controlled.
//!
//! The wallpaper is rendered on a worker thread: that thread owns the
//! Wayland connection, the layer surface and the graphics app.  The app
//! is not `Send`, so it is created where it is used and never moved
//! between the threads.
//!
//! The host talks to the renderer with two channels: the commands it
//! sends and the statuses it receives.  A command is answered by the
//! next status, and a status that carries problems tells the host what
//! could not be used — nothing is swallowed.
//!
//! ```no_run
//! use linux_wallpaper_engine::engine::EngineConfig;
//! use linux_wallpaper_engine::engine::runtime::WallpaperHandle;
//! use linux_wallpaper_engine::engine::source::WallpaperSource;
//!
//! let source = WallpaperSource::auto("/path/to/scene.pkg".into());
//! let handle = WallpaperHandle::spawn(EngineConfig::default(), source)?;
//!
//! // Later: show another wallpaper and stop it again.
//! handle.switch(WallpaperSource::auto("/path/to/other.pkg".into()))?;
//! handle.stop()?;
//! # Ok::<(), linux_wallpaper_engine::engine::EngineError>(())
//! ```
//!
//! # Nothing panics
//!
//! The release profile keeps `panic = "abort"`: a panic aborts the whole
//! process, the tray application included.  A panic is a bug, never an
//! expected outcome, so every failure of the worker is reported as an
//! [`EngineStatus::Failed`] and every failure that does not stop the
//! engine becomes an [`EngineProblem`] that the host shows to the user.
//! `catch_unwind` is not used anywhere: it cannot catch anything under
//! `panic = "abort"`.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::engine::EngineConfig;
use crate::engine::source::WallpaperSource;
use crate::scene::adapters::{AdapterConfig, wlr_app};
use crate::scene::error::{EngineError, EngineProblem};

/// How long the wallpaper has to come up before the host gives up on it.
const START_TIMEOUT: Duration = Duration::from_secs(10);

/// What the host can ask the renderer to do.
#[derive(Clone, Debug)]
pub enum EngineCommand {
    /// Show another wallpaper.
    ///
    /// The scene is loaded again; the device, the surface and the audio
    /// stream stay.
    Switch(WallpaperSource),
    /// Change the rate the wallpaper is rendered at.
    ///
    /// [None] is "as fast as possible" and [Some](0) the static image
    /// mode.
    SetFps(Option<u32>),
    /// Stop rendering.  The events are still dispatched, so the wallpaper
    /// stays in control.
    Pause,
    /// Render again.
    Resume,
    /// Stop the wallpaper.
    Stop,
}

/// What the renderer tells the host about.
///
/// The status is not [Clone]: it carries the error that caused a failure
/// and the problems of the wallpaper, and those cannot be copied.
#[derive(Debug)]
pub enum EngineStatus {
    /// The wallpaper is starting.
    Starting,
    /// The wallpaper is up.
    Running {
        /// The title of the wallpaper, when its `project.json` has one.
        title: Option<String>,
        /// The files of the wallpaper that could not be used.
        ///
        /// A status carries the problems that appeared since the
        /// previous one: a problem is reported once, never twice.  The
        /// host adds them up and shows them to the user.
        problems: Vec<EngineProblem>,
    },
    /// The wallpaper could not be started.
    Failed(EngineError),
    /// The wallpaper stopped.
    Stopped,
}

/// The handle of a running wallpaper.
///
/// The handle is the only thing the host needs to keep: it sends the
/// commands to the renderer and reads the statuses it sends back.
pub struct WallpaperHandle {
    /// The commands the host sends to the renderer.
    commands: Sender<EngineCommand>,
    /// The statuses the renderer sends to the host.
    statuses: Receiver<EngineStatus>,
    /// The statuses that arrived while the wallpaper came up.
    ///
    /// The wallpaper is waited for when it starts, so the statuses that
    /// arrive in the meantime are kept: they are handed to the host by
    /// the first [WallpaperHandle::poll_status] calls, so nothing is
    /// lost.
    pending: Mutex<VecDeque<EngineStatus>>,
    /// The thread that renders the wallpaper.
    worker: Option<JoinHandle<()>>,
}

impl WallpaperHandle {
    /// Starts the wallpaper on a worker thread and waits for it to come up.
    ///
    /// The worker owns the Wayland connection, the layer surface and the
    /// graphics app: the app is not `Send`, so it is created on the
    /// worker and never moved.
    ///
    /// The call waits for the wallpaper to come up (at most ten seconds)
    /// and says how it went: an error comes back when the wallpaper could
    /// not be started.
    pub fn spawn(config: EngineConfig, source: WallpaperSource) -> Result<Self, EngineError> {
        let (commands, command_rx) = mpsc::channel();
        let (status_tx, statuses) = mpsc::channel();

        let worker = thread::Builder::new()
            .name("wallpaper".to_string())
            .spawn(move || run(config, source, command_rx, status_tx))
            .map_err(EngineError::Io)?;

        // The wallpaper has to come up before the host can use it: the
        // first status that is not "starting" says how it went.
        let mut pending: VecDeque<EngineStatus> = VecDeque::new();
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match statuses.recv_timeout(remaining) {
                Ok(status) => match status {
                    EngineStatus::Running { .. } => {
                        // The wallpaper is up: the status is handed to the
                        // host by the first `poll_status`, so the title
                        // and the problems of the wallpaper are not lost.
                        pending.push_back(status);
                        return Ok(Self {
                            commands,
                            statuses,
                            pending: Mutex::new(pending),
                            worker: Some(worker),
                        });
                    }
                    EngineStatus::Failed(error) => return Err(error),
                    EngineStatus::Stopped => {
                        pending.push_back(EngineStatus::Stopped);
                        return Err(EngineError::Failed(
                            "the wallpaper stopped before it came up",
                        ));
                    }
                    // The wallpaper is still starting: keep waiting.
                    EngineStatus::Starting => pending.push_back(EngineStatus::Starting),
                },
                Err(RecvTimeoutError::Timeout) => {
                    return Err(EngineError::Failed("the wallpaper did not come up in time"));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(EngineError::Failed("the wallpaper thread is gone"));
                }
            }
        }
    }

    /// Shows another wallpaper.
    ///
    /// The scene is loaded again; the device, the surface and the audio
    /// stream stay.
    pub fn switch(&self, source: WallpaperSource) -> Result<(), EngineError> {
        self.send(EngineCommand::Switch(source))
    }

    /// Changes the rate the wallpaper is rendered at.
    ///
    /// [None] is "as fast as possible" and [Some](0) the static image
    /// mode.
    pub fn set_fps(&self, fps: Option<u32>) -> Result<(), EngineError> {
        self.send(EngineCommand::SetFps(fps))
    }

    /// Stops the rendering.  The wallpaper stays in control.
    pub fn pause(&self) -> Result<(), EngineError> {
        self.send(EngineCommand::Pause)
    }

    /// Renders again.
    pub fn resume(&self) -> Result<(), EngineError> {
        self.send(EngineCommand::Resume)
    }

    /// Stops the wallpaper.
    pub fn stop(&self) -> Result<(), EngineError> {
        self.send(EngineCommand::Stop)
    }

    /// The next status of the renderer (non-blocking).
    ///
    /// [None] means that the renderer has nothing to say right now.  The
    /// statuses that arrived while the wallpaper came up are handed out
    /// first.
    pub fn poll_status(&self) -> Option<EngineStatus> {
        let mut pending = self.pending.lock().ok()?;
        if let Some(status) = pending.pop_front() {
            return Some(status);
        }
        drop(pending);

        self.statuses.try_recv().ok()
    }

    /// Sends a command to the renderer.
    fn send(&self, command: EngineCommand) -> Result<(), EngineError> {
        self.commands
            .send(command)
            .map_err(|_| EngineError::Failed("the wallpaper is not running any more"))
    }
}

impl Drop for WallpaperHandle {
    /// Stops the wallpaper.
    ///
    /// The worker thread must not keep the process alive after the handle
    /// is gone: it is asked to stop and waited for.
    fn drop(&mut self) {
        let _ = self.commands.send(EngineCommand::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Runs the wallpaper on the worker thread.
///
/// Everything the renderer needs is created here: the Wayland connection,
/// the layer surface and the graphics app.  Nothing of it is moved to
/// another thread, and nothing of it panics: a failure is reported to
/// the host and the thread ends.
fn run(
    config: EngineConfig,
    source: WallpaperSource,
    commands: Receiver<EngineCommand>,
    status: Sender<EngineStatus>,
) {
    // The host knows the wallpaper is starting.
    let _ = status.send(EngineStatus::Starting);

    let adapter = AdapterConfig {
        source,
        fit_mode: config.fit_mode,
        no_effects: config.no_effects,
        no_mdl: config.no_mdl,
        assets_path: config
            .assets_path
            .map(|path| path.to_string_lossy().to_string()),
        target_fps: config.target_fps,
        show_progress: config.show_progress,
        status: Some(status.clone()),
    };

    // The adapter owns the Wayland connection, the layer surface and the
    // graphics app: they are created on this thread and stay here.
    match wlr_app::run(adapter, Some(commands)) {
        Ok(()) => {
            let _ = status.send(EngineStatus::Stopped);
        }
        Err(error) => {
            let _ = status.send(EngineStatus::Failed(error));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::adapters::FitMode;
    use std::path::PathBuf;

    /// The default configuration is the documented one.
    #[test]
    fn the_default_configuration_is_the_documented_one() {
        let config = EngineConfig::default();

        assert_eq!(config.fit_mode, FitMode::Cover);
        assert_eq!(config.target_fps, None);
        assert!(!config.no_effects);
        assert!(!config.no_mdl);
        assert_eq!(config.assets_path, None);
        assert!(!config.show_progress);
    }

    /// The commands are `Debug` and `Clone`: the host keeps them around.
    #[test]
    fn the_commands_are_debug_and_clone() {
        fn assert_debug_clone<T: std::fmt::Debug + Clone>() {}
        assert_debug_clone::<EngineCommand>();
    }

    /// The statuses are `Debug` and travel between the threads.
    ///
    /// They are not `Clone`: they carry the error of a failure and the
    /// problems of the wallpaper, and those cannot be copied.
    #[test]
    fn the_statuses_are_debug_and_send() {
        fn assert_debug<T: std::fmt::Debug>() {}
        fn assert_send<T: Send>() {}

        assert_debug::<EngineStatus>();
        assert_send::<EngineStatus>();
        assert_send::<EngineError>();
        assert_send::<EngineProblem>();
    }

    /// Whether the machine has a display the wallpaper could be shown on.
    fn has_display() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
    }

    /// A wallpaper that cannot be started fails with an error.
    ///
    /// The test needs a machine without a display: with one, the
    /// wallpaper would really come up (and cover the screen of whoever
    /// runs the tests).
    #[test]
    fn spawning_a_wallpaper_without_a_display_fails() {
        if has_display() {
            eprintln!("the machine has a display: the wallpaper is not started");
            return;
        }

        let source = WallpaperSource::Package(PathBuf::from("/nonexistent/scene.pkg"));
        let result = WallpaperHandle::spawn(EngineConfig::default(), source);

        match result {
            Ok(_) => panic!("the wallpaper cannot start without a display"),
            Err(error) => {
                // The error says what went wrong and carries its cause.
                assert!(!error.trace().is_empty());
            }
        }
    }
}
