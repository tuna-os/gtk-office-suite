//! The chart dialog: pick a type, see it drawn from the selected column,
//! insert it onto the sheet.
//!
//! Lifted out of `TablesWindow::new`, which had grown to 2167 lines in a
//! 2343-line file and crossed the 2300-line ceiling
//! `scripts/release_gate.py` keeps on it. The ceiling is there to make
//! exactly that growth visible, so the module moved rather than the
//! ceiling. This is the seam with the fewest ties to the rest of the
//! constructor: the dialog needs the parent window, the workbook state to
//! read the selected column from, and the controller to record an
//! insertion through — nothing else in `new`'s several hundred locals.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::{self as gtk, prelude::*};
use libadwaita as adw;
use adw::prelude::AdwDialogExt;

use tables_core::controller::{WorkbookController, WorkbookState};

/// Returns the callback the `insert-chart` action runs.
///
/// `parent` is shared rather than cloned because the window does not exist
/// yet when the constructor wires this up: it is filled in later, and the
/// dialog reads it at the moment it is presented.
pub fn opener(
    parent: Rc<RefCell<Option<adw::ApplicationWindow>>>,
    state: Rc<RefCell<WorkbookState>>,
    controller: Rc<RefCell<WorkbookController>>,
) -> Box<dyn Fn()> {
    Box::new(move || {
        let st = state.borrow();
        let active = st.active_sheet;
        let sheet = st.sheets[active].borrow();
        let col = sheet.selected_col;
        let mut data = Vec::new();
        for r in 0..sheet.rows {
            let label = sheet.data[r][0].clone();
            let val_str = &sheet.data[r][col];
            if let Ok(val) = val_str.parse::<f64>() {
                let lbl = if label.is_empty() { format!("Row {}", r + 1) } else { label };
                data.push((lbl, val));
            }
        }
        if data.is_empty() { return; }

        let dialog = adw::Dialog::builder()
            .title(suite_common::i18n("Chart"))
            .content_width(600)
            .content_height(480)
            .build();

        let chart_type = Rc::new(Cell::new(crate::charts::ChartType::Bar));
        let data_rc = Rc::new(data);

        let preview = gtk::DrawingArea::new();
        preview.set_vexpand(true);
        preview.set_hexpand(true);
        let ct = chart_type.clone();
        let d = data_rc.clone();
        preview.set_draw_func(move |_, cr, w, h| {
            let surface = crate::charts::render_chart(&d, ct.get(), w, h);
            cr.set_source_surface(&surface, 0.0, 0.0).unwrap();
            cr.paint().unwrap();
        });

        let type_combo = gtk::DropDown::from_strings(&["Bar", "Line", "Pie"]);
        let ct2 = chart_type.clone();
        let pv = preview.clone();
        type_combo.connect_selected_notify(move |dd| {
            ct2.set(match dd.selected() {
                0 => crate::charts::ChartType::Bar,
                1 => crate::charts::ChartType::Line,
                _ => crate::charts::ChartType::Pie,
            });
            pv.queue_draw();
        });

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.set_margin_start(12); header.set_margin_end(12); header.set_margin_top(6);
        header.append(&gtk::Label::new(Some("Type:")));
        header.append(&type_combo);

        // Insert persists the chart on the sheet (saved into xlsx).
        let insert_btn = gtk::Button::with_label(&suite_common::i18n("Insert into Sheet"));
        insert_btn.add_css_class("suggested-action");
        insert_btn.set_halign(gtk::Align::End);
        insert_btn.set_margin_end(12);
        insert_btn.set_margin_bottom(12);
        {
            let ctl = controller.clone();
            let ct = chart_type.clone();
            let dlg = dialog.clone();
            insert_btn.connect_clicked(move |_| {
                use tables_core::sheet::{ChartKind, ChartSpec};
                let (col, first, last) = {
                    let state = ctl.borrow().state.clone();
                    let st = state.borrow();
                    let sheet = st.sheet();
                    let col = sheet.selected_col;
                    let mut first = None;
                    let mut last = 0;
                    for r in 0..sheet.rows {
                        if sheet.data[r][col].parse::<f64>().is_ok() {
                            first.get_or_insert(r);
                            last = r;
                        }
                    }
                    (col, first, last)
                };
                let Some(first) = first else { return };
                let kind = match ct.get() {
                    crate::charts::ChartType::Bar => ChartKind::Bar,
                    crate::charts::ChartType::Line => ChartKind::Line,
                    crate::charts::ChartType::Pie => ChartKind::Pie,
                };
                let chart = ChartSpec {
                    kind,
                    title: String::new(),
                    x_axis_title: None,
                    y_axis_title: None,
                    legend_position: tables_core::sheet::LegendPosition::Right,
                    series: Vec::new(),
                    cat: (first, 0, last),
                    val: (first, col, last),
                    anchor: (last + 2, col),
                    width_px: 480.0,
                    height_px: 280.0,
                };
                ctl.borrow_mut().mutate_sheet("Insert Chart", move |sheet| {
                    sheet.charts.push(chart);
                });
                dlg.close();
            });
        }

        let box_content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        box_content.append(&header);
        box_content.append(&preview);
        box_content.append(&insert_btn);
        dialog.set_child(Some(&box_content));
        let pw = parent.borrow().clone();
        dialog.present(pw.as_ref());
    })
}
