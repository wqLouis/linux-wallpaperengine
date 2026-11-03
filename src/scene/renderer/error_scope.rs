//! Helpers to capture wgpu errors instead of letting them abort the app.
//!
//! wgpu reports validation errors (a broken shader, an invalid pipeline,
//! ...) through the *device error sink*.  When no error scope is active the
//! sink falls back to the uncaptured error handler, which by default logs
//! "Handling wgpu errors as fatal by default" and panics.  The release
//! profile is built with `panic = "abort"`, so a single broken shader would
//! take the whole application down.
//!
//! [`validation_scope`] runs a closure inside a validation error scope and
//! hands the first error back to the caller, so the caller can log it and
//! degrade gracefully (skip the broken effect) instead of dying.

use wgpu::{Device, Error, ErrorFilter};

/// Run `f` inside a wgpu validation error scope.
///
/// Returns the first validation error raised inside the scope, if any,
/// otherwise the value produced by `f`.
///
/// The scope is pushed and popped on the calling thread - wgpu error scopes
/// are thread local.  Popping the scope resolves immediately on the native
/// backends (the error is already captured in the scope), so this does not
/// block the render loop.
pub fn validation_scope<T>(device: &Device, f: impl FnOnce() -> T) -> Result<T, Error> {
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let value = f();
    match pollster::block_on(scope.pop()) {
        Some(err) => Err(err),
        None => Ok(value),
    }
}
