// column_menu.rs — the column header menu: sort and filter a column.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// docs/DESIGN-UI.md (Numbers and Sheets): right-clicking a column header,
// or Alt+Down on a cell, opens a popover on that column's header with
// Sort Ascending / Sort Descending and a checklist of the column's values
// (Sheets' filter by values): unticking a value hides its rows, ticking it
// shows them again. Each change is one undo step. The sorting, the value
// list and the filtering are tables-core's (column_values,
// sort_column, filter_column_values); this only wires GTK.

use gtk4::{self as gtk, prelude::*};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use tables_core::controller::WorkbookController;
use tables_core::sheet::{col_label, SortDirection, COL_HEADER_HEIGHT};

/// Most values listed; a column with more is still sortable.
const MAX_VALUES: usize = 200;

type Ctl = Rc<RefCell<WorkbookController>>;

fn menu_button(icon: Option<&str>, label: &str) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    if let Some(icon) = icon {
        content.append(&gtk::Image::from_icon_name(icon));
    }
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    content.append(&text);
    let b = gtk::Button::builder().child(&content).build();
    b.add_css_class("flat");
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    b
}

/// Open the menu for column `col` under its header.
fn open(ctl: &Ctl, grid: &gtk::DrawingArea, h: &gtk::Adjustment, refresh: &Rc<dyn Fn()>, col: usize) {
    let (values, x, w) = {
        let c = ctl.borrow();
        let state = c.state.borrow();
        let sheet = state.sheet();
        if col >= sheet.cols {
            return;
        }
        (sheet.column_values(col), tables_core::sheet::col_x(col, h.value(), &sheet), sheet.col_width(col))
    };

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let title = gtk::Label::new(Some(&format!("Column {}", col_label(col))));
    title.add_css_class("heading");
    title.set_xalign(0.0);
    title.set_margin_start(12);
    title.set_margin_top(6);
    title.set_margin_bottom(6);
    content.append(&title);

    let popover = gtk::Popover::builder().child(&content).position(gtk::PositionType::Bottom).build();
    popover.set_parent(grid);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, 0, w as i32, COL_HEADER_HEIGHT as i32)));
    popover.connect_closed(|p| {
        let p = p.clone();
        // Unparent after the close animation has used it.
        gtk::glib::idle_add_local_once(move || p.unparent());
    });

    for (icon, label, direction) in [
        ("view-sort-ascending-symbolic", "Sort Ascending", SortDirection::Ascending),
        ("view-sort-descending-symbolic", "Sort Descending", SortDirection::Descending),
    ] {
        let b = menu_button(Some(icon), label);
        let (ctl, grid, refresh, popover) = (ctl.clone(), grid.clone(), refresh.clone(), popover.clone());
        b.connect_clicked(move |_| {
            popover.popdown();
            ctl.borrow_mut().sort_column(col, direction);
            refresh();
            grid.queue_draw();
        });
        content.append(&b);
    }

    if !values.is_empty() {
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let heading = gtk::Label::new(Some("Show Rows With"));
        heading.add_css_class("caption-heading");
        heading.add_css_class("dim-label");
        heading.set_xalign(0.0);
        heading.set_margin_start(12);
        heading.set_margin_top(6);
        content.append(&heading);
        let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        list.set_margin_start(6);
        list.set_margin_end(6);
        // What is hidden now, kept as the checklist changes.
        let hidden: Rc<RefCell<HashSet<String>>> =
            Rc::new(RefCell::new(values.iter().filter(|v| !v.shown).map(|v| v.text.clone()).collect()));
        for v in values.iter().take(MAX_VALUES) {
            let shown_as = if v.text.is_empty() { "(Blank)".to_string() } else { v.text.clone() };
            let check = gtk::CheckButton::with_label(&format!("{shown_as}  ({})", v.rows));
            check.set_active(v.shown);
            check.update_property(&[gtk::accessible::Property::Label(&shown_as)]);
            let (ctl, grid, refresh, hidden, text) = (ctl.clone(), grid.clone(), refresh.clone(), hidden.clone(), v.text.clone());
            check.connect_toggled(move |c| {
                {
                    let mut h = hidden.borrow_mut();
                    if c.is_active() {
                        h.remove(&text);
                    } else {
                        h.insert(text.clone());
                    }
                }
                ctl.borrow_mut().filter_column_values(col, &hidden.borrow());
                refresh();
                grid.queue_draw();
            });
            list.append(&check);
        }
        let scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(280)
            .build();
        content.append(&scroll);
        let clear = menu_button(None, "Clear Filter");
        let (ctl, grid, refresh, popover) = (ctl.clone(), grid.clone(), refresh.clone(), popover.clone());
        clear.connect_clicked(move |_| {
            popover.popdown();
            ctl.borrow_mut().clear_filter();
            refresh();
            grid.queue_draw();
        });
        content.append(&clear);
    }
    popover.popup();
}

/// Wire the column menu onto the grid: a right-click on a column header,
/// or Alt+Down for the active cell's column.
pub fn attach(ctl: &Ctl, grid: &gtk::DrawingArea, h: &gtk::Adjustment, refresh: &Rc<dyn Fn()>) {
    let right = gtk::GestureClick::new();
    right.set_button(3);
    // Ahead of the grid's own right-click (Format Cells), and claimed on
    // the header so that one doesn't run too.
    right.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let (ctl, grid, h, refresh) = (ctl.clone(), grid.clone(), h.clone(), refresh.clone());
        right.connect_pressed(move |g, _, x, y| {
            if y >= COL_HEADER_HEIGHT {
                return;
            }
            let col = {
                let c = ctl.borrow();
                let state = c.state.borrow();
                let sheet = state.sheet();
                tables_core::sheet::col_at(x, h.value(), &sheet)
            };
            if let Some(col) = col {
                g.set_state(gtk::EventSequenceState::Claimed);
                open(&ctl, &grid, &h, &refresh, col);
            }
        });
    }
    grid.add_controller(right);

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let (ctl, grid, h, refresh) = (ctl.clone(), grid.clone(), h.clone(), refresh.clone());
        keys.connect_key_pressed(move |_, key, _, mods| {
            if key == gtk::gdk::Key::Down && mods.contains(gtk::gdk::ModifierType::ALT_MASK) {
                let col = ctl.borrow().state.borrow().sheet().selected_col;
                open(&ctl, &grid, &h, &refresh, col);
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
    }
    grid.add_controller(keys);
}
