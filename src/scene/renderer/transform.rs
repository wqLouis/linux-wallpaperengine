//! Object-space transforms for Wallpaper Engine layers.
//!
//! Mirrors the scene.json fields per object:
//!   * `origin`  → [`Transform::position`]
//!   * `angles`  → [`Transform::rotation`]
//!   * `scale`   → [`Transform::scale`]
//!   * `pivot`   → [`Transform::pivot`]
//!   * `alignment` → [`Transform::alignment`]
//!
//! Order of operations in Wallpaper Engine:
//!   1. Translate the object so the pivot is at the origin.
//!   2. Rotate around the pivot (Euler `angles`).
//!   3. Scale around the pivot (`scale`).
//!   4. Translate the result to its world `position` (`origin`).
//!
//! For a child object, the local position is interpreted as an offset in
//! the parent's frame, and the composed model matrix is
//! `M_parent_world * M_child_local`.
//!
//! [`Alignment`] is an optional pre-shift of the pivot, used to anchor
//! text/image layers to a specific side (left, right, top-left, etc.).
//!
//! ## Rotation convention
//!
//! We use [`EulerRot::XYZ`] with arguments `(x, y, z)` matching the
//! scene.json `angles` order.  This is a standard extrinsic Tait-Bryan
//! rotation: apply X first, then Y, then Z.  For 2D scenes (which is
//! the common case) only `angles.z` is non-zero.

use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// Anchor alignment — which side of the object the layer is anchored to.
///
/// The alignment is implemented as a 2D pivot shift: e.g. a layer with
/// `Right` alignment has its pivot at the right edge, so when its
/// `position` is at the right edge of the scene, the rest of the object
/// extends to the left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Alignment {
    #[default]
    Center,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Alignment {
    /// Return the pivot shift (in object-local units) implied by this
    /// alignment, given the object's rendered size.
    ///
    /// For example, an object of size (100, 50) with `Right` alignment
    /// gets a pivot shift of (+50, 0) — the right edge becomes the pivot.
    pub fn pivot_offset(self, size: Vec2) -> Vec2 {
        let hx = size.x * 0.5;
        let hy = size.y * 0.5;
        match self {
            Alignment::Center => Vec2::ZERO,
            Alignment::Left => Vec2::new(-hx, 0.0),
            Alignment::Right => Vec2::new(hx, 0.0),
            Alignment::Top => Vec2::new(0.0, hy),
            Alignment::Bottom => Vec2::new(0.0, -hy),
            Alignment::TopLeft => Vec2::new(-hx, hy),
            Alignment::TopRight => Vec2::new(hx, hy),
            Alignment::BottomLeft => Vec2::new(-hx, -hy),
            Alignment::BottomRight => Vec2::new(hx, -hy),
        }
    }
}

/// An object's local transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// World (or parent-local) position — the scene.json `origin`.
    pub position: Vec3,
    /// Euler rotation in radians — the scene.json `angles`.
    pub rotation: Vec3,
    /// Non-uniform scale — the scene.json `scale`.
    pub scale: Vec3,
    /// Pivot point (in object-local space, pre-scale) — the scene.json `pivot`.
    pub pivot: Vec3,
    /// Anchor alignment — the scene.json `alignment`.
    pub alignment: Alignment,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
            pivot: Vec3::ZERO,
            alignment: Alignment::Center,
        }
    }
}

impl Transform {
    /// Build a new transform with the given position/rotation/scale and
    /// a pivot of zero and `Center` alignment.
    pub fn new(position: Vec3, rotation: Vec3, scale: Vec3) -> Self {
        Self {
            position,
            rotation,
            scale,
            ..Self::default()
        }
    }

    /// Builder: set the pivot.
    pub fn with_pivot(mut self, pivot: Vec3) -> Self {
        self.pivot = pivot;
        self
    }

    /// Builder: set the alignment.
    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Build the rotation quaternion from the Euler angles.
    ///
    /// Uses [`EulerRot::XYZ`] with arguments `(x, y, z)` so the call
    /// matches the scene.json `angles` field.  For 2D scenes (the
    /// common case) only `rotation.z` is non-zero.
    pub fn rotation_quat(&self) -> Quat {
        Quat::from_euler(EulerRot::XYZ, self.rotation.x, self.rotation.y, self.rotation.z)
    }
}

/// Build the 4×4 model matrix for an object with the given transform and
/// rendered size.
///
///   `M = T(position) * R(rotation) * S(scale) * T(-effective_pivot)`
///
/// where `effective_pivot = pivot + alignment_offset(size)`.
///
/// This means: first translate so the pivot sits at the origin, then
/// scale and rotate around that origin, then translate the result to
/// the world position.
pub fn build_model_matrix(transform: &Transform, size: Vec2) -> Mat4 {
    let alignment_offset = transform.alignment.pivot_offset(size);
    let effective_pivot = transform.pivot + alignment_offset.extend(0.0);

    Mat4::from_translation(transform.position)
        * Mat4::from_quat(transform.rotation_quat())
        * Mat4::from_scale(transform.scale)
        * Mat4::from_translation(-effective_pivot)
}

/// Compose a child's local model matrix with a parent's world matrix.
///   `M_child_world = M_parent_world * M_child_local`
pub fn compose(parent_world: Mat4, child_local: Mat4) -> Mat4 {
    parent_world * child_local
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    const EPS: f32 = 1e-5;

    fn vec3_approx_eq(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < EPS
    }

    // ── Alignment ──────────────────────────────────────────────────

    #[test]
    fn alignment_center_is_no_offset() {
        assert_eq!(Alignment::Center.pivot_offset(Vec2::new(100.0, 50.0)), Vec2::ZERO);
    }

    #[test]
    fn alignment_left_shifts_pivot_to_left_edge() {
        assert_eq!(Alignment::Left.pivot_offset(Vec2::new(100.0, 50.0)), Vec2::new(-50.0, 0.0));
    }

    #[test]
    fn alignment_right_shifts_pivot_to_right_edge() {
        assert_eq!(Alignment::Right.pivot_offset(Vec2::new(100.0, 50.0)), Vec2::new(50.0, 0.0));
    }

    #[test]
    fn alignment_top_left_corners() {
        assert_eq!(Alignment::TopLeft.pivot_offset(Vec2::new(100.0, 50.0)), Vec2::new(-50.0, 25.0));
        assert_eq!(Alignment::TopRight.pivot_offset(Vec2::new(100.0, 50.0)), Vec2::new(50.0, 25.0));
        assert_eq!(Alignment::BottomLeft.pivot_offset(Vec2::new(100.0, 50.0)), Vec2::new(-50.0, -25.0));
        assert_eq!(Alignment::BottomRight.pivot_offset(Vec2::new(100.0, 50.0)), Vec2::new(50.0, -25.0));
    }

    // ── Model matrix basics ────────────────────────────────────────

    #[test]
    fn identity_transform_yields_identity_matrix() {
        let t = Transform::default();
        let m = build_model_matrix(&t, Vec2::new(10.0, 10.0));
        assert!(vec3_approx_eq(m.transform_point3(Vec3::ZERO), Vec3::ZERO));
        // The identity matrix has 1s on the diagonal.
        for i in 0..4 {
            assert!((m.col(i)[i] - 1.0).abs() < EPS);
        }
    }

    #[test]
    fn pure_translation_moves_origin() {
        let t = Transform {
            position: Vec3::new(10.0, 20.0, 0.0),
            ..Transform::default()
        };
        let m = build_model_matrix(&t, Vec2::new(100.0, 100.0));
        assert!(vec3_approx_eq(m.transform_point3(Vec3::ZERO), Vec3::new(10.0, 20.0, 0.0)));
    }

    #[test]
    fn pure_scale_doubles_local_point() {
        let t = Transform {
            position: Vec3::new(100.0, 100.0, 0.0),
            scale: Vec3::new(2.0, 2.0, 1.0),
            ..Transform::default()
        };
        let m = build_model_matrix(&t, Vec2::new(100.0, 100.0));
        // Local (10, 0) → (120, 100) in world
        assert!(vec3_approx_eq(m.transform_point3(Vec3::new(10.0, 0.0, 0.0)), Vec3::new(120.0, 100.0, 0.0)));
    }

    // ── Pivot ──────────────────────────────────────────────────────

    #[test]
    fn pivot_maps_to_world_position() {
        // Pivot is fixed at world `position`, not at its own coordinates.
        let t = Transform {
            position: Vec3::new(50.0, 25.0, 0.0),
            pivot: Vec3::new(10.0, 0.0, 0.0),
            ..Transform::default()
        };
        let m = build_model_matrix(&t, Vec2::new(100.0, 100.0));
        // Local pivot (10, 0) maps to world position (50, 25).
        assert!(vec3_approx_eq(m.transform_point3(Vec3::new(10.0, 0.0, 0.0)), Vec3::new(50.0, 25.0, 0.0)));
    }

    #[test]
    fn rotation_around_pivot_ccw() {
        // Rotate 90° CCW around the pivot at (10, 0).  Position is (0, 0).
        let t = Transform {
            position: Vec3::ZERO,
            rotation: Vec3::new(0.0, 0.0, FRAC_PI_2),
            pivot: Vec3::new(10.0, 0.0, 0.0),
            ..Transform::default()
        };
        let m = build_model_matrix(&t, Vec2::new(100.0, 100.0));

        // Pivot itself: (10, 0) → (0, 0) (the position).
        assert!(vec3_approx_eq(m.transform_point3(Vec3::new(10.0, 0.0, 0.0)), Vec3::ZERO));

        // 5 units right of pivot: (15, 0) → 5 units up from position (0, 5).
        assert!(vec3_approx_eq(m.transform_point3(Vec3::new(15.0, 0.0, 0.0)), Vec3::new(0.0, 5.0, 0.0)));

        // 5 units above pivot: (10, 5) → 5 units left of position (-5, 0).
        assert!(vec3_approx_eq(m.transform_point3(Vec3::new(10.0, 5.0, 0.0)), Vec3::new(-5.0, 0.0, 0.0)));
    }

    #[test]
    fn scale_around_pivot_keeps_pivot_fixed() {
        // Scale 2x around the pivot at (10, 0).
        let t = Transform {
            position: Vec3::ZERO,
            scale: Vec3::new(2.0, 2.0, 1.0),
            pivot: Vec3::new(10.0, 0.0, 0.0),
            ..Transform::default()
        };
        let m = build_model_matrix(&t, Vec2::new(100.0, 100.0));

        // Pivot is fixed.
        assert!(vec3_approx_eq(m.transform_point3(Vec3::new(10.0, 0.0, 0.0)), Vec3::ZERO));

        // 5 units right of the pivot doubles to 10 units right.
        assert!(vec3_approx_eq(m.transform_point3(Vec3::new(15.0, 0.0, 0.0)), Vec3::new(10.0, 0.0, 0.0)));
    }

    // ── Alignment in model matrix ──────────────────────────────────

    #[test]
    fn alignment_shifts_pivot_in_model_matrix() {
        // Right-aligned object of width 100: pivot shifts to +50.
        let t = Transform {
            position: Vec3::new(200.0, 0.0, 0.0),
            alignment: Alignment::Right,
            ..Transform::default()
        };
        let m = build_model_matrix(&t, Vec2::new(100.0, 50.0));

        // The right edge of the object (x=+50 in local) should be at the
        // position (x=200).
        assert!(vec3_approx_eq(m.transform_point3(Vec3::new(50.0, 0.0, 0.0)), Vec3::new(200.0, 0.0, 0.0)));

        // The left edge of the object (x=-50 in local) should be at x=100.
        assert!(vec3_approx_eq(m.transform_point3(Vec3::new(-50.0, 0.0, 0.0)), Vec3::new(100.0, 0.0, 0.0)));
    }

    // ── Parent composition ─────────────────────────────────────────

    #[test]
    fn child_translated_under_parent() {
        let parent = Transform {
            position: Vec3::new(100.0, 0.0, 0.0),
            ..Transform::default()
        };
        let child = Transform {
            position: Vec3::new(10.0, 0.0, 0.0),
            ..Transform::default()
        };
        let composed = compose(
            build_model_matrix(&parent, Vec2::new(10.0, 10.0)),
            build_model_matrix(&child, Vec2::new(10.0, 10.0)),
        );
        assert!(vec3_approx_eq(composed.transform_point3(Vec3::ZERO), Vec3::new(110.0, 0.0, 0.0)));
    }

    #[test]
    fn child_offset_rotated_by_parent() {
        // Parent rotated 90° CCW around z.  Child's local x-axis becomes world y.
        let parent = Transform {
            position: Vec3::ZERO,
            rotation: Vec3::new(0.0, 0.0, FRAC_PI_2),
            ..Transform::default()
        };
        let child = Transform {
            position: Vec3::new(10.0, 0.0, 0.0),
            ..Transform::default()
        };
        let composed = compose(
            build_model_matrix(&parent, Vec2::new(10.0, 10.0)),
            build_model_matrix(&child, Vec2::new(10.0, 10.0)),
        );
        // Child at parent-local (10, 0) → world (0, 10) (CCW 90° in y-up).
        assert!(vec3_approx_eq(composed.transform_point3(Vec3::ZERO), Vec3::new(0.0, 10.0, 0.0)));
    }

    #[test]
    fn child_offset_scaled_by_parent() {
        // Parent scaled 2x.  Child's local position is also scaled.
        let parent = Transform {
            position: Vec3::ZERO,
            scale: Vec3::new(2.0, 2.0, 1.0),
            ..Transform::default()
        };
        let child = Transform {
            position: Vec3::new(10.0, 0.0, 0.0),
            ..Transform::default()
        };
        let composed = compose(
            build_model_matrix(&parent, Vec2::new(10.0, 10.0)),
            build_model_matrix(&child, Vec2::new(10.0, 10.0)),
        );
        // Child at (10, 0) under 2x scale → (20, 0).
        assert!(vec3_approx_eq(composed.transform_point3(Vec3::ZERO), Vec3::new(20.0, 0.0, 0.0)));
    }

    #[test]
    fn grandchild_inherits_through_chain() {
        // Grandparent at (100, 0), parent at (10, 0) in grandparent frame,
        // child at (5, 0) in parent frame.  World position: (115, 0).
        let grandparent = Transform {
            position: Vec3::new(100.0, 0.0, 0.0),
            ..Transform::default()
        };
        let parent = Transform {
            position: Vec3::new(10.0, 0.0, 0.0),
            ..Transform::default()
        };
        let child = Transform {
            position: Vec3::new(5.0, 0.0, 0.0),
            ..Transform::default()
        };
        let composed = compose(
            compose(
                build_model_matrix(&grandparent, Vec2::new(10.0, 10.0)),
                build_model_matrix(&parent, Vec2::new(10.0, 10.0)),
            ),
            build_model_matrix(&child, Vec2::new(10.0, 10.0)),
        );
        assert!(vec3_approx_eq(composed.transform_point3(Vec3::ZERO), Vec3::new(115.0, 0.0, 0.0)));
    }

    #[test]
    fn parent_visibility_propagates_through_compose() {
        // Just a sanity check that the compose function does not need
        // any visibility data — that's handled separately in the loader.
        let _ = compose(Mat4::IDENTITY, Mat4::IDENTITY);
    }

    // ── Generic parent-scale regression ────────────────────────────
    //
    // Validates that a child's world position and rendered size are
    // both affected by the parent's scale, matching the pre-rewrite
    // behaviour of `object.origin + object.origin * parent.scale`.

    #[test]
    fn parent_scale_affects_child_position_and_size() {
        let parent = Transform {
            position: Vec3::new(100.0, 50.0, 0.0),
            scale: Vec3::new(0.5, 0.5, 1.0),
            ..Transform::default()
        };
        let child = Transform {
            position: Vec3::new(-200.0, -300.0, 0.0),
            ..Transform::default()
        };
        let composed = compose(
            build_model_matrix(&parent, Vec2::new(10.0, 10.0)),
            build_model_matrix(&child, Vec2::new(400.0, 600.0)),
        );

        // Child origin (0, 0) lands at parent.origin + child.origin * parent.scale
        // = (100 - 200*0.5, 50 - 300*0.5, 0) = (0, -100, 0).
        let origin_world = composed.transform_point3(Vec3::ZERO);
        assert!(vec3_approx_eq(origin_world, Vec3::new(0.0, -100.0, 0.0)));

        // Child top-left corner (size = 400 x 600, half = 200 x 300)
        // after the parent scale (0.5) lands at:
        //   origin + (-200 * 0.5, +300 * 0.5) = (-100, +50).
        let half = Vec3::new(200.0, 300.0, 0.0);
        let top_left_world = composed.transform_point3(Vec3::new(-half.x, half.y, 0.0));
        assert!(vec3_approx_eq(top_left_world, Vec3::new(-100.0, 50.0, 0.0)));

        // The rendered size is (400 * 0.5, 600 * 0.5) = (200, 300).
        let bottom_right_world = composed.transform_point3(Vec3::new(half.x, -half.y, 0.0));
        let width = bottom_right_world.x - top_left_world.x;
        let height = top_left_world.y - bottom_right_world.y;
        assert!((width - 200.0).abs() < 0.01);
        assert!((height - 300.0).abs() < 0.01);
    }
}
