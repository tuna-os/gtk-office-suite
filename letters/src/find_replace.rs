// SPDX-License-Identifier: GPL-3.0-or-later
//
// find_replace.rs — Find & Replace SearchBar and text navigation for Letters.

use gtk4::{self as gtk, prelude::*};
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;
use crate::formatting::{active_buffer, get_textview};

pub struct FindState {
    pub matches: Vec<(gtk::TextIter, gtk::TextIter)>,
    pub current: usize,
}

/// Build the find/replace search bar that overlays the content area.
/// Uses GtkSearchBar per GNOME HIG (not a stacked bar).
pub fn make_find_replace_widget(tv: &adw::TabView) -> (gtk::SearchBar, gtk::SearchEntry) {
    let tv = tv.clone();

    let search_entry = gtk::SearchEntry::new();
    search_entry.set_placeholder_text(Some("Find\u{2026}"));
    search_entry.set_hexpand(true);
    search_entry.update_property(&[gtk::accessible::Property::Label("Find")]);

    let replace_entry = gtk::Entry::new();
    replace_entry.set_placeholder_text(Some("Replace\u{2026}"));
    replace_entry.update_property(&[gtk::accessible::Property::Label("Replace with")]);

    let match_label = gtk::Label::new(Some(""));
    match_label.add_css_class("dim-label");
    match_label.set_margin_start(4);
    match_label.set_margin_end(4);

    let find_prev = gtk::Button::new();
    find_prev.set_icon_name("go-up-symbolic");
    find_prev.set_tooltip_text(Some(&suite_common::i18n("Previous match (Shift+Enter)")));
    find_prev.update_property(&[gtk::accessible::Property::Label("Previous match")]);
    find_prev.add_css_class("flat");

    let find_next = gtk::Button::new();
    find_next.set_icon_name("go-down-symbolic");
    find_next.set_tooltip_text(Some(&suite_common::i18n("Next match (Enter)")));
    find_next.update_property(&[gtk::accessible::Property::Label("Next match")]);
    find_next.add_css_class("flat");

    let replace_btn = gtk::Button::with_label(&suite_common::i18n("Replace"));
    replace_btn.set_tooltip_text(Some(&suite_common::i18n("Replace current match")));
    replace_btn.add_css_class("flat");

    let replace_all_btn = gtk::Button::with_label(&suite_common::i18n("Replace All"));
    replace_all_btn.set_tooltip_text(Some(&suite_common::i18n("Replace all matches")));
    replace_all_btn.add_css_class("flat");

    let case_toggle = gtk::ToggleButton::builder()
        .label("Aa")
        .tooltip_text(suite_common::i18n("Case sensitive"))
        .build();
    case_toggle.update_property(&[gtk::accessible::Property::Label("Case sensitive")]);
    case_toggle.add_css_class("flat");

    let close_btn = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .tooltip_text("Close (Escape)")
        .build();
    close_btn.update_property(&[gtk::accessible::Property::Label("Close search bar")]);
    close_btn.add_css_class("flat");

    // Shared search state
    let state = Rc::new(RefCell::new(FindState { matches: Vec::new(), current: 0 }));

    // Shared widgets for closures
    let search_data = Rc::new((search_entry.clone(), match_label.clone(), case_toggle.clone()));

    // ── Helper: run search, populate matches, highlight ─────
    let run_search: Rc<Box<dyn Fn()>> = {
        let tv = tv.clone();
        let state = state.clone();
        let sd = search_data.clone();
        Rc::new(Box::new(move || {
            let query = sd.0.text().to_string();
            let ml = &sd.1;
            let ct = &sd.2;
            if query.is_empty() {
                ml.set_label("");
                state.borrow_mut().matches.clear();
                state.borrow_mut().current = 0;
                if let Some(buf) = active_buffer(&tv) {
                    for tag_name in &["search-match", "search-current"] {
                        if let Some(tag) = buf.tag_table().lookup(tag_name) {
                            buf.remove_tag(&tag, &buf.start_iter(), &buf.end_iter());
                        }
                    }
                }
                return;
            }
            if let Some(buf) = active_buffer(&tv) {
                let flags = if ct.is_active() {
                    gtk::TextSearchFlags::TEXT_ONLY
                } else {
                    gtk::TextSearchFlags::CASE_INSENSITIVE
                };
                // Clear previous highlights
                for tag_name in &["search-match", "search-current"] {
                    if let Some(tag) = buf.tag_table().lookup(tag_name) {
                        buf.remove_tag(&tag, &buf.start_iter(), &buf.end_iter());
                    }
                }
                // Find all matches
                let mut matches = Vec::new();
                let mut iter = buf.start_iter();
                while let Some((start, end)) = iter.forward_search(&query, flags, None) {
                    matches.push((start, end));
                    iter = end;
                }
                let count = matches.len();
                state.borrow_mut().matches = matches;
                state.borrow_mut().current = 0;
                ml.set_label(&format!("{}/{}", if count > 0 { 1 } else { 0 }, count));
                // Highlight all matches
                if let Some(tag) = buf.tag_table().lookup("search-match") {
                    for (s, e) in state.borrow().matches.iter() {
                        buf.apply_tag(&tag, s, e);
                    }
                }
                // Highlight current match
                if let Some(tag) = buf.tag_table().lookup("search-current") {
                    if let Some((s, e)) = state.borrow().matches.first() {
                        buf.apply_tag(&tag, s, e);
                        buf.select_range(s, e);
                        scroll_to_cursor(&tv);
                    }
                }
            }
        }))
    };

    // ── On each keystroke ────────────────────────────────────
    {
        let rs = run_search.clone();
        search_entry.connect_search_changed(move |_| {
            rs();
        });
    }

    // ── Case toggle ──────────────────────────────────────────
    {
        let rs = run_search.clone();
        case_toggle.connect_toggled(move |_| {
            rs();
        });
    }

    // ── Find Next ────────────────────────────────────────────
    {
        let tv = tv.clone();
        let state = state.clone();
        let ml = match_label.clone();
        find_next.connect_clicked(move |_| {
            navigate_match(&tv, &state, &ml, 1);
        });
    }

    // ── Find Previous ────────────────────────────────────────
    {
        let tv = tv.clone();
        let state = state.clone();
        let ml = match_label.clone();
        find_prev.connect_clicked(move |_| {
            navigate_match(&tv, &state, &ml, -1);
        });
    }

    // ── Replace current match ────────────────────────────────
    {
        let tv = tv.clone();
        let state = state.clone();
        let re = replace_entry.clone();
        let rs = run_search.clone();
        replace_btn.connect_clicked(move |_| {
            let replacement = re.text().to_string();
            let st = state.borrow();
            if st.matches.is_empty() { return; }
            if let Some((start, end)) = st.matches.get(st.current) {
                if let Some(buf) = active_buffer(&tv) {
                    let mut s = *start;
                    let mut e = *end;
                    buf.begin_user_action();
                    buf.delete(&mut s, &mut e);
                    buf.insert(&mut s, &replacement);
                    buf.end_user_action();
                }
            }
            drop(st);
            rs();
        });
    }

    // ── Replace All ──────────────────────────────────────────
    {
        let tv = tv.clone();
        let state = state.clone();
        let re = replace_entry.clone();
        let rs = run_search.clone();
        replace_all_btn.connect_clicked(move |_| {
            let replacement = re.text().to_string();
            let st = state.borrow();
            let matches = st.matches.clone();
            drop(st);
            if matches.is_empty() { return; }
            if let Some(buf) = active_buffer(&tv) {
                buf.begin_user_action();
                for (s, e) in matches.into_iter().rev() {
                    let mut start = s;
                    let mut end = e;
                    buf.delete(&mut start, &mut end);
                    buf.insert(&mut start, &replacement);
                }
                buf.end_user_action();
            }
            rs();
        });
    }

    // ── Layout ───────────────────────────────────────────────
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    hbox.set_margin_start(6);
    hbox.set_margin_end(6);
    hbox.set_margin_top(2);
    hbox.set_margin_bottom(2);
    hbox.append(&search_entry);
    hbox.append(&replace_entry);
    hbox.append(&match_label);
    hbox.append(&find_prev);
    hbox.append(&find_next);
    hbox.append(&replace_btn);
    hbox.append(&replace_all_btn);
    hbox.append(&case_toggle);
    hbox.append(&close_btn);

    let search_bar = gtk::SearchBar::new();
    search_bar.set_child(Some(&hbox));
    search_bar.set_search_mode(false);
    search_bar.set_show_close_button(false);
    search_bar.connect_entry(&search_entry);

    // Wire close button
    {
        let sb = search_bar.clone();
        close_btn.connect_clicked(move |_| {
            sb.set_search_mode(false);
        });
    }

    // Enter in search field = find next
    {
        search_entry.connect_activate(move |_| {
            find_next.activate();
        });
    }

    (search_bar, search_entry)
}

/// Scroll the active text view so the cursor is visible.
/// Navigate to the next/previous match and update highlights.
pub fn navigate_match(tv: &adw::TabView, state: &RefCell<FindState>, ml: &gtk::Label, direction: i32) {
    let mut st = state.borrow_mut();
    if st.matches.is_empty() { return; }
    let n = st.matches.len() as i32;
    let new_idx = ((st.current as i32 + direction).rem_euclid(n)) as usize;
    st.current = new_idx;
    let m = st.matches[new_idx];
    drop(st);
    if let Some(buf) = active_buffer(tv) {
        if let Some(tag) = buf.tag_table().lookup("search-current") {
            buf.remove_tag(&tag, &buf.start_iter(), &buf.end_iter());
        }
        if let Some(tag) = buf.tag_table().lookup("search-current") {
            buf.apply_tag(&tag, &m.0, &m.1);
        }
        buf.select_range(&m.0, &m.1);
        scroll_to_cursor(tv);
    }
    ml.set_label(&format!("{}/{}", new_idx + 1, n));
}

pub fn scroll_to_cursor(tv: &adw::TabView) {
    if let Some(page) = tv.selected_page() {
        if let Some(textview) = get_textview(&page.child()) {
            let buf = textview.buffer();
            let mark = buf.get_insert();
            textview.scroll_to_mark(&mark, 0.0, true, 0.0, 0.0);
        }
    }
}
