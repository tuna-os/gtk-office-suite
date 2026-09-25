// SPDX-License-Identifier: GPL-3.0-or-later
//! Cell notes (Excel's notes, Calc's comments): a red corner marks a cell
//! with one, resting the pointer on it shows it, and Edit Note (Shift+F2,
//! as in Excel; Ctrl+Alt+C, as in Calc) opens it for editing in a popover
//! at the cell. Closing the popover keeps what was typed, as in Calc; an
//! emptied note is removed. Each change is one undo step. The model and
//! the file formats are tables-core's; this wires GTK.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk4::{self as gtk, glib, prelude::*};
use tables_core::controller::WorkbookController;
use tables_core::sheet::{col_label, xy_to_cell};

type Ctl = Rc<RefCell<WorkbookController>>;

/// How long the pointer rests on a cell before its note shows.
const HOVER_DELAY: Duration = Duration::from_millis(400);

/// The corner marker's size, in px: a red triangle in the cell's top-right
/// corner, as Excel and Calc draw it.
pub const MARKER: f64 = 6.0;

/// Draw the note marker for a cell at `(x, y, w)` (its top edge).
pub fn draw_marker(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64) {
    let right = (x + w).round() - 1.0;
    let top = y.round() + 1.0;
    cr.set_source_rgb(0.86, 0.12, 0.12);
    cr.move_to(right - MARKER, top);
    cr.line_to(right, top);
    cr.line_to(right, top + MARKER);
    cr.close_path();
    cr.fill().unwrap();
}

/// The cell's rectangle in `host`'s coordinates, for pointing a popover.
fn cell_rect(ctl: &Ctl, grid: &gtk::DrawingArea, host: &gtk::Widget, h: f64, v: f64, row: usize, col: usize) -> gtk::gdk::Rectangle {
    let (x, y, w, ht) = {
        let c = ctl.borrow();
        let state = c.state.borrow();
        let s = state.sheet();
        (tables_core::sheet::col_x(col, h, &s), tables_core::sheet::row_y(row, v, &s), s.col_width(col), s.row_height(row))
    };
    let origin = grid
        .compute_point(host, &gtk::graphene::Point::new(x as f32, y as f32))
        .map_or((x as i32, y as i32), |p| (p.x() as i32, p.y() as i32));
    gtk::gdk::Rectangle::new(origin.0, origin.1, w as i32, ht as i32)
}

fn note_at(ctl: &Ctl, row: usize, col: usize) -> Option<String> {
    let c = ctl.borrow();
    let state = c.state.borrow();
    let s = state.sheet();
    s.notes.get(row).and_then(|r| r.get(col)).cloned().flatten()
}

/// Popovers go beside the grid, not on it: the grid's accessible children
/// are its cells (grid_area.rs), so a popover inside it would be invisible
/// to screen readers.
fn host_of(grid: &gtk::DrawingArea) -> gtk::Widget {
    grid.parent().unwrap_or_else(|| grid.clone().upcast())
}

fn unparent_when_closed(popover: &gtk::Popover) {
    popover.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
}

/// Open the note editor for the active cell.
fn edit(ctl: &Ctl, grid: &gtk::DrawingArea, h: &gtk::Adjustment, v: &gtk::Adjustment, refresh: &Rc<dyn Fn()>) {
    let (row, col) = {
        let c = ctl.borrow();
        let state = c.state.borrow();
        let s = state.sheet();
        (s.selected_row, s.selected_col)
    };
    let existing = note_at(ctl, row, col);
    let host = host_of(grid);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.set_margin_top(6);
    content.set_margin_bottom(6);
    content.set_margin_start(6);
    content.set_margin_end(6);
    let title = gtk::Label::new(Some(&format!("{} {}{}", suite_common::i18n("Note on"), col_label(col), row + 1)));
    title.add_css_class("heading");
    title.set_xalign(0.0);
    content.append(&title);
    let text = gtk::TextView::builder().wrap_mode(gtk::WrapMode::WordChar).accepts_tab(false).build();
    text.buffer().set_text(existing.as_deref().unwrap_or(""));
    text.update_property(&[gtk::accessible::Property::Label(&suite_common::i18n("Note"))]);
    let scroller = gtk::ScrolledWindow::builder()
        .child(&text)
        .min_content_width(240)
        .min_content_height(96)
        .max_content_height(240)
        .propagate_natural_height(true)
        .build();
    scroller.add_css_class("card");
    content.append(&scroller);

    let popover = gtk::Popover::builder().child(&content).position(gtk::PositionType::Right).build();
    popover.set_parent(&host);
    popover.set_pointing_to(Some(&cell_rect(ctl, grid, &host, h.value(), v.value(), row, col)));

    // Delete, for a note that exists: the popover then closes, and nothing
    // is kept.
    let deleted = Rc::new(Cell::new(false));
    if existing.is_some() {
        let delete = gtk::Button::with_label(&suite_common::i18n("Delete Note"));
        delete.add_css_class("destructive-action");
        delete.set_halign(gtk::Align::End);
        let (deleted, popover) = (deleted.clone(), popover.clone());
        delete.connect_clicked(move |_| {
            deleted.set(true);
            popover.popdown();
        });
        content.append(&delete);
    }

    {
        let (ctl, grid, refresh, text) = (ctl.clone(), grid.clone(), refresh.clone(), text.clone());
        popover.connect_closed(move |_| {
            let b = text.buffer();
            let typed = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
            let note = (!deleted.get()).then_some(typed);
            if ctl.borrow_mut().set_note(row, col, note) {
                refresh();
                grid.queue_draw();
            }
            grid.grab_focus();
        });
    }
    unparent_when_closed(&popover);
    popover.popup();
    text.grab_focus();
}

/// Wire notes onto the grid: the hover popover and the Edit Note action.
pub fn attach(
    app: &gtk::Application,
    ctl: &Ctl,
    grid: &gtk::DrawingArea,
    h: &gtk::Adjustment,
    v: &gtk::Adjustment,
    refresh: &Rc<dyn Fn()>,
) {
    // Hover: after the pointer rests on a noted cell, show its note beside
    // it; leaving the cell hides it.
    let shown: Rc<RefCell<Option<(usize, usize, gtk::Popover)>>> = Rc::new(RefCell::new(None));
    let pending: Rc<Cell<Option<(usize, usize)>>> = Rc::new(Cell::new(None));
    let hide = {
        let shown = shown.clone();
        move || {
            if let Some((_, _, p)) = shown.borrow_mut().take() {
                p.popdown();
            }
        }
    };
    let motion = gtk::EventControllerMotion::new();
    {
        let (ctl, grid, h, v, shown, pending, hide) = (ctl.clone(), grid.clone(), h.clone(), v.clone(), shown.clone(), pending.clone(), hide.clone());
        motion.connect_motion(move |_, x, y| {
            let cell = {
                let c = ctl.borrow();
                let state = c.state.borrow();
                let s = state.sheet();
                xy_to_cell(x, y, h.value(), v.value(), &s).map(|(col, row)| (row, col))
            };
            if shown.borrow().as_ref().map(|s| (s.0, s.1)) == cell && cell.is_some() {
                return;
            }
            hide();
            pending.set(None);
            let Some((row, col)) = cell else { return };
            if note_at(&ctl, row, col).is_none() {
                return;
            }
            pending.set(Some((row, col)));
            let (ctl, grid, h, v, shown, pending) = (ctl.clone(), grid.clone(), h.clone(), v.clone(), shown.clone(), pending.clone());
            glib::timeout_add_local_once(HOVER_DELAY, move || {
                if pending.get() != Some((row, col)) {
                    return;
                }
                let Some(note) = note_at(&ctl, row, col) else { return };
                let host = host_of(&grid);
                let label = gtk::Label::builder()
                    .label(&note)
                    .wrap(true)
                    .max_width_chars(40)
                    .xalign(0.0)
                    .margin_top(6)
                    .margin_bottom(6)
                    .margin_start(6)
                    .margin_end(6)
                    .build();
                let popover = gtk::Popover::builder()
                    .child(&label)
                    .position(gtk::PositionType::Right)
                    .autohide(false)
                    .can_focus(false)
                    .has_arrow(true)
                    .build();
                popover.set_parent(&host);
                popover.set_pointing_to(Some(&cell_rect(&ctl, &grid, &host, h.value(), v.value(), row, col)));
                unparent_when_closed(&popover);
                popover.popup();
                *shown.borrow_mut() = Some((row, col, popover));
            });
        });
    }
    {
        let (pending, hide) = (pending.clone(), hide.clone());
        motion.connect_leave(move |_| {
            pending.set(None);
            hide();
        });
    }
    grid.add_controller(motion);

    let act = gtk::gio::SimpleAction::new("edit-note", None);
    {
        let (ctl, grid, h, v, refresh) = (ctl.clone(), grid.clone(), h.clone(), v.clone(), refresh.clone());
        act.connect_activate(move |_, _| {
            hide();
            edit(&ctl, &grid, &h, &v, &refresh);
        });
    }
    app.add_action(&act);
    app.set_accels_for_action("app.edit-note", &["<Shift>F2", "<Primary><Alt>c"]);
    suite_common::actions::register_labels(&[("app.edit-note", &suite_common::i18n("Edit Note…"))]);
}
