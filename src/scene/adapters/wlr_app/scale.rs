//! Fractional-scale and viewporter protocol management.
//!
//! Encapsulates the `wp_fractional_scale` and `wp_viewporter` protocol objects
//! used by the wlr-layer-shell adapter to handle HiDPI output correctly.
//!
//! The compositor sends a `preferred_scale` event with an integer numerator
//! representing the scale ×120 (e.g. 180 = 1.5×).  When the compositor does
//! not support the protocol, a fallback scale is computed from output mode vs.
//! logical size.

use log;
use smithay_client_toolkit::output::OutputState;
use wayland_client::protocol::wl_output;
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::WpFractionalScaleV1,
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};

/// Opaque data tag for `wp_fractional_scale_*` and `wp_viewporter` dispatch.
#[derive(Debug)]
pub struct FractionalScaleData;

/// Manages fractional-scale and viewporter protocol state.
///
/// Owns the protocol objects (keeping them alive) and tracks the current
/// preferred scale factor.  Provides a [`compute_from_output`](Self::compute_from_output)
/// fallback for compositors that do not advertise the protocol.
pub struct ScaleState {
    /// `wp_fractional_scale_manager_v1` global (kept alive).
    #[allow(dead_code)]
    pub mgr: Option<WpFractionalScaleManagerV1>,
    /// Per-surface `wp_fractional_scale_v1` object.
    #[allow(dead_code)]
    pub fractional: Option<WpFractionalScaleV1>,
    /// `wp_viewporter` global (kept alive).
    #[allow(dead_code)]
    pub viewporter: Option<WpViewporter>,
    /// `wp_viewport` for the main surface.
    pub viewport: Option<WpViewport>,

    /// Preferred scale numerator (× 1/120).  Default 120 = 1.0×.
    pub scale_num: u32,
    /// `true` once a `preferred_scale` event has been received (or computed
    /// via fallback), locking out further automatic recomputation.
    pub scale_received: bool,
    /// The value of `scale_num` at the last successful reconfigure, used to
    /// skip redundant reapplies.
    pub last_applied_scale: u32,
}

impl ScaleState {
    /// Create a new `ScaleState`, taking ownership of the protocol objects.
    ///
    /// `scale_num` starts at 120 (1.0×) until the compositor provides a value.
    pub fn new(
        mgr: Option<WpFractionalScaleManagerV1>,
        fractional: Option<WpFractionalScaleV1>,
        viewporter: Option<WpViewporter>,
        viewport: Option<WpViewport>,
    ) -> Self {
        Self {
            mgr,
            fractional,
            viewporter,
            viewport,
            scale_num: 120,
            scale_received: false,
            last_applied_scale: 120,
        }
    }

    /// Handle a `preferred_scale` event from the compositor.
    pub fn handle_preferred_scale(&mut self, scale: u32) {
        log::info!("preferred_scale: {} (×{:.2})", scale, scale as f64 / 120.0);
        self.scale_num = scale;
        self.scale_received = true;
    }

    /// Fallback: compute a scale factor from output mode vs. logical size.
    ///
    /// Used when the compositor does not advertise
    /// `wp_fractional_scale_manager_v1`.  Returns `true` if a scale was set.
    ///
    /// `fallback_logical` is used when `info.logical_size` is not yet
    /// available (some compositors send output events asynchronously, so
    /// we fall back to the known surface logical size from the last
    /// `configure` event).
    pub fn compute_from_output(
        &mut self,
        output_state: &OutputState,
        output: &wl_output::WlOutput,
        fallback_logical: Option<(u32, u32)>,
    ) -> bool {
        if self.scale_received {
            return false;
        }
        let Some(info) = output_state.info(output) else {
            return false;
        };
        let Some(mode) = info.modes.iter().find(|m| m.current) else {
            return false;
        };
        let (log_w, log_h) = match info.logical_size {
            Some((w, h)) if w > 0 && h > 0 => (w, h),
            _ => match fallback_logical {
                Some((w, h)) => (w as i32, h as i32),
                _ => return false,
            },
        };
        if log_w <= 0 || log_h <= 0 {
            return false;
        }

        let w_scale = mode.dimensions.0 as f64 / log_w as f64;
        let h_scale = mode.dimensions.1 as f64 / log_h as f64;
        let computed = ((w_scale + h_scale) / 2.0 * 120.0).round() as u32;

        if computed > self.scale_num {
            self.scale_num = computed;
            self.scale_received = true;
            return true;
        }
        false
    }

}
