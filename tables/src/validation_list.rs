// SPDX-License-Identifier: GPL-3.0-or-later
//! List validation's dropdown: the active cell, when its validation is a
//! list, shows an arrow at its right edge, as in Excel and Calc. Clicking
//! the arrow, or Alt+Down (which otherwise opens the column menu), opens
//! the list in a popover at the cell; choosing an item puts it in the cell
//! as one undo step. The rule and its file formats are tables-core's.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::{self as gtk, prelude::*};
use tables_core::controller::WorkbookController;
use tables_core::sheet::{col_x, row_y, ValidationRule};

type Ctl = Rc<RefCell<WorkbookController>>;

/// The arrow button's width, in px.
pub const ARROW_W: f64 = 16.0;

/// The list items of the active cell, if its validation is a list.
fn active_list(ctl: &Ctl) -> Option<(usize, usize, Vec<String>)> {
    let c = ctl.borrow();
    let state = c.state.borrow();
    let s = state.sheet();
    let (r, col) = (s.selected_row, s.selected_col);
    match s.validations.get(r)?.get(col)? {
        Some(ValidationRule::List(items)) if !items.is_empty() => Some((r, col, items.clone())),
        _ => None,
    }
}

/// Draw the arrow on a cell at `(x, y, w, h)`.
pub fn draw_arrow(cr: &gtk::cairo::Context, (x, y, w, h): (f64, f64, f64, f64), dark: bool) {
    let left = (x + w - ARROW_W).round();
    let (top, bottom) = (y.round() + 1.0, (y + h).round() - 1.0);
    let (bg, fg) = if dark { ((0.25, 0.25, 0.27), (0.9, 0.9, 0.9)) } else { ((0.93, 0.93, 0.93), (0.2, 0.2, 0.2)) };
    cr.set_source_rgb(bg.0, bg.1, bg.2);
    cr.rectangle(left, top, ARROW_W - 1.0, bottom - top);
    cr.fill().unwrap();
    let (cx, cy) = (left + ARROW_W / 2.0 - 0.5, (top + bottom) / 2.0);
    cr.set_source_rgb(fg.0, fg.1, fg.2);
    cr.move_to(cx - 4.0, cy - 2.0);
    cr.line_to(cx + 4.0, cy - 2.0);
    cr.line_to(cx, cy + 2.5);
    cr.close_path();
    cr.fill().unwrap();
}

/// Whether the active cell shows the arrow (it has a list).
pub fn shows_arrow(sheet: &tables_core::sheet::SheetModel, row: usize, col: usize) -> bool {
    matches!(sheet.validations.get(row).and_then(|r| r.get(col)), Some(Some(ValidationRule::List(items))) if !items.is_empty())
}

/// Open the active cell's list, if it has one. False if it hasn't.
pub fn open(ctl: &Ctl, grid: &gtk::DrawingArea, h: &gtk::Adjustment, v: &gtk::Adjustment, refresh: &Rc<dyn Fn()>) -> bool {
    let Some((row, col, items)) = active_list(ctl) else { return false };
    let (rect, current) = {
        let c = ctl.borrow();
        let state = c.state.borrow();
        let s = state.sheet();
        let r = (col_x(col, h.value(), &s), row_y(row, v.value(), &s), s.col_width(col), s.row_height(row));
        (r, state.cell_input(row, col))
    };
    // Beside the grid, not on it: the grid's accessible children are its
    // cells, so a popover inside it would be invisible to screen readers.
    let host: gtk::Widget = grid.parent().unwrap_or_else(|| grid.clone().upcast());
    let origin = grid
        .compute_point(&host, &gtk::graphene::Point::new(rect.0 as f32, rect.1 as f32))
        .map_or((rect.0 as i32, rect.1 as i32), |p| (p.x() as i32, p.y() as i32));

    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let popover = gtk::Popover::builder().child(&gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(320)
        .build())
        .position(gtk::PositionType::Bottom)
        .build();
    popover.add_css_class("menu");
    popover.set_parent(&host);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(origin.0, origin.1, rect.2 as i32, rect.3 as i32)));
    popover.connect_closed(|p| {
        let p = p.clone();
        gtk::glib::idle_add_local_once(move || p.unparent());
    });

    let mut first_button = None;
    for item in items {
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let check = gtk::Image::from_icon_name("object-select-symbolic");
        check.set_opacity(if item == current { 1.0 } else { 0.0 });
        row_box.append(&check);
        let label = gtk::Label::new(Some(&item));
        label.set_xalign(0.0);
        row_box.append(&label);
        let button = gtk::Button::builder().child(&row_box).build();
        button.add_css_class("flat");
        button.update_property(&[gtk::accessible::Property::Label(&item)]);
        if item == current {
            button.update_state(&[gtk::accessible::State::Checked(gtk::AccessibleTristate::True)]);
        }
        let is_current = item == current;
        let (ctl, grid, refresh, popover) = (ctl.clone(), grid.clone(), refresh.clone(), popover.clone());
        button.connect_clicked(move |_| {
            popover.popdown();
            ctl.borrow_mut().edit_cell(row, col, item.clone());
            refresh();
            grid.queue_draw();
            grid.grab_focus();
        });
        if is_current || first_button.is_none() {
            first_button = Some(button.clone());
        }
        list.append(&button);
    }
    popover.popup();
    if let Some(b) = first_button {
        b.grab_focus();
    }
    true
}

/// Clicking the active cell's arrow opens its list.
pub fn attach(ctl: &Ctl, grid: &gtk::DrawingArea, h: &gtk::Adjustment, v: &gtk::Adjustment, refresh: &Rc<dyn Fn()>) {
    let click = gtk::GestureClick::new();
    click.set_button(1);
    // Ahead of the grid's own click, which would otherwise (re)select the
    // cell under the arrow and start a drag.
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let (ctl2, grid2, h2, v2, refresh2) = (ctl.clone(), grid.clone(), h.clone(), v.clone(), refresh.clone());
    click.connect_pressed(move |g, _, x, y| {
        let (ctl, h, v) = (&ctl2, &h2, &v2);
        let on_arrow = {
            let c = ctl.borrow();
            let state = c.state.borrow();
            let s = state.sheet();
            let (r, col) = (s.selected_row, s.selected_col);
            if !shows_arrow(&s, r, col) {
                return;
            }
            let right = col_x(col, h.value(), &s) + s.col_width(col);
            let top = row_y(r, v.value(), &s);
            x >= right - ARROW_W && x <= right && y >= top && y <= top + s.row_height(r)
        };
        if on_arrow {
            g.set_state(gtk::EventSequenceState::Claimed);
            open(ctl, &grid2, h, v, &refresh2);
        }
    });
    grid.add_controller(click);

    // Alt+Down on a cell with a list opens it; the column menu leaves
    // such a cell to this.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let (ctl, grid2, h, v, refresh) = (ctl.clone(), grid.clone(), h.clone(), v.clone(), refresh.clone());
    keys.connect_key_pressed(move |_, key, _, mods| {
        if key == gtk::gdk::Key::Down && mods.contains(gtk::gdk::ModifierType::ALT_MASK) && open(&ctl, &grid2, &h, &v, &refresh) {
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    grid.add_controller(keys);
}
