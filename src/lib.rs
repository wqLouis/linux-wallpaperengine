//! Linux Wallpaper Engine — the reusable engine.
//!
//! This crate is the engine itself: it parses the Wallpaper Engine
//! packages, loads the scenes they describe and renders them with
//! [`wgpu`].  The adapters in [`scene::adapters`] put the rendered
//! frames on the screen (Wayland layer shell, a plain window, the GNOME
//! shell extension or a macOS window).
//!
//! The crate is meant to be embedded: the command line interface
//! (`linux-wallpaper-engine`) and the graphical interface are thin
//! wrappers around it.

pub mod collections;
pub mod engine;
pub mod scene;

/// The Wallpaper Engine package parser, re-exported so that consumers
/// (the CLI, the GUI) do not need their own dependency on it.
pub use pkg_parser;

/// The maximum number of textures that a shader stage can bind.
///
/// The renderer uploads the textures of a scene into a binding array, so
/// the size of that array is the limit of the renderer.
pub const MAX_TEXTURE: u32 = 512;
