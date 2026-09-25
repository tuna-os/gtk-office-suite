//! Pointer interaction on the slide canvas: move, resize and rotate.
//!
//! Extracted from `DecksWindow::new`, which had grown to 1674 lines in one
//! function and pushed `window.rs` to exactly its 1800-line ceiling — the
//! next line added anywhere in the file would have failed the release
//! gate. This block was the largest self-contained piece: a braced scope,
//! so Rust already guaranteed nothing inside it escaped, and its only
//! inputs were the six handles it cloned at the top. Moving it is
//! therefore a relocation rather than a redesign, which is what you want
//! when the goal is headroom and the tests are the only safety net.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::{self as gtk, prelude::*};

use crate::canvas::{canvas_to_slide, hit_test_object, snap_to_grid, GRID_SPACING};
use decks_core::controller::DecksController;
use decks_core::engine::Slide;
use decks_core::guides::{snap_move, snap_resize, Edges, Guide, Rect};
use decks_core::undo::{obj_bounds, set_obj_origin};

/// How close, in canvas pixels, an edge has to come to snap to a guide.
const SNAP_PX: f64 = 6.0;

/// The bounding boxes of every object on the slide but `oi`.
fn others(slide: &Slide, oi: usize) -> Vec<Rect> {
    slide.objects.iter().enumerate().filter(|(i, _)| *i != oi).map(|(_, o)| obj_bounds(o)).collect()
}

/// Which edges a resize handle moves.
fn edges_of(handle: crate::canvas::SelectionHandle) -> Edges {
    use crate::canvas::SelectionHandle as H;
    let (left, right, top, bottom) = match handle {
        H::TopLeft => (true, false, true, false),
        H::TopCenter => (false, false, true, false),
        H::TopRight => (false, true, true, false),
        H::RightCenter => (false, true, false, false),
        H::BottomRight => (false, true, false, true),
        H::BottomCenter => (false, false, false, true),
        H::BottomLeft => (true, false, false, true),
        H::LeftCenter => (true, false, false, false),
        H::Rotate => (false, false, false, false),
    };
    Edges { left, right, top, bottom }
}

/// Where a move ends up: smart guides first (docs/DESIGN-UI.md), then
/// the grid on an axis no guide claimed, when the grid is on.
fn moved(orig: Rect, d: (f64, f64), others: &[Rect], tol: f64, grid: bool) -> (Rect, Vec<Guide>) {
    let raw = (orig.0 + d.0, orig.1 + d.1, orig.2, orig.3);
    let s = snap_move(raw, others, tol);
    let mut r = s.rect;
    if grid && !s.snapped.0 {
        r.0 = snap_to_grid(r.0, GRID_SPACING);
    }
    if grid && !s.snapped.1 {
        r.1 = snap_to_grid(r.1, GRID_SPACING);
    }
    (r, s.guides)
}

/// Where a resize ends up, by the same rules.
fn resized(
    handle: crate::canvas::SelectionHandle,
    orig: Rect,
    d: (f64, f64),
    others: &[Rect],
    tol: f64,
    grid: bool,
) -> (Rect, Vec<Guide>) {
    let e = edges_of(handle);
    let (mut x, mut y, mut w, mut h) = orig;
    if e.left {
        x += d.0;
        w -= d.0;
    }
    if e.right {
        w += d.0;
    }
    if e.top {
        y += d.1;
        h -= d.1;
    }
    if e.bottom {
        h += d.1;
    }
    let s = snap_resize((x, y, w, h), e, others, tol);
    let (mut x, mut y, mut w, mut h) = s.rect;
    if grid && !s.snapped.0 {
        (x, w) = (snap_to_grid(x, GRID_SPACING), snap_to_grid(w, GRID_SPACING));
    }
    if grid && !s.snapped.1 {
        (y, h) = (snap_to_grid(y, GRID_SPACING), snap_to_grid(h, GRID_SPACING));
    }
    ((x, y, w.max(10.0), h.max(10.0)), s.guides)
}

/// Install the drag gestures that move, resize and rotate the selected
/// object. Call once during window construction.
#[allow(clippy::too_many_arguments)]
pub(crate) fn register_canvas_pointer_input(
    canvas: &gtk::DrawingArea,
    slides: &Rc<RefCell<Vec<Slide>>>,
    current_slide: &Rc<Cell<usize>>,
    selected_object: &Rc<Cell<Option<usize>>>,
    controller: &Rc<DecksController>,
    refresh_hud: &Rc<dyn Fn()>,
    snap_enabled: &Rc<Cell<bool>>,
    guides: &Rc<RefCell<Vec<Guide>>>,
) {
        let ss = slides.clone();
        let cs = canvas.clone();
        let cs_ref = current_slide.clone();
        let so = selected_object.clone();
        let controller = controller.clone();
        let refresh = refresh_hud.clone();

        #[derive(Clone, Copy)]
        enum DragMode {
            Move { oi: usize, orig: Rect },
            Resize { oi: usize, handle: crate::canvas::SelectionHandle, orig_bounds: (f64, f64, f64, f64) },
            Rotate { oi: usize, orig_angle: f64 },
        }

        let drag_mode: Rc<Cell<Option<DragMode>>> = Rc::new(Cell::new(None));
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);

        let dm_begin = drag_mode.clone();
        let dm_update = drag_mode.clone();
        let dm_end = drag_mode.clone();

        let ss_b = ss.clone();
        let ss_u = ss.clone();
        let cs_b = cs.clone();
        let cs_u = cs.clone();
        let cs_e = cs.clone();
        let cs_ref_b = cs_ref.clone();
        let cs_ref_u = cs_ref.clone();
        let cs_ref_e = cs_ref.clone();
        let so_b = so.clone();
        let refresh_b = refresh.clone();
        let snap_u = snap_enabled.clone();
        let snap_e = snap_enabled.clone();
        let ss_e = ss.clone();
        let guides_u = guides.clone();
        let guides_e = guides.clone();

        drag.connect_drag_begin(move |_g, x, y| {
            let idx = cs_ref_b.get();
            let sl = ss_b.borrow();
            if idx >= sl.len() { return; }

            if let Some(oi) = so_b.get() {
                if oi < sl[idx].objects.len() {
                    if let Some(handle) = crate::canvas::hit_test_handles(
                        &sl[idx].objects[oi], x, y, cs_b.width() as f64, cs_b.height() as f64
                    ) {
                        if handle == crate::canvas::SelectionHandle::Rotate {
                            dm_begin.set(Some(DragMode::Rotate {
                                oi,
                                orig_angle: sl[idx].objects[oi].rotation(),
                            }));
                        } else {
                            dm_begin.set(Some(DragMode::Resize {
                                oi,
                                handle,
                                orig_bounds: decks_core::undo::obj_bounds(&sl[idx].objects[oi]),
                            }));
                        }
                        return;
                    }
                }
            }

            let (sx, sy) = canvas_to_slide(x, y, cs_b.width() as f64, cs_b.height() as f64);
            if let Some(oi) = hit_test_object(&sl[idx].objects, sx, sy) {
                so_b.set(Some(oi));
                dm_begin.set(Some(DragMode::Move { oi, orig: obj_bounds(&sl[idx].objects[oi]) }));
            } else {
                so_b.set(None);
                dm_begin.set(None);
            }
            drop(sl);
            cs_b.queue_draw();
            refresh_b();
        });

        drag.connect_drag_update(move |_g, dx, dy| {
            let idx = cs_ref_u.get();
            let mut sl = ss_u.borrow_mut();
            if idx >= sl.len() { return; }

            let (_, _, sw, _) = crate::canvas::slide_geometry(cs_u.width() as f64, cs_u.height() as f64);
            let k = 960.0 / sw.max(1.0);

            if let Some(mode) = dm_update.get() {
                match mode {
                    DragMode::Move { oi, orig } => {
                        if oi < sl[idx].objects.len() {
                            let others = others(&sl[idx], oi);
                            let (r, g) = moved(orig, (dx * k, dy * k), &others, SNAP_PX * k, snap_u.get());
                            set_obj_origin(&mut sl[idx].objects[oi], r.0, r.1);
                            *guides_u.borrow_mut() = g;
                            cs_u.queue_draw();
                        }
                    }
                    DragMode::Resize { oi, handle, orig_bounds } => {
                        if oi < sl[idx].objects.len() {
                            let others = others(&sl[idx], oi);
                            let (r, g) = resized(handle, orig_bounds, (dx * k, dy * k), &others, SNAP_PX * k, snap_u.get());
                            decks_core::undo::set_obj_bounds(&mut sl[idx].objects[oi], r.0, r.1, r.2, r.3);
                            *guides_u.borrow_mut() = g;
                            cs_u.queue_draw();
                        }
                    }
                    DragMode::Rotate { oi, orig_angle } => {
                        if oi < sl[idx].objects.len() {
                            let angle_delta = (dx + dy) * 0.5;
                            let mut new_angle = (orig_angle + angle_delta) % 360.0;
                            if new_angle < 0.0 { new_angle += 360.0; }
                            if snap_u.get() {
                                new_angle = (new_angle / 15.0).round() * 15.0;
                            }
                            decks_core::undo::set_obj_rotation(&mut sl[idx].objects[oi], new_angle);
                            cs_u.queue_draw();
                        }
                    }
                }
            }
        });

        drag.connect_drag_end(move |_g, dx, dy| {
            let idx = cs_ref_e.get();
            let (_, _, sw, _) = crate::canvas::slide_geometry(cs_e.width() as f64, cs_e.height() as f64);
            let k = 960.0 / sw.max(1.0);

            if let Some(mode) = dm_end.get() {
                match mode {
                    DragMode::Move { oi, orig } => {
                        let others = ss_e.borrow().get(idx).map(|s| others(s, oi)).unwrap_or_default();
                        let (r, _) = moved(orig, (dx * k, dy * k), &others, SNAP_PX * k, snap_e.get());
                        let (net_dx, net_dy) = (r.0 - orig.0, r.1 - orig.1);
                        if net_dx != 0.0 || net_dy != 0.0 {
                            // The drag moved the object live; put it back so
                            // the command's move is the one on record.
                            if let Some(o) = ss_e.borrow_mut().get_mut(idx).and_then(|s| s.objects.get_mut(oi)) {
                                set_obj_origin(o, orig.0, orig.1);
                            }
                            controller.move_object(idx, oi, net_dx, net_dy);
                        }
                    }
                    DragMode::Resize { oi, handle, orig_bounds } => {
                        let others = ss_e.borrow().get(idx).map(|s| others(s, oi)).unwrap_or_default();
                        let (new_bounds, _) = resized(handle, orig_bounds, (dx * k, dy * k), &others, SNAP_PX * k, snap_e.get());
                        if new_bounds != orig_bounds {
                            controller.resize_object(idx, oi, orig_bounds, new_bounds);
                        }
                    }
                    DragMode::Rotate { oi, orig_angle } => {
                        let angle_delta = (dx + dy) * 0.5;
                        let mut new_angle = (orig_angle + angle_delta) % 360.0;
                        if new_angle < 0.0 { new_angle += 360.0; }
                        if snap_e.get() {
                            new_angle = (new_angle / 15.0).round() * 15.0;
                        }
                        if new_angle != orig_angle {
                            controller.rotate_object(idx, oi, orig_angle, new_angle);
                        }
                    }
                }
            }
            dm_end.set(None);
            guides_e.borrow_mut().clear();
            cs_e.queue_draw();
        });
        canvas.add_controller(drag);
}
