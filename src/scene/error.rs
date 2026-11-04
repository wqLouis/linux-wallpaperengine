//! The error type of the engine.
//!
//! The engine never panics on a broken wallpaper, a missing device or a
//! missing audio device: every failure travels up the call stack as an
//! error, and the error keeps the cause it had, so the failure can be
//! reported with the whole trace:
//!
//! ```text
//! the scene.json could not be parsed: expected value at line 3
//! ```
//!
//! The variants are specific, so that a caller can match on them: a
//! missing audio device is not fatal for a silent wallpaper, a missing
//! graphics device is.  Every variant carries the error that caused it
//! and the chain of causes continues through Error::source.

use std::error::Error as _;
use std::fmt;

use crate::scene::loader::scene_loader::SceneError;

/// Everything that can go wrong while loading or rendering a wallpaper.
#[derive(Debug)]
pub enum EngineError {
    /// The wallpaper package could not be read.
    Package(std::io::Error),
    /// The wallpaper could not be loaded.
    Scene(SceneError),
    /// No graphics adapter could be found.
    Adapter(wgpu::RequestAdapterError),
    /// The graphics device could not be created.
    Device(wgpu::RequestDeviceError),
    /// The surface of the output could not be created.
    Surface(wgpu::CreateSurfaceError),
    /// The audio device could not be opened.
    Audio(rodio::StreamError),
    /// The output window could not be created.
    Window(WindowError),
    /// The feature is not available on this platform.
    Unsupported(UnsupportedError),
    /// A file could not be read.
    Io(std::io::Error),
    /// The session bus is not reachable.
    ///
    /// The GNOME backend talks to the Shell extension over the session
    /// bus: when the bus is not there, the wallpaper cannot be shown.
    Bus(zbus::Error),
    /// The failure has no error of its own: the string is the message.
    ///
    /// This is the leaf of the enum: the static string is what the failure
    /// really is, and nothing underneath it to point at.
    Failed(&'static str),
}

/// A problem that did not stop the engine, but that the user must know about.
///
/// The loader and the renderer collect these instead of swallowing them: a
/// wallpaper with a broken model still plays, but the user is told that
/// something was skipped.  The CLI prints them, the GUI shows them.
#[derive(Debug)]
pub enum EngineProblem {
    /// A file of the wallpaper could not be read or parsed.
    Asset {
        /// The path of the file inside the wallpaper.
        path: String,
        /// Why the file could not be used.
        error: SceneError,
    },
    /// A file of the wallpaper is of a kind that is not supported.
    Unsupported {
        /// The path of the file inside the wallpaper.
        path: String,
        /// Why the kind is not supported.
        error: UnsupportedError,
    },
    /// The audio device is not available: the wallpaper plays without sound.
    Audio(rodio::StreamError),
    /// The renderer could not draw the wallpaper.
    Render(RenderError),
}

/// Why the renderer could not draw a frame.
///
/// A frame that cannot be drawn does not stop the wallpaper: the frame is
/// skipped and the problem is reported to the host, which shows it to the
/// user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderError {
    /// The scene is not loaded: there is nothing to draw yet.
    NotReady,
    /// The surface did not give a frame to draw into.
    Surface,
}

impl fmt::Display for EngineProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineProblem::Asset { path, error } => {
                write!(f, "{path} could not be used: {error}")
            }
            EngineProblem::Unsupported { path, error } => {
                write!(f, "{path} could not be used: {error}")
            }
            EngineProblem::Audio(error) => {
                write!(f, "the audio device is not available: {error}")
            }
            EngineProblem::Render(error) => {
                write!(f, "the wallpaper could not be drawn: {error}")
            }
        }
    }
}

impl std::error::Error for EngineProblem {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EngineProblem::Asset { error, .. } => Some(error),
            EngineProblem::Unsupported { error, .. } => Some(error),
            EngineProblem::Audio(error) => Some(error),
            EngineProblem::Render(error) => Some(error),
        }
    }
}

/// Why the output window could not be created.
#[derive(Debug)]
pub enum WindowError {
    /// The connection to the display server failed.
    Connect(wayland_client::ConnectError),
    /// The display server does not offer the protocol the adapter needs.
    MissingProtocol(&'static str),
    /// The display server reported a protocol error.
    Protocol(wayland_client::backend::WaylandError),
    /// The windowing system refused to create its event loop.
    EventLoop(winit::error::EventLoopError),
    /// The windowing system refused to create the window.
    Create(winit::error::OsError),
    /// The display server sent an event that could not be dispatched.
    Dispatch(wayland_client::DispatchError),
    /// The globals of the display server could not be read.
    Globals(wayland_client::globals::GlobalError),
    /// A protocol of the display server could not be bound.
    Bind(wayland_client::globals::BindError),
}

/// Why a feature is not available.
#[derive(Debug)]
pub enum UnsupportedError {
    /// The platform does not support the adapter.
    Platform(&'static str),
    /// The format of the file is not supported.
    Format(&'static str),
}

impl EngineError {
    /// The whole chain of causes, one per line.
    ///
    /// The GUI shows this in a dialog, the CLI in its message.
    pub fn trace(&self) -> String {
        let mut lines = vec![self.to_string()];
        let mut source = self.source();
        while let Some(error) = source {
            lines.push(error.to_string());
            source = error.source();
        }
        lines.join("\n")
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Package(error) => {
                write!(f, "the wallpaper package could not be read: {error}")
            }
            EngineError::Scene(error) => write!(f, "{error}"),
            EngineError::Adapter(error) => write!(f, "no graphics adapter: {error}"),
            EngineError::Device(error) => {
                write!(f, "the graphics device could not be created: {error}")
            }
            EngineError::Surface(error) => {
                write!(f, "the output surface could not be created: {error}")
            }
            EngineError::Audio(error) => write!(f, "the audio device could not be opened: {error}"),
            EngineError::Window(error) => write!(f, "the output window could not be created: {error}"),
            EngineError::Unsupported(error) => write!(f, "{error}"),
            EngineError::Io(error) => write!(f, "a file could not be read: {error}"),
            EngineError::Bus(error) => write!(f, "the session bus is not reachable: {error}"),
            EngineError::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for EngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EngineError::Package(error) => Some(error),
            EngineError::Scene(error) => Some(error),
            EngineError::Adapter(error) => Some(error),
            EngineError::Device(error) => Some(error),
            EngineError::Surface(error) => Some(error),
            EngineError::Audio(error) => Some(error),
            EngineError::Window(error) => Some(error),
            EngineError::Unsupported(error) => Some(error),
            EngineError::Io(error) => Some(error),
            EngineError::Bus(error) => Some(error),
            EngineError::Failed(_) => None,
        }
    }
}

impl fmt::Display for WindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WindowError::Connect(error) => write!(f, "the display server is not reachable: {error}"),
            WindowError::MissingProtocol(protocol) => {
                write!(f, "the display server does not support {protocol}")
            }
            WindowError::Protocol(error) => write!(f, "the display server reported an error: {error}"),
            WindowError::EventLoop(error) => write!(f, "the event loop failed: {error}"),
            WindowError::Create(error) => write!(f, "the window could not be created: {error}"),
            WindowError::Dispatch(error) => {
                write!(f, "the display server sent an event that failed: {error}")
            }
            WindowError::Globals(error) => {
                write!(f, "the display server did not answer: {error}")
            }
            WindowError::Bind(error) => {
                write!(f, "the display server does not offer the protocol: {error}")
            }
        }
    }
}

impl std::error::Error for WindowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WindowError::Connect(error) => Some(error),
            WindowError::MissingProtocol(_) => None,
            WindowError::Protocol(error) => Some(error),
            WindowError::EventLoop(error) => Some(error),
            WindowError::Create(error) => Some(error),
            WindowError::Dispatch(error) => Some(error),
            WindowError::Globals(error) => Some(error),
            WindowError::Bind(error) => Some(error),
        }
    }
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::NotReady => write!(f, "the scene is not loaded"),
            RenderError::Surface => write!(f, "the surface did not give a frame"),
        }
    }
}

impl std::error::Error for RenderError {}

impl fmt::Display for UnsupportedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnsupportedError::Platform(what) => write!(f, "{what} is not supported on this platform"),
            UnsupportedError::Format(what) => write!(f, "the format {what} is not supported"),
        }
    }
}

impl std::error::Error for UnsupportedError {}

impl From<SceneError> for EngineError {
    fn from(error: SceneError) -> Self {
        EngineError::Scene(error)
    }
}

impl From<std::io::Error> for EngineError {
    fn from(error: std::io::Error) -> Self {
        EngineError::Io(error)
    }
}
