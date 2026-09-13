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
use decks_core::undo::set_obj_position;
use decks_core::engine::Slide;

/// Install the drag gestures that move, resize and rotate the selected
/// object. Call once during window construction.
pub(crate) fn register_canvas_pointer_input(
    canvas: &gtk::DrawingArea,
    slides: &Rc<RefCell<Vec<Slide>>>,
    current_slide: &Rc<Cell<usize>>,
    selected_object: &Rc<Cell<Option<usize>>>,
    controller: &Rc<DecksController>,
    refresh_hud: &Rc<dyn Fn()>,
    snap_enabled: &Rc<Cell<bool>>,
) {
        let ss = slides.clone();
        let cs = canvas.clone();
        let cs_ref = current_slide.clone();
        let so = selected_object.clone();
        let controller = controller.clone();
        let refresh = refresh_hud.clone();

        #[derive(Clone, Copy)]
        enum DragMode {
            Move { oi: usize, orig_x: f64, orig_y: f64 },
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
                let (ox, oy) = decks_core::undo::obj_position(&sl[idx].objects[oi]);
                so_b.set(Some(oi));
                dm_begin.set(Some(DragMode::Move { oi, orig_x: ox, orig_y: oy }));
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
                    DragMode::Move { oi, orig_x, orig_y } => {
                        if oi < sl[idx].objects.len() {
                            let (raw_x, raw_y) = (orig_x + dx * k, orig_y + dy * k);
                            let (nx, ny) = if snap_u.get() {
                                (snap_to_grid(raw_x, GRID_SPACING), snap_to_grid(raw_y, GRID_SPACING))
                            } else {
                                (raw_x, raw_y)
                            };
                            set_obj_position(&mut sl[idx].objects[oi], nx, ny);
                            cs_u.queue_draw();
                        }
                    }
                    DragMode::Resize { oi, handle, orig_bounds: (ox, oy, ow, oh) } => {
                        if oi < sl[idx].objects.len() {
                            use crate::canvas::SelectionHandle;
                            let (mut nx, mut ny, mut nw, mut nh) = (ox, oy, ow, oh);
                            let (sdx, sdy) = (dx * k, dy * k);
                            match handle {
                                SelectionHandle::TopLeft => { nx += sdx; ny += sdy; nw -= sdx; nh -= sdy; }
                                SelectionHandle::TopCenter => { ny += sdy; nh -= sdy; }
                                SelectionHandle::TopRight => { ny += sdy; nw += sdx; nh -= sdy; }
                                SelectionHandle::RightCenter => { nw += sdx; }
                                SelectionHandle::BottomRight => { nw += sdx; nh += sdy; }
                                SelectionHandle::BottomCenter => { nh += sdy; }
                                SelectionHandle::BottomLeft => { nx += sdx; nw -= sdx; nh += sdy; }
                                SelectionHandle::LeftCenter => { nx += sdx; nw -= sdx; }
                                SelectionHandle::Rotate => {}
                            }
                            if snap_u.get() {
                                nx = snap_to_grid(nx, GRID_SPACING);
                                ny = snap_to_grid(ny, GRID_SPACING);
                                nw = snap_to_grid(nw, GRID_SPACING);
                                nh = snap_to_grid(nh, GRID_SPACING);
                            }
                            decks_core::undo::set_obj_bounds(&mut sl[idx].objects[oi], nx, ny, nw, nh);
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
                    DragMode::Move { oi, orig_x, orig_y } => {
                        let (raw_x, raw_y) = (orig_x + dx * k, orig_y + dy * k);
                        let (snapped_x, snapped_y) = if snap_e.get() {
                            (snap_to_grid(raw_x, GRID_SPACING), snap_to_grid(raw_y, GRID_SPACING))
                        } else {
                            (raw_x, raw_y)
                        };
                        let (net_dx, net_dy) = (snapped_x - orig_x, snapped_y - orig_y);
                        if net_dx != 0.0 || net_dy != 0.0 {
                            controller.move_object(idx, oi, net_dx, net_dy);
                        }
                    }
                    DragMode::Resize { oi, handle, orig_bounds: (ox, oy, ow, oh) } => {
                        use crate::canvas::SelectionHandle;
                        let (mut nx, mut ny, mut nw, mut nh) = (ox, oy, ow, oh);
                        let (sdx, sdy) = (dx * k, dy * k);
                        match handle {
                            SelectionHandle::TopLeft => { nx += sdx; ny += sdy; nw -= sdx; nh -= sdy; }
                            SelectionHandle::TopCenter => { ny += sdy; nh -= sdy; }
                            SelectionHandle::TopRight => { ny += sdy; nw += sdx; nh -= sdy; }
                            SelectionHandle::RightCenter => { nw += sdx; }
                            SelectionHandle::BottomRight => { nw += sdx; nh += sdy; }
                            SelectionHandle::BottomCenter => { nh += sdy; }
                            SelectionHandle::BottomLeft => { nx += sdx; nw -= sdx; nh += sdy; }
                            SelectionHandle::LeftCenter => { nx += sdx; nw -= sdx; }
                            SelectionHandle::Rotate => {}
                        }
                        if snap_e.get() {
                            nx = snap_to_grid(nx, GRID_SPACING);
                            ny = snap_to_grid(ny, GRID_SPACING);
                            nw = snap_to_grid(nw, GRID_SPACING);
                            nh = snap_to_grid(nh, GRID_SPACING);
                        }
                        let new_bounds = (nx, ny, nw.max(10.0), nh.max(10.0));
                        if new_bounds != (ox, oy, ow, oh) {
                            controller.resize_object(idx, oi, (ox, oy, ow, oh), new_bounds);
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
        });
        canvas.add_controller(drag);
}
