//! chart_inspector.rs — the Format inspector's Chart tab: the chart's
//! type, drawn as each would look, and its data sheet (series name, and a
//! category and value per point).
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Keynote's Chart inspector and PowerPoint's Edit Data in one tab. Every
//! change is a decks_core::engine::chart::ChartEdit through
//! DecksController::edit_chart: one undo step when it changes the chart.
//! A text field commits when Enter is pressed, when focus leaves it, or
//! half a second after the last change, so a word typed into a category
//! is one step, not one per letter. The pause is also how text set without
//! the keyboard (a screen reader's editable-text interface, which neither
//! focuses the field nor activates it) reaches the chart.

use adw::prelude::*;
use decks_core::engine::chart::{ChartData, ChartEdit, ChartKind};
use decks_core::DecksController;
use gtk4 as gtk;
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Shows a chart's type and data in the tab.
type Sync = Rc<dyn Fn(&ChartData)>;

/// The Chart tab and its model→widget refresh.
pub struct ChartInspector {
    pub page: adw::PreferencesPage,
    /// Show `chart`'s type and data. Call when the chart or the
    /// selection changes.
    pub sync: Rc<dyn Fn(&ChartData)>,
}

/// One row of the data sheet.
struct PointRow {
    row: adw::ActionRow,
    category: gtk::Entry,
    /// An entry, not a spin button: a spin button shows a fixed number of
    /// digits, and on leaving it writes the rounded number it shows back
    /// (4.333 became 4.33, an edit nobody made).
    value: gtk::Entry,
}

/// A value as the data sheet shows it: exactly, with no trailing zeros.
fn shown(v: f64) -> String {
    format!("{v}")
}

/// How long a field waits after the last change before it commits.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(500);

/// Call `commit(entry, final)` when `entry` is activated or loses focus
/// (`final`), and when its text has settled after a change that wasn't
/// the data sheet's own (`!final`: an unfinished value is left alone).
fn on_commit(entry: &gtk::Entry, syncing: &Rc<Cell<bool>>, commit: impl Fn(&gtk::Entry, bool) + 'static) {
    let commit = Rc::new(commit);
    let pending: Rc<RefCell<Option<gtk::glib::SourceId>>> = Rc::default();
    let cancel = {
        let pending = pending.clone();
        move || {
            if let Some(id) = pending.borrow_mut().take() {
                id.remove();
            }
        }
    };
    {
        let (commit, cancel) = (commit.clone(), cancel.clone());
        entry.connect_activate(move |e| {
            cancel();
            commit(e, true);
        });
    }
    {
        let (commit, pending, cancel, syncing) = (commit.clone(), pending.clone(), cancel.clone(), syncing.clone());
        entry.connect_changed(move |e| {
            if syncing.get() {
                return;
            }
            cancel();
            let (commit, e2, slot) = (commit.clone(), e.clone(), pending.clone());
            let id = gtk::glib::timeout_add_local_once(SETTLE, move || {
                slot.borrow_mut().take();
                commit(&e2, false);
            });
            *pending.borrow_mut() = Some(id);
        });
    }
    let focus = gtk::EventControllerFocus::new();
    let e = entry.clone();
    focus.connect_leave(move |_| {
        cancel();
        commit(&e, true);
    });
    entry.add_controller(focus);
}

/// Whether the person is in `entry`: the data sheet leaves its text alone.
fn editing(entry: &gtk::Entry) -> bool {
    entry.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN)
}

/// A toggle for chart type `kind`: a small drawing of it and its name.
fn kind_button(kind: ChartKind, group: Option<&gtk::ToggleButton>) -> gtk::ToggleButton {
    let name = ChartData::kind_name(kind);
    let tile = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let area = gtk::DrawingArea::new();
    area.set_content_width(64);
    area.set_content_height(44);
    let sample = ChartData::sample(kind);
    area.set_draw_func(move |_, cr, w, h| {
        // Drawn at twice the size and scaled down, so the axis labels fit.
        cr.scale(0.5, 0.5);
        suite_common::charts::draw_chart(cr, &sample.points, sample.kind, w as f64 * 2.0, h as f64 * 2.0, None);
    });
    tile.append(&area);
    let label = gtk::Label::new(Some(name));
    label.add_css_class("caption");
    tile.append(&label);
    let b = gtk::ToggleButton::builder().child(&tile).build();
    b.add_css_class("flat");
    b.set_tooltip_text(Some(&format!("{name} Chart")));
    b.update_property(&[gtk::accessible::Property::Label(name)]);
    if let Some(g) = group {
        b.set_group(Some(g));
    }
    b
}

pub fn build(
    ctl: &Rc<DecksController>,
    current_slide: &Rc<Cell<usize>>,
    selected: &Rc<Cell<Option<usize>>>,
    changed: Rc<dyn Fn()>,
) -> ChartInspector {
    let syncing = Rc::new(Cell::new(false));
    let apply: Rc<dyn Fn(ChartEdit)> = {
        let (ctl, cs, sel, syncing) = (ctl.clone(), current_slide.clone(), selected.clone(), syncing.clone());
        Rc::new(move |edit: ChartEdit| {
            if syncing.get() {
                return;
            }
            let Some(oi) = sel.get() else { return };
            if ctl.edit_chart(cs.get(), oi, &edit) {
                changed();
            }
        })
    };

    // ── Type ─────────────────────────────────────────────────────────────
    let type_group = adw::PreferencesGroup::builder().title("Chart Type").build();
    let kinds = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(3)
        .min_children_per_line(3)
        .homogeneous(true)
        .row_spacing(6)
        .column_spacing(6)
        .build();
    let mut kind_buttons: Vec<(ChartKind, gtk::ToggleButton)> = Vec::new();
    for kind in decks_core::insert::CHART_KINDS {
        let b = kind_button(kind, kind_buttons.first().map(|(_, b)| b));
        let apply = apply.clone();
        b.connect_toggled(move |t| {
            if t.is_active() {
                apply(ChartEdit::Kind(kind));
            }
        });
        kinds.append(&b);
        kind_buttons.push((kind, b));
    }
    type_group.add(&kinds);

    // ── Data ─────────────────────────────────────────────────────────────
    let data_group = adw::PreferencesGroup::builder()
        .title("Data")
        .description("Each value's category, and the series' name in the legend")
        .build();
    let series = adw::EntryRow::builder().title("Series Name").show_apply_button(true).build();
    {
        let apply = apply.clone();
        series.connect_apply(move |r| apply(ChartEdit::Series(r.text().to_string())));
    }
    data_group.add(&series);
    let points_group = adw::PreferencesGroup::new();
    let add = gtk::Button::builder().label("Add Value").halign(gtk::Align::Start).build();
    add.add_css_class("pill");
    add.update_property(&[gtk::accessible::Property::Label("Add Value")]);
    {
        let apply = apply.clone();
        add.connect_clicked(move |_| apply(ChartEdit::AddPoint));
    }
    let add_group = adw::PreferencesGroup::new();
    add_group.add(&add);

    let page = adw::PreferencesPage::new();
    page.add(&type_group);
    page.add(&data_group);
    page.add(&points_group);
    page.add(&add_group);

    // The data sheet's rows, rebuilt when the number of points changes.
    let rows: Rc<RefCell<Vec<PointRow>>> = Rc::default();
    // Shows the chart again (a value that wasn't a number is put back);
    // bound to `sync` and the selected chart below.
    let last: Rc<RefCell<Option<ChartData>>> = Rc::default();
    let resync_slot: Rc<RefCell<Option<Sync>>> = Rc::default();
    // Set while `resync` runs: it puts a field's value back even while the
    // field has focus (Enter on something that isn't a number).
    let forcing = Rc::new(Cell::new(false));
    let resync: Rc<dyn Fn()> = {
        let (last, slot, forcing) = (last.clone(), resync_slot.clone(), forcing.clone());
        Rc::new(move || {
            let chart = last.borrow().clone();
            let sync = slot.borrow().clone();
            if let (Some(chart), Some(sync)) = (chart, sync) {
                forcing.set(true);
                sync(&chart);
                forcing.set(false);
            }
        })
    };
    let make_row = {
        let (apply, syncing) = (apply.clone(), syncing.clone());
        move |i: usize| -> PointRow {
            let n = i + 1;
            let category = gtk::Entry::builder().width_chars(8).max_width_chars(12).valign(gtk::Align::Center).build();
            category.update_property(&[gtk::accessible::Property::Label(&format!("Category {n}"))]);
            {
                let apply = apply.clone();
                on_commit(&category, &syncing, move |e, _| apply(ChartEdit::Category(i, e.text().to_string())));
            }
            let value = gtk::Entry::builder()
                .width_chars(8)
                .max_width_chars(12)
                .xalign(1.0)
                .input_purpose(gtk::InputPurpose::Number)
                .valign(gtk::Align::Center)
                .build();
            value.update_property(&[gtk::accessible::Property::Label(&format!("Value {n}"))]);
            {
                let (apply, resync) = (apply.clone(), resync.clone());
                on_commit(&value, &syncing, move |e, last| {
                    // Not a number: left alone while it is being typed
                    // ("-", "1e"), and the value it was comes back when
                    // the field is left.
                    match e.text().trim().replace(',', ".").parse::<f64>() {
                        Ok(v) if v.is_finite() => apply(ChartEdit::Value(i, v)),
                        _ if last => resync(),
                        _ => {}
                    }
                });
            }
            let remove = gtk::Button::from_icon_name("list-remove-symbolic");
            remove.add_css_class("flat");
            remove.set_valign(gtk::Align::Center);
            remove.set_tooltip_text(Some("Remove Value"));
            remove.update_property(&[gtk::accessible::Property::Label(&format!("Remove Value {n}"))]);
            {
                let apply = apply.clone();
                remove.connect_clicked(move |_| apply(ChartEdit::RemovePoint(i)));
            }
            let row = adw::ActionRow::new();
            row.add_prefix(&category);
            row.add_suffix(&value);
            row.add_suffix(&remove);
            PointRow { row, category, value }
        }
    };

    let sync: Rc<dyn Fn(&ChartData)> = {
        let (syncing, rows, points_group, series, last) = (syncing.clone(), rows.clone(), points_group.clone(), series.clone(), last.clone());
        let leave_alone = move |e: &gtk::Entry| editing(e) && !forcing.get();
        Rc::new(move |chart: &ChartData| {
            *last.borrow_mut() = Some(chart.clone());
            syncing.set(true);
            for (kind, b) in &kind_buttons {
                if *kind == chart.kind && !b.is_active() {
                    b.set_active(true);
                }
            }
            if series.text() != chart.series {
                series.set_text(&chart.series);
            }
            let mut rows = rows.borrow_mut();
            while rows.len() > chart.points.len() {
                if let Some(r) = rows.pop() {
                    points_group.remove(&r.row);
                }
            }
            while rows.len() < chart.points.len() {
                let r = make_row(rows.len());
                points_group.add(&r.row);
                rows.push(r);
            }
            for (r, (category, value)) in rows.iter().zip(&chart.points) {
                if r.category.text() != *category && !leave_alone(&r.category) {
                    r.category.set_text(category);
                }
                if r.value.text() != shown(*value) && !leave_alone(&r.value) {
                    r.value.set_text(&shown(*value));
                }
            }
            syncing.set(false);
        })
    };

    *resync_slot.borrow_mut() = Some(sync.clone());
    ChartInspector { page, sync }
}
