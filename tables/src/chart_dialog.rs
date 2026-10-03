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
use adw::prelude::*;

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

        let chart_type = Rc::new(Cell::new(tables_core::sheet::ChartKind::Bar));
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

        // The kinds Excel and Calc share, in their order.
        use tables_core::sheet::ChartKind;
        const KINDS: [(&str, ChartKind); 5] = [
            ("Column", ChartKind::Bar),
            ("Line", ChartKind::Line),
            ("Area", ChartKind::Area),
            ("Pie", ChartKind::Pie),
            ("XY (Scatter)", ChartKind::Scatter),
        ];
        // All five in view as linked toggles, like the inspector's
        // alignment buttons: nothing to open to see what there is.
        let type_combo = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        type_combo.add_css_class("linked");
        let mut first: Option<gtk::ToggleButton> = None;
        for (name, kind) in KINDS {
            let button = gtk::ToggleButton::with_label(&suite_common::i18n(name));
            button.set_group(first.as_ref());
            if first.is_none() {
                button.set_active(true);
                first = Some(button.clone());
            }
            let (ct2, pv) = (chart_type.clone(), preview.clone());
            button.connect_toggled(move |b| {
                if b.is_active() {
                    ct2.set(kind);
                    pv.queue_draw();
                }
            });
            type_combo.append(&button);
        }

        // The type as a row, the chart drawn below it as a card.
        type_combo.set_valign(gtk::Align::Center);
        let type_row = adw::ActionRow::builder().title(suite_common::i18n("Type")).build();
        type_row.add_suffix(&type_combo);
        let group = adw::PreferencesGroup::new();
        group.add(&type_row);
        preview.set_size_request(-1, 280);
        let card = gtk::Frame::new(None);
        card.add_css_class("card");
        card.set_child(Some(&preview));
        // Insert puts the chart on the sheet (saved into xlsx).
        let suite_common::dialogs::ActionDialog { dialog, action: insert_btn } = suite_common::dialogs::action_dialog(
            &suite_common::i18n("Insert Chart"),
            &suite_common::i18n("_Insert"),
            600,
            &suite_common::dialogs::form_body(&[group.upcast_ref(), card.upcast_ref()]),
        );
        {
            let ctl = controller.clone();
            let ct = chart_type.clone();
            let dlg = dialog.clone();
            insert_btn.connect_clicked(move |_| {
                use tables_core::sheet::ChartSpec;
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
                let kind = ct.get();
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

        let pw = parent.borrow().clone();
        dialog.present(pw.as_ref());
    })
}
