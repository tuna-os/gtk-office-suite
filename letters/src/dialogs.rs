// dialogs.rs — modal and inline dialogs for Letters.
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk4::{self as gtk, prelude::*};
use adw::prelude::*;
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;
use suite_common::i18n;

use crate::page_container::PageContainer;

/// State for the Find and Replace bar.
pub struct FindState {
    pub matches: Vec<(i32, i32)>, // (start_offset, end_offset)
    pub current_idx: usize,
}

impl Default for FindState {
    fn default() -> Self {
        Self {
            matches: Vec::new(),
            current_idx: 0,
        }
    }
}

/// Helper to get the active GtkTextBuffer from an AdwTabView.
pub fn active_buffer(tv: &adw::TabView) -> Option<gtk::TextBuffer> {
    tv.selected_page()
        .and_then(|p| get_textview(&p.child()))
        .map(|tv| tv.buffer())
}

/// Helper to find the GtkTextView inside a page widget hierarchy.
pub fn get_textview(widget: &impl IsA<gtk::Widget>) -> Option<gtk::TextView> {
    if let Ok(tv) = widget.clone().upcast::<gtk::Widget>().downcast::<gtk::TextView>() {
        return Some(tv);
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        if let Some(tv) = get_textview(&c) {
            return Some(tv);
        }
        child = c.next_sibling();
    }
    None
}

/// Show the header and footer configuration dialog.
pub fn show_header_footer_dialog(pc: &PageContainer) {
    let dialog = adw::AlertDialog::new(
        Some(&i18n("Headers and Footers")),
        Some(&i18n("Use {page} for automatic page numbering.")),
    );

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);

    let hdr_entry = gtk::Entry::builder()
        .placeholder_text(i18n("Header text"))
        .text(&pc.header_text())
        .build();
    let ftr_entry = gtk::Entry::builder()
        .placeholder_text(i18n("Footer text"))
        .text(&pc.footer_text())
        .build();

    content.append(&gtk::Label::new(Some(&i18n("Header"))));
    content.append(&hdr_entry);
    content.append(&gtk::Label::new(Some(&i18n("Footer"))));
    content.append(&ftr_entry);
    dialog.set_extra_child(Some(&content));

    dialog.add_responses(&[("cancel", &i18n("_Cancel")), ("apply", &i18n("_Apply"))]);
    dialog.set_default_response(Some("apply"));
    dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);

    let parent = pc.root().and_downcast::<adw::ApplicationWindow>();
    let pc = pc.clone();
    dialog.choose(parent.as_ref(), None::<&gtk::gio::Cancellable>, move |response| {
        if response.as_str() == "apply" {
            pc.set_header_text(&hdr_entry.text());
            pc.set_footer_text(&ftr_entry.text());
        }
    });
}

/// Show a dialog for inserting a custom dimension table.
pub fn show_insert_table_dialog(tv: &adw::TabView) {
    let dialog = adw::AlertDialog::new(
        Some(&i18n("Insert Table")),
        Some(&i18n("Specify rows and columns for the new table.")),
    );

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);

    let rows_spin = gtk::SpinButton::with_range(1.0, 50.0, 1.0);
    rows_spin.set_value(3.0);
    let cols_spin = gtk::SpinButton::with_range(1.0, 20.0, 1.0);
    cols_spin.set_value(3.0);

    let grid = gtk::Grid::new();
    grid.set_row_spacing(8);
    grid.set_column_spacing(12);
    grid.attach(&gtk::Label::new(Some(&i18n("Rows:"))), 0, 0, 1, 1);
    grid.attach(&rows_spin, 1, 0, 1, 1);
    grid.attach(&gtk::Label::new(Some(&i18n("Columns:"))), 0, 1, 1, 1);
    grid.attach(&cols_spin, 1, 1, 1, 1);
    content.append(&grid);

    dialog.set_extra_child(Some(&content));
    dialog.add_responses(&[("cancel", &i18n("_Cancel")), ("insert", &i18n("_Insert"))]);
    dialog.set_default_response(Some("insert"));
    dialog.set_response_appearance("insert", adw::ResponseAppearance::Suggested);

    let tv = tv.clone();
    let parent = tv.root().and_downcast::<adw::ApplicationWindow>();
    dialog.choose(parent.as_ref(), None::<&gtk::gio::Cancellable>, move |response| {
        if response.as_str() == "insert" {
            let rows = rows_spin.value() as u32;
            let cols = cols_spin.value() as u32;
            if let Some(buf) = active_buffer(&tv) {
                let mut md = String::new();
                md.push('|');
                for c in 0..cols { md.push_str(&format!(" Header {} |", c + 1)); }
                md.push('\n');
                md.push('|');
                for _ in 0..cols { md.push_str(" --- |"); }
                md.push('\n');
                for r in 0..rows {
                    md.push('|');
                    for c in 0..cols { md.push_str(&format!(" Cell {}.{} |", r + 1, c + 1)); }
                    md.push('\n');
                }
                let ins = buf.selection_bounds().map(|(i, _)| i).unwrap_or_else(|| buf.start_iter());
                let mut pos = ins;
                buf.insert(&mut pos, &md);

                crate::bridge::apply_structured_edit(&buf, |editor| {
                    editor.insert_table(rows, cols);
                });
            }
        }
    });
}

/// Create and wire the search bar and find/replace controls for Letters.
pub fn make_find_replace_widget(tv: &adw::TabView) -> (gtk::SearchBar, gtk::SearchEntry) {
    let search_bar = gtk::SearchBar::new();
    let search_entry = gtk::SearchEntry::new();
    search_entry.set_placeholder_text(Some(&i18n("Find\u{2026}")));
    search_entry.set_hexpand(true);
    search_bar.set_key_capture_widget(Some(tv));
    search_bar.connect_entry(&search_entry);

    let match_label = gtk::Label::new(None);
    match_label.add_css_class("dim-label");

    let prev_btn = gtk::Button::from_icon_name("go-up-symbolic");
    prev_btn.set_tooltip_text(Some(&i18n("Previous match (Shift+Enter)")));
    prev_btn.add_css_class("flat");

    let next_btn = gtk::Button::from_icon_name("go-down-symbolic");
    next_btn.set_tooltip_text(Some(&i18n("Next match (Enter)")));
    next_btn.add_css_class("flat");

    let replace_entry = gtk::Entry::builder()
        .placeholder_text(i18n("Replace\u{2026}"))
        .visible(false)
        .build();

    let replace_btn = gtk::Button::builder()
        .label(i18n("Replace"))
        .visible(false)
        .build();
    replace_btn.add_css_class("flat");

    let replace_all_btn = gtk::Button::builder()
        .label(i18n("Replace All"))
        .visible(false)
        .build();
    replace_all_btn.add_css_class("flat");

    let toggle_replace = gtk::ToggleButton::builder()
        .icon_name("edit-find-replace-symbolic")
        .tooltip_text(i18n("Toggle Replace"))
        .build();
    toggle_replace.add_css_class("flat");

    let row1 = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row1.set_margin_start(12);
    row1.set_margin_end(12);
    row1.set_margin_top(4);
    row1.set_margin_bottom(4);
    row1.append(&toggle_replace);
    row1.append(&search_entry);
    row1.append(&match_label);
    row1.append(&prev_btn);
    row1.append(&next_btn);

    let row2 = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row2.set_margin_start(12);
    row2.set_margin_end(12);
    row2.set_margin_bottom(4);
    row2.append(&replace_entry);
    row2.append(&replace_btn);
    row2.append(&replace_all_btn);

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 2);
    vbox.append(&row1);
    vbox.append(&row2);
    search_bar.set_child(Some(&vbox));

    let re_c = replace_entry.clone();
    let rb_c = replace_btn.clone();
    let rab_c = replace_all_btn.clone();
    toggle_replace.connect_toggled(move |btn| {
        let active = btn.is_active();
        re_c.set_visible(active);
        rb_c.set_visible(active);
        rab_c.set_visible(active);
    });

    let state = Rc::new(RefCell::new(FindState::default()));

    // Search changed handler
    {
        let tv = tv.clone();
        let state = state.clone();
        let ml = match_label.clone();
        search_entry.connect_search_changed(move |entry| {
            let query = entry.text().to_string();
            let mut st = state.borrow_mut();
            st.matches.clear();
            st.current_idx = 0;

            if let Some(buf) = active_buffer(&tv) {
                // Clear old tags
                let start = buf.start_iter();
                let end = buf.end_iter();
                if let Some(tag) = buf.tag_table().lookup("search-match") {
                    buf.remove_tag(&tag, &start, &end);
                }
                if let Some(tag) = buf.tag_table().lookup("search-current") {
                    buf.remove_tag(&tag, &start, &end);
                }

                if !query.is_empty() {
                    let mut cur = buf.start_iter();
                    while let Some((m_start, m_end)) = cur.forward_search(
                        &query,
                        gtk::TextSearchFlags::CASE_INSENSITIVE,
                        None,
                    ) {
                        let so = m_start.offset();
                        let eo = m_end.offset();
                        st.matches.push((so, eo));
                        if let Some(tag) = buf.tag_table().lookup("search-match") {
                            buf.apply_tag(&tag, &m_start, &m_end);
                        }
                        cur = m_end;
                    }
                }
            }

            let count = st.matches.len();
            if count == 0 {
                ml.set_text(if query.is_empty() { "" } else { "No matches" });
            } else {
                ml.set_text(&format!("1 of {count}"));
                if let Some(buf) = active_buffer(&tv) {
                    let (so, eo) = st.matches[0];
                    let ms = buf.iter_at_offset(so);
                    let me = buf.iter_at_offset(eo);
                    if let Some(tag) = buf.tag_table().lookup("search-current") {
                        buf.apply_tag(&tag, &ms, &me);
                    }
                    buf.select_range(&ms, &me);
                    scroll_to_cursor(&tv);
                }
            }
        });
    }

    // Navigation buttons
    {
        let tv = tv.clone();
        let state = state.clone();
        let ml = match_label.clone();
        next_btn.connect_clicked(move |_| navigate_match(&tv, &state, &ml, 1));
    }
    {
        let tv = tv.clone();
        let state = state.clone();
        let ml = match_label.clone();
        prev_btn.connect_clicked(move |_| navigate_match(&tv, &state, &ml, -1));
    }

    // Replace handler
    {
        let tv = tv.clone();
        let state = state.clone();
        let ml = match_label.clone();
        let se = search_entry.clone();
        let re = replace_entry.clone();
        replace_btn.connect_clicked(move |_| {
            if let Some(buf) = active_buffer(&tv) {
                let st = state.borrow();
                if !st.matches.is_empty() && st.current_idx < st.matches.len() {
                    let (so, eo) = st.matches[st.current_idx];
                    drop(st);
                    let rep = re.text().to_string();
                    let mut ms = buf.iter_at_offset(so);
                    let mut me = buf.iter_at_offset(eo);
                    buf.begin_user_action();
                    buf.delete(&mut ms, &mut me);
                    buf.insert(&mut ms, &rep);
                    buf.end_user_action();
                    // Re-trigger search
                    se.emit_by_name::<()>("search-changed", &[]);
                }
            }
        });
    }

    // Replace All handler
    {
        let tv = tv.clone();
        let se = search_entry.clone();
        let re = replace_entry;
        replace_all_btn.connect_clicked(move |_| {
            let query = se.text().to_string();
            let rep = re.text().to_string();
            if query.is_empty() { return; }
            if let Some(buf) = active_buffer(&tv) {
                buf.begin_user_action();
                let mut cur = buf.start_iter();
                while let Some((mut ms, mut me)) = cur.forward_search(
                    &query,
                    gtk::TextSearchFlags::CASE_INSENSITIVE,
                    None,
                ) {
                    buf.delete(&mut ms, &mut me);
                    buf.insert(&mut ms, &rep);
                    cur = ms;
                }
                buf.end_user_action();
                se.emit_by_name::<()>("search-changed", &[]);
            }
        });
    }

    (search_bar, search_entry)
}

pub fn navigate_match(tv: &adw::TabView, state: &RefCell<FindState>, ml: &gtk::Label, direction: i32) {
    let mut st = state.borrow_mut();
    let count = st.matches.len();
    if count == 0 { return; }

    if let Some(buf) = active_buffer(tv) {
        let (old_so, old_eo) = st.matches[st.current_idx];
        let old_ms = buf.iter_at_offset(old_so);
        let old_me = buf.iter_at_offset(old_eo);
        if let Some(tag) = buf.tag_table().lookup("search-current") {
            buf.remove_tag(&tag, &old_ms, &old_me);
        }
        if let Some(tag) = buf.tag_table().lookup("search-match") {
            buf.apply_tag(&tag, &old_ms, &old_me);
        }

        if direction > 0 {
            st.current_idx = (st.current_idx + 1) % count;
        } else {
            st.current_idx = (st.current_idx + count - 1) % count;
        }

        let (so, eo) = st.matches[st.current_idx];
        let ms = buf.iter_at_offset(so);
        let me = buf.iter_at_offset(eo);
        if let Some(tag) = buf.tag_table().lookup("search-current") {
            buf.apply_tag(&tag, &ms, &me);
        }
        buf.select_range(&ms, &me);
        ml.set_text(&format!("{} of {count}", st.current_idx + 1));
        scroll_to_cursor(tv);
    }
}

pub fn scroll_to_cursor(tv: &adw::TabView) {
    if let Some(page) = tv.selected_page() {
        if let Some(text_view) = get_textview(&page.child()) {
            let mark = text_view.buffer().get_insert();
            text_view.scroll_to_mark(&mark, 0.1, false, 0.0, 0.0);
        }
    }
}
