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

use std::cell::RefCell;
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
    grid: gtk4::DrawingArea,
) -> Box<dyn Fn()> {
    dialog(parent, state, controller, grid, false)
}

/// Returns the callback the `edit-chart` action runs: the same dialog,
/// opened on the chart that covers the selected cell. Applying replaces that
/// chart as one undo step. It does nothing when no chart is under the cell.
pub fn edit_opener(
    parent: Rc<RefCell<Option<adw::ApplicationWindow>>>,
    state: Rc<RefCell<WorkbookState>>,
    controller: Rc<RefCell<WorkbookController>>,
    grid: gtk4::DrawingArea,
) -> Box<dyn Fn()> {
    dialog(parent, state, controller, grid, true)
}

fn dialog(
    parent: Rc<RefCell<Option<adw::ApplicationWindow>>>,
    state: Rc<RefCell<WorkbookState>>,
    controller: Rc<RefCell<WorkbookController>>,
    grid: gtk4::DrawingArea,
    edit: bool,
) -> Box<dyn Fn()> {
    Box::new(move || {
        // What the selection asks for: one column over the first column's
        // labels, or a table of a category column and a series per column to
        // its right (`ChartSpec::from_selection`).
        use tables_core::sheet::{ChartKind, ChartSpec, LegendPosition};
        let (draft, categories, plotted, edit_index) = {
            let st = state.borrow();
            let sheet = st.sheets[st.active_sheet].borrow();
            if edit {
                let Some(i) = sheet.chart_at_cell(sheet.selected_row, sheet.selected_col) else { return };
                let spec = sheet.charts[i].clone();
                let (categories, plotted) = spec.plot(&sheet);
                (spec, categories, plotted, Some(i))
            } else {
                let block = sheet.selection_block();
                let Some(mut spec) = ChartSpec::from_selection(&sheet, block, ChartKind::Bar, (0, 0), 480.0, 280.0) else { return };
                // Placed under the data, at the selection's left edge.
                let last = spec.series.iter().map(|s| s.val.2).max().unwrap_or(block.2);
                spec.anchor = (last + 2, block.1);
                let (categories, plotted) = spec.plot(&sheet);
                (spec, categories, plotted, None)
            }
        };
        let draft = Rc::new(RefCell::new(draft));
        let plot = Rc::new((categories, plotted));

        let preview = gtk::DrawingArea::new();
        preview.set_vexpand(true);
        preview.set_hexpand(true);
        {
            let (draft, plot) = (draft.clone(), plot.clone());
            preview.set_draw_func(move |_, cr, w, h| {
                let spec = draft.borrow();
                let series: Vec<crate::charts::Series> =
                    plot.1.iter().map(|(name, values)| crate::charts::Series { name: Some(name), values }).collect();
                let decor = crate::charts::Decor {
                    title: &spec.title,
                    x_title: spec.x_axis_title.as_deref(),
                    y_title: spec.y_axis_title.as_deref(),
                    legend: match spec.legend_position {
                        LegendPosition::None => crate::charts::LegendPos::None,
                        LegendPosition::Top => crate::charts::LegendPos::Top,
                        LegendPosition::Bottom => crate::charts::LegendPos::Bottom,
                        LegendPosition::Left => crate::charts::LegendPos::Left,
                        LegendPosition::Right => crate::charts::LegendPos::Right,
                    },
                };
                let surface = crate::charts::render_chart_decorated(&plot.0, &series, spec.kind, w, h, &decor);
                cr.set_source_surface(&surface, 0.0, 0.0).unwrap();
                cr.paint().unwrap();
            });
        }

        // The kinds Excel and Calc share, in their order.
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
            if std::mem::discriminant(&kind) == std::mem::discriminant(&draft.borrow().kind) {
                button.set_active(true);
            }
            if first.is_none() {
                first = Some(button.clone());
            }
            let (draft, pv) = (draft.clone(), preview.clone());
            button.connect_toggled(move |b| {
                if b.is_active() {
                    draft.borrow_mut().kind = kind;
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

        // Title, axis titles and legend: what the file keeps and Calc and
        // Excel draw.
        let text_row = |label: &str, name: &str, initial: String, set: fn(&mut ChartSpec, String)| {
            let row = adw::EntryRow::builder().title(suite_common::i18n(label)).build();
            row.set_widget_name(name);
            row.set_text(&initial);
            let (draft, pv) = (draft.clone(), preview.clone());
            row.connect_changed(move |r| {
                set(&mut draft.borrow_mut(), r.text().to_string());
                pv.queue_draw();
            });
            row
        };
        let (title0, x0, y0) = {
            let d = draft.borrow();
            (d.title.clone(), d.x_axis_title.clone().unwrap_or_default(), d.y_axis_title.clone().unwrap_or_default())
        };
        group.add(&text_row("Title", "chart-title", title0, |c, t| c.title = t));
        group.add(&text_row("Horizontal axis title", "chart-x-title", x0, |c, t| c.x_axis_title = Some(t).filter(|t| !t.trim().is_empty())));
        group.add(&text_row("Vertical axis title", "chart-y-title", y0, |c, t| c.y_axis_title = Some(t).filter(|t| !t.trim().is_empty())));
        const LEGENDS: [(&str, LegendPosition); 5] = [
            ("Right", LegendPosition::Right),
            ("Bottom", LegendPosition::Bottom),
            ("Top", LegendPosition::Top),
            ("Left", LegendPosition::Left),
            ("None", LegendPosition::None),
        ];
        let names: Vec<String> = LEGENDS.iter().map(|(n, _)| suite_common::i18n(n)).collect();
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let legend_row = adw::ComboRow::builder().title(suite_common::i18n("Legend")).model(&gtk::StringList::new(&name_refs)).build();
        legend_row.set_widget_name("chart-legend");
        let legend0 = LEGENDS.iter().position(|(_, p)| *p == draft.borrow().legend_position).unwrap_or(0);
        legend_row.set_selected(legend0 as u32);
        {
            let (draft, pv) = (draft.clone(), preview.clone());
            legend_row.connect_selected_notify(move |r| {
                if let Some((_, pos)) = LEGENDS.get(r.selected() as usize) {
                    draft.borrow_mut().legend_position = *pos;
                    pv.queue_draw();
                }
            });
        }
        group.add(&legend_row);

        preview.set_size_request(-1, 280);
        let card = gtk::Frame::new(None);
        card.add_css_class("card");
        card.set_child(Some(&preview));
        // Insert puts the chart on the sheet (saved into xlsx).
        let suite_common::dialogs::ActionDialog { dialog, action: insert_btn } = suite_common::dialogs::action_dialog(
            &suite_common::i18n(if edit { "Edit Chart" } else { "Insert Chart" }),
            &suite_common::i18n(if edit { "_Apply" } else { "_Insert" }),
            600,
            &suite_common::dialogs::form_body(&[group.upcast_ref(), card.upcast_ref()]),
        );
        {
            let ctl = controller.clone();
            let draft = draft.clone();
            let dlg = dialog.clone();
            let grid = grid.clone();
            insert_btn.connect_clicked(move |_| {
                // One undo step, whatever the dialog held.
                let chart = draft.borrow().clone();
                match edit_index {
                    Some(i) => crate::sheet_actions::apply(&ctl, &grid, |c| { c.mutate_sheet("Edit Chart", move |sheet| {
                        if let Some(slot) = sheet.charts.get_mut(i) {
                            *slot = chart;
                        }
                    }); }),
                    None => crate::sheet_actions::apply(&ctl, &grid, |c| { c.mutate_sheet("Insert Chart", move |sheet| {
                        sheet.charts.push(chart);
                    }); }),
                };
                dlg.close();
            });
        }

        let pw = parent.borrow().clone();
        dialog.present(pw.as_ref());
    })
}
