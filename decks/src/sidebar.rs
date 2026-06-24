// sidebar.rs — Slide sidebar list and management controls.
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk4::{self as gtk, prelude::*};
use crate::engine::Slide;

/// Rebuild the slide list widget from the current slides state.
pub fn rebuild_slide_list(list: &gtk::ListBox, slides: &[Slide], selected: usize) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    for (i, _slide) in slides.iter().enumerate() {
        let row = gtk::ListBoxRow::new();
        let label = gtk::Label::new(Some(&format!("Slide {}", i + 1)));
        label.set_halign(gtk::Align::Start);
        label.set_margin_start(12);
        label.set_margin_end(12);
        label.set_margin_top(8);
        label.set_margin_bottom(8);
        row.set_child(Some(&label));
        list.append(&row);
    }
    if let Some(row) = list.row_at_index(selected as i32) {
        list.select_row(Some(&row));
    }
}
