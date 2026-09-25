// SPDX-License-Identifier: GPL-3.0-or-later
//! The Format inspector's Number group: the selection's format as a code
//! (Excel's and Calc's language: `#,##0.00;[Red]-#,##0.00`), edited in
//! place with a live preview of the active cell, and a few common codes to
//! start from. Applying sets every selected cell's format as one undo step.
//! Codes a named kind draws exactly stay that kind; any other code is kept
//! as written (tables_core::io::kind_for_code), so it saves back unchanged.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk4::{self as gtk};
use libadwaita as adw;
use suite_common::format::NumberFormat;
use tables_core::controller::WorkbookController;

type Ctl = Rc<RefCell<WorkbookController>>;

/// Codes offered to start from.
const PRESETS: [(&str, &str); 9] = [
    ("General", "General"),
    ("Number", "#,##0.00"),
    ("Currency", "\"$\"#,##0.00"),
    ("Accounting", "_(\"$\"* #,##0.00_);_(\"$\"* (#,##0.00);_(\"$\"* \"-\"??_);_(@_)"),
    ("Percent", "0.00%"),
    ("Scientific", "0.00E+00"),
    ("Fraction", "# ?/?"),
    ("Date", "yyyy-mm-dd"),
    ("Time", "h:mm AM/PM"),
];

/// The active cell's value, or a sample when it's empty.
fn preview_value(ctl: &Ctl) -> String {
    let c = ctl.borrow();
    let state = c.state.borrow();
    let s = state.sheet();
    let v = state.engine.cell(s.selected_row, s.selected_col);
    if v.is_empty() { "1234.5".into() } else { v }
}

fn format_of(code: &str) -> NumberFormat {
    let code = code.trim();
    if code.is_empty() || code.eq_ignore_ascii_case("general") {
        NumberFormat::default()
    } else {
        NumberFormat::new(tables_core::io::kind_for_code(code))
    }
}

/// The Number group, its sync (call when the selection changes), and its
/// code row, for focusing from the keyboard.
pub fn group(ctl: &Ctl, grid: &gtk::DrawingArea) -> (adw::PreferencesGroup, Rc<dyn Fn()>, adw::EntryRow) {
    let group = adw::PreferencesGroup::builder().title("Number").build();

    let code = adw::EntryRow::builder().title("Format Code").show_apply_button(true).build();
    group.add(&code);
    let preview = adw::ActionRow::builder().title("Preview").subtitle_selectable(true).build();
    preview.add_css_class("property");
    group.add(&preview);

    let presets = gtk::StringList::new(&PRESETS.iter().map(|p| p.0).collect::<Vec<_>>());
    let start = adw::ComboRow::builder().title("Start From").model(&presets).build();
    group.add(&start);

    let syncing = Rc::new(Cell::new(false));
    let update_preview = {
        let (ctl, preview, code) = (ctl.clone(), preview.clone(), code.clone());
        Rc::new(move || {
            let value = preview_value(&ctl);
            let shown = format_of(&code.text()).format(&value);
            preview.set_subtitle(&if shown.is_empty() { " ".into() } else { gtk::glib::markup_escape_text(&shown).to_string() });
        })
    };
    {
        let update_preview = update_preview.clone();
        code.connect_changed(move |_| update_preview());
    }
    {
        let (ctl, grid) = (ctl.clone(), grid.clone());
        code.connect_apply(move |row| {
            let nf = format_of(&row.text());
            ctl.borrow_mut().mutate_sheet("Number Format", move |s| {
                let (r0, c0, r1, c1) = s.selection_block();
                for r in r0..=r1.min(s.rows.saturating_sub(1)) {
                    for c in c0..=c1.min(s.cols.saturating_sub(1)) {
                        s.formats[r][c] = nf.clone();
                    }
                }
            });
            grid.queue_draw();
        });
    }
    {
        let (code, syncing) = (code.clone(), syncing.clone());
        start.connect_selected_notify(move |row| {
            if syncing.get() {
                return;
            }
            if let Some((_, preset)) = PRESETS.get(row.selected() as usize) {
                code.set_text(preset);
                code.grab_focus();
            }
        });
    }

    let sync: Rc<dyn Fn()> = {
        let (ctl, code, start, syncing) = (ctl.clone(), code.clone(), start.clone(), syncing.clone());
        Rc::new(move || {
            let Ok(c) = ctl.try_borrow() else { return };
            let kind = {
                let state = c.state.borrow();
                let s = state.sheet();
                s.formats.get(s.selected_row).and_then(|r| r.get(s.selected_col)).map(|f| f.kind.clone())
            };
            drop(c);
            let text = kind
                .as_ref()
                .and_then(tables_core::io::code_for_kind)
                .unwrap_or_else(|| "General".into());
            syncing.set(true);
            code.set_text(&text);
            let preset = PRESETS.iter().position(|p| p.1 == text).unwrap_or(0);
            start.set_selected(preset as u32);
            syncing.set(false);
            update_preview();
        })
    };
    (group, sync, code)
}
