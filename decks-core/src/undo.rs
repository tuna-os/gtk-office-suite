// undo.rs — object geometry helpers (bounds, origin, rotation) and z-order.
// SPDX-License-Identifier: GPL-3.0-or-later
//


use crate::engine::SlideObject;

// ── Helper for offsetting object coordinates ───────────────────────────

pub(crate) fn offset_object(obj: &mut SlideObject, dx: f64, dy: f64) {
    match obj {
        SlideObject::TextBox { x, y, .. }
        | SlideObject::Rect { x, y, .. }
        | SlideObject::Shape { x, y, .. }
        | SlideObject::Table { x, y, .. }
        | SlideObject::Image { x, y, .. }
        | SlideObject::Circle { x, y, .. } => {
            *x += dx;
            *y += dy;
        }
    }
}

/// Set absolute position of any object.
pub fn set_obj_position(obj: &mut SlideObject, nx: f64, ny: f64) {
    match obj {
        SlideObject::TextBox { x, y, .. }
        | SlideObject::Rect { x, y, .. }
        | SlideObject::Shape { x, y, .. }
        | SlideObject::Table { x, y, .. }
        | SlideObject::Image { x, y, .. }
        | SlideObject::Circle { x, y, .. } => {
            *x = nx;
            *y = ny;
        }
    }
}

/// Get position of any object as (x, y).
pub fn obj_position(obj: &SlideObject) -> (f64, f64) {
    match obj {
        SlideObject::TextBox { x, y, .. }
        | SlideObject::Rect { x, y, .. }
        | SlideObject::Shape { x, y, .. }
        | SlideObject::Table { x, y, .. }
        | SlideObject::Image { x, y, .. }
        | SlideObject::Circle { x, y, .. } => (*x, *y),
    }
}

/// Get rotation angle in degrees of any object.
pub fn obj_rotation(obj: &SlideObject) -> f64 {
    match obj {
        SlideObject::TextBox { rotation, .. }
        | SlideObject::Rect { rotation, .. }
        | SlideObject::Shape { rotation, .. }
        | SlideObject::Table { rotation, .. }
        | SlideObject::Circle { rotation, .. }
        | SlideObject::Image { rotation, .. } => *rotation,
    }
}

/// Set rotation angle in degrees of any object.
pub fn set_obj_rotation(obj: &mut SlideObject, angle: f64) {
    match obj {
        SlideObject::TextBox { rotation, .. }
        | SlideObject::Rect { rotation, .. }
        | SlideObject::Shape { rotation, .. }
        | SlideObject::Table { rotation, .. }
        | SlideObject::Circle { rotation, .. }
        | SlideObject::Image { rotation, .. } => *rotation = angle,
    }
}

/// Get bounding rectangle of any object as (x, y, w, h).
pub fn obj_bounds(obj: &SlideObject) -> (f64, f64, f64, f64) {
    match obj {
        SlideObject::TextBox { x, y, w, h, .. }
        | SlideObject::Rect { x, y, w, h, .. }
        | SlideObject::Shape { x, y, w, h, .. }
        | SlideObject::Table { x, y, w, h, .. }
        | SlideObject::Image { x, y, w, h, .. } => (*x, *y, *w, *h),
        SlideObject::Circle { x, y, r, .. } => (*x - *r, *y - *r, *r * 2.0, *r * 2.0),
    }
}

/// Move an object so the top-left of its *bounding box* lands at
/// (nx, ny), leaving its size alone.
///
/// A circle stores its centre while every other object stores its
/// corner, which is why `obj_bounds` and `set_obj_bounds` both convert.
/// Align and distribute work entirely in bounding-box space and used to
/// write those coordinates straight into the raw field with
/// `set_obj_position`, which moved every circle up and left by its
/// radius — on apply, and again on each undo, so repeated align/undo
/// cycles walked it off the slide. Found by the seeded command
/// sequences in decks-core/tests/stateful.rs (#442).
pub fn set_obj_origin(obj: &mut SlideObject, nx: f64, ny: f64) {
    match obj {
        SlideObject::Circle { x, y, r, .. } => {
            *x = nx + *r;
            *y = ny + *r;
        }
        _ => set_obj_position(obj, nx, ny),
    }
}

/// Set bounding rectangle of any object (x, y, w, h).
pub fn set_obj_bounds(obj: &mut SlideObject, nx: f64, ny: f64, nw: f64, nh: f64) {
    match obj {
        SlideObject::TextBox { x, y, w, h, .. }
        | SlideObject::Rect { x, y, w, h, .. }
        | SlideObject::Shape { x, y, w, h, .. }
        | SlideObject::Table { x, y, w, h, .. }
        | SlideObject::Image { x, y, w, h, .. } => {
            *x = nx;
            *y = ny;
            *w = nw.max(10.0);
            *h = nh.max(10.0);
        }
        SlideObject::Circle { x, y, r, .. } => {
            *x = nx + nw / 2.0;
            *y = ny + nh / 2.0;
            *r = (nw.min(nh) / 2.0).max(5.0);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignMode {
    Left,
    Center,
    Right,
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistributeMode {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZOrderOp {
    BringToFront,
    SendToBack,
    BringForward,
    SendBackward,
}

/// Where object `index` of `len` ends up after `op`, as
/// `DecksController::z_order_object` moves it; the selection follows it.
pub fn z_order_index(index: usize, len: usize, op: ZOrderOp) -> usize {
    let last = len.saturating_sub(1);
    match op {
        ZOrderOp::BringToFront => last,
        ZOrderOp::SendToBack => 0,
        ZOrderOp::BringForward => (index + 1).min(last),
        ZOrderOp::SendBackward => index.saturating_sub(1),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_selection_follows_a_z_order_move() {
        assert_eq!(z_order_index(1, 4, ZOrderOp::BringToFront), 3);
        assert_eq!(z_order_index(1, 4, ZOrderOp::SendToBack), 0);
        assert_eq!(z_order_index(3, 4, ZOrderOp::BringForward), 3);
        assert_eq!(z_order_index(0, 4, ZOrderOp::SendBackward), 0);
        assert_eq!(z_order_index(2, 4, ZOrderOp::SendBackward), 1);
    }

    use super::*;
}
