//! chart_inspector.rs — the Format inspector's Chart tab: the chart's
//! type, drawn as each would look, its series (named, added, removed) and
//! its data sheet (a category per row and each series' value for it).
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

/// One row of the data sheet: a category and each series' value.
struct PointRow {
    row: adw::ActionRow,
    category: gtk::Entry,
    /// Entries, not spin buttons: a spin button shows a fixed number of
    /// digits, and on leaving it writes the rounded number it shows back
    /// (4.333 became 4.33, an edit nobody made).
    values: Vec<gtk::Entry>,
}

/// One series' row: its name, and the button that takes it out.
struct SeriesRow {
    row: adw::ActionRow,
    name: gtk::Entry,
    remove: gtk::Button,
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
        crate::canvas::draw_chart_data(cr, &sample, w as f64 * 2.0, h as f64 * 2.0, false);
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
    let series_group = adw::PreferencesGroup::builder()
        .title("Series")
        .description("Each series' name in the legend")
        .build();
    let add_series = gtk::Button::builder().label("Add Series").valign(gtk::Align::Center).build();
    add_series.add_css_class("flat");
    add_series.update_property(&[gtk::accessible::Property::Label("Add Series")]);
    {
        let apply = apply.clone();
        add_series.connect_clicked(move |_| apply(ChartEdit::AddSeries));
    }
    series_group.set_header_suffix(Some(&add_series));
    let points_group = adw::PreferencesGroup::builder()
        .title("Data")
        .description("A category, and each series' value for it")
        .build();
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
    page.add(&series_group);
    page.add(&points_group);
    page.add(&add_group);

    // The rows, rebuilt when the number of series or points changes.
    let series_rows: Rc<RefCell<Vec<SeriesRow>>> = Rc::default();
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
    let make_series_row = {
        let (apply, syncing) = (apply.clone(), syncing.clone());
        move |k: usize| -> SeriesRow {
            let n = k + 1;
            let name = gtk::Entry::builder().hexpand(true).valign(gtk::Align::Center).placeholder_text("No name").build();
            name.update_property(&[gtk::accessible::Property::Label(&format!("Series {n} Name"))]);
            {
                let apply = apply.clone();
                on_commit(&name, &syncing, move |e, _| apply(ChartEdit::SeriesName(k, e.text().to_string())));
            }
            let remove = gtk::Button::from_icon_name("list-remove-symbolic");
            remove.add_css_class("flat");
            remove.set_valign(gtk::Align::Center);
            remove.set_tooltip_text(Some("Remove Series"));
            remove.update_property(&[gtk::accessible::Property::Label(&format!("Remove Series {n}"))]);
            {
                let apply = apply.clone();
                remove.connect_clicked(move |_| apply(ChartEdit::RemoveSeries(k)));
            }
            let row = adw::ActionRow::builder().title(format!("{n}")).build();
            row.add_suffix(&name);
            row.add_suffix(&remove);
            SeriesRow { row, name, remove }
        }
    };
    let make_row = {
        let (apply, syncing) = (apply.clone(), syncing.clone());
        move |i: usize, series: usize| -> PointRow {
            let n = i + 1;
            let category = gtk::Entry::builder().width_chars(6).max_width_chars(10).hexpand(true).valign(gtk::Align::Center).build();
            category.update_property(&[gtk::accessible::Property::Label(&format!("Category {n}"))]);
            {
                let apply = apply.clone();
                on_commit(&category, &syncing, move |e, _| apply(ChartEdit::Category(i, e.text().to_string())));
            }
            let row = adw::ActionRow::new();
            row.add_prefix(&category);
            let values: Vec<gtk::Entry> = (0..series)
                .map(|k| {
                    let value = gtk::Entry::builder()
                        .width_chars(5)
                        .max_width_chars(8)
                        .xalign(1.0)
                        .input_purpose(gtk::InputPurpose::Number)
                        .valign(gtk::Align::Center)
                        .build();
                    // The first series' values are "Value n", as with one.
                    let label = if k == 0 { format!("Value {n}") } else { format!("Value {n}, Series {}", k + 1) };
                    value.update_property(&[gtk::accessible::Property::Label(&label)]);
                    let (apply, resync) = (apply.clone(), resync.clone());
                    on_commit(&value, &syncing, move |e, last| {
                        // Not a number: left alone while it is being typed
                        // ("-", "1e"), and the value it was comes back when
                        // the field is left.
                        match e.text().trim().replace(',', ".").parse::<f64>() {
                            Ok(v) if v.is_finite() => apply(ChartEdit::Value(k, i, v)),
                            _ if last => resync(),
                            _ => {}
                        }
                    });
                    row.add_suffix(&value);
                    value
                })
                .collect();
            let remove = gtk::Button::from_icon_name("list-remove-symbolic");
            remove.add_css_class("flat");
            remove.set_valign(gtk::Align::Center);
            remove.set_tooltip_text(Some("Remove Value"));
            remove.update_property(&[gtk::accessible::Property::Label(&format!("Remove Value {n}"))]);
            {
                let apply = apply.clone();
                remove.connect_clicked(move |_| apply(ChartEdit::RemovePoint(i)));
            }
            row.add_suffix(&remove);
            PointRow { row, category, values }
        }
    };

    let sync: Rc<dyn Fn(&ChartData)> = {
        let (syncing, rows, points_group, last) = (syncing.clone(), rows.clone(), points_group.clone(), last.clone());
        let (series_rows, series_group) = (series_rows.clone(), series_group.clone());
        let leave_alone = move |e: &gtk::Entry| editing(e) && !forcing.get();
        Rc::new(move |chart: &ChartData| {
            *last.borrow_mut() = Some(chart.clone());
            syncing.set(true);
            for (kind, b) in &kind_buttons {
                if *kind == chart.kind && !b.is_active() {
                    b.set_active(true);
                }
            }
            let k = chart.series.len();
            let mut series_rows = series_rows.borrow_mut();
            if series_rows.len() != k {
                for r in series_rows.drain(..) {
                    series_group.remove(&r.row);
                }
                for s in 0..k {
                    let r = make_series_row(s);
                    series_group.add(&r.row);
                    series_rows.push(r);
                }
            }
            for (r, s) in series_rows.iter().zip(&chart.series) {
                if r.name.text() != s.name && !leave_alone(&r.name) {
                    r.name.set_text(&s.name);
                }
                // A chart keeps one series.
                r.remove.set_visible(k > 1);
            }
            let mut rows = rows.borrow_mut();
            if rows.len() != chart.categories.len() || rows.first().is_some_and(|r| r.values.len() != k) {
                for r in rows.drain(..) {
                    points_group.remove(&r.row);
                }
                for i in 0..chart.categories.len() {
                    let r = make_row(i, k);
                    points_group.add(&r.row);
                    rows.push(r);
                }
            }
            for (i, (r, category)) in rows.iter().zip(&chart.categories).enumerate() {
                if r.category.text() != *category && !leave_alone(&r.category) {
                    r.category.set_text(category);
                }
                for (entry, s) in r.values.iter().zip(&chart.series) {
                    let v = shown(s.values.get(i).copied().unwrap_or(0.0));
                    if entry.text() != v && !leave_alone(entry) {
                        entry.set_text(&v);
                    }
                }
            }
            syncing.set(false);
        })
    };

    *resync_slot.borrow_mut() = Some(sync.clone());
    ChartInspector { page, sync }
}
