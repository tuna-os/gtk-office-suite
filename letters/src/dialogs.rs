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
#[derive(Default)]
pub struct FindState {
    pub matches: Vec<(i32, i32)>, // (start_offset, end_offset)
    pub current_idx: usize,
}

/// Helper to get the active GtkTextBuffer from an AdwTabView.
pub fn active_buffer(tv: &adw::TabView) -> Option<gtk::TextBuffer> {
    tv.selected_page().and_then(|p| crate::page_container::buffer_of(&p.child()))
}

/// Call `f(buffer, changed)` for the active tab's buffer whenever its
/// content changes (`changed`) or its caret moves, and when another tab
/// becomes active — coalesced to one call per main-loop idle, after the
/// edit (and the live model's update) is complete.
pub fn watch_active_buffer(tv: &adw::TabView, f: impl Fn(&gtk::TextBuffer, bool) + 'static) {
    let f = Rc::new(f);
    // The buffer (the latest active one) and whether it changed, while a
    // call is scheduled.
    let pending: Rc<RefCell<Option<(gtk::TextBuffer, bool)>>> = Rc::default();
    let schedule = Rc::new(move |buf: &gtk::TextBuffer, changed: bool| {
        let mut p = pending.borrow_mut();
        let scheduled = p.is_some();
        let changed = changed || p.as_ref().is_some_and(|(b, c)| *c && b == buf);
        *p = Some((buf.clone(), changed));
        if scheduled {
            return;
        }
        let (f, pending) = (f.clone(), pending.clone());
        gtk::glib::idle_add_local_once(move || {
            let next = pending.borrow_mut().take();
            if let Some((buf, changed)) = next {
                f(&buf, changed);
            }
        });
    });
    let handlers: RefCell<Vec<(gtk::glib::WeakRef<gtk::TextBuffer>, gtk::glib::SignalHandlerId)>> = RefCell::default();
    tv.connect_selected_page_notify(move |tv| {
        for (buf, id) in handlers.borrow_mut().drain(..) {
            if let Some(buf) = buf.upgrade() {
                buf.disconnect(id);
            }
        }
        let Some(buf) = active_buffer(tv) else { return };
        let s = schedule.clone();
        let changed = buf.connect_changed(move |b| s(b, true));
        let s = schedule.clone();
        let moved = buf.connect_mark_set(move |b, _, mark| {
            if mark.name().as_deref() == Some("insert") {
                s(b, false);
            }
        });
        handlers.borrow_mut().extend([(buf.downgrade(), changed), (buf.downgrade(), moved)]);
        schedule(&buf, true);
    });
}

/// Give the keyboard back to the active tab's page view, scrolled to its
/// caret.
pub fn focus_active_view(tv: &adw::TabView) {
    let Some(child) = tv.selected_page().map(|p| p.child()) else { return };
    if let Some(view) = crate::page_container::find(&child).and_then(|pc| pc.page_view()) {
        view.grab_focus();
        crate::page_edit::scroll_to_caret(&view);
    }
}

/// Show the header and footer configuration dialog.
///
/// The header and footer are the document's: the page view, print and
/// every save draw them from the tab's model. Writing them only to the
/// container, as this dialog once did, meant a header that was shown and
/// never written to the file (#438).
pub fn show_header_footer_dialog(pc: &PageContainer, buf: &gtk::TextBuffer) {
    // An action dialog of entry rows, like Page Setup and Tables' forms.
    // It used to be an alert holding a boxed list, which looked like
    // nothing else in the suite.
    let (header, footer) = header_footer_of(buf);
    let hdr_entry = adw::EntryRow::builder().title(i18n("Header")).text(header).build();
    let ftr_entry = adw::EntryRow::builder().title(i18n("Footer")).text(footer).build();
    let group = adw::PreferencesGroup::builder().description(i18n("Type {page} where the page number goes.")).build();
    group.add(&hdr_entry);
    group.add(&ftr_entry);
    let suite_common::dialogs::ActionDialog { dialog, action } = suite_common::dialogs::action_dialog(
        &i18n("Headers and Footers"),
        &i18n("_Apply"),
        400,
        &suite_common::dialogs::form_body(&[group.upcast_ref()]),
    );
    {
        let (buf, d) = (buf.clone(), dialog.clone());
        action.connect_clicked(move |_| {
            apply_header_footer(&buf, &hdr_entry.text(), &ftr_entry.text());
            d.close();
        });
    }
    dialog.present(pc.root().as_ref());
}

/// The document's header and footer, empty when it has none: what the
/// dialog offers to edit.
fn header_footer_of(buf: &gtk::TextBuffer) -> (String, String) {
    let doc = crate::bridge::document_of(buf);
    (doc.header.unwrap_or_default(), doc.footer.unwrap_or_default())
}

/// Set the document's header and footer, as the dialog's Apply does: one
/// model edit, which Undo takes back.
///
/// Empty text means "no header", not an empty one: the ODT and DOCX writers
/// both emit a header block for `Some("")`, which would put an empty header
/// into every saved file.
pub(crate) fn apply_header_footer(buf: &gtk::TextBuffer, header: &str, footer: &str) {
    let Some(live) = crate::live::of(buf) else { return };
    let present = |text: &str| (!text.is_empty()).then(|| text.to_string());
    let (header, footer) = (present(header), present(footer));
    live.borrow_mut().edit_with(buf, |doc| {
        if (&doc.header, &doc.footer) == (&header, &footer) {
            Vec::new()
        } else {
            vec![letters_core::edit::Op::SetHeaderFooter { header, footer }]
        }
    });
    crate::live::sync_actions(buf);
}

/// Set the document's page size, margins and orientation from Page Setup:
/// one model edit, which Undo takes back. The columns and column gap are
/// the document's own and are kept.
///
/// Page Setup used to write only the application's settings, which the
/// page view uses for a document with no page of its own. On a document
/// that has one (any .docx or .odt) it changed nothing, and no document
/// ever saved what was chosen.
pub(crate) fn apply_page_geometry(buf: &gtk::TextBuffer, page: letters_core::model::PageGeometry) -> bool {
    let Some(live) = crate::live::of(buf) else { return false };
    let applied = live.borrow_mut().edit_with(buf, |doc| {
        let base = doc.page.unwrap_or_default();
        let page = letters_core::model::PageGeometry { columns: base.columns, column_gap_pt: base.column_gap_pt, ..page };
        if doc.page == Some(page) {
            Vec::new()
        } else {
            vec![letters_core::edit::Op::SetPage { page: Some(page) }]
        }
    });
    crate::live::sync_actions(buf);
    applied
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
    prev_btn.set_tooltip_text(Some(&i18n("Previous Match (Shift+Enter)")));
    prev_btn.add_css_class("flat");

    let next_btn = gtk::Button::from_icon_name("go-down-symbolic");
    next_btn.set_tooltip_text(Some(&i18n("Next Match (Enter)")));
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
        let se = search_entry.clone();
        let re = replace_entry.clone();
        replace_btn.connect_clicked(move |_| {
            if let Some(buf) = active_buffer(&tv) {
                let st = state.borrow();
                if !st.matches.is_empty() && st.current_idx < st.matches.len() {
                    let (so, eo) = st.matches[st.current_idx];
                    drop(st);
                    let rep = re.text().to_string();
                    replace_ranges(&buf, &[(so.max(0) as usize, eo.max(0) as usize)], &rep);
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
                let mut ranges = Vec::new();
                let mut cur = buf.start_iter();
                while let Some((ms, me)) = cur.forward_search(
                    &query,
                    gtk::TextSearchFlags::CASE_INSENSITIVE,
                    None,
                ) {
                    ranges.push((ms.offset().max(0) as usize, me.offset().max(0) as usize));
                    cur = me;
                }
                replace_ranges(&buf, &ranges, &rep);
                se.emit_by_name::<()>("search-changed", &[]);
            }
        });
    }

    (search_bar, search_entry)
}

/// Replace each buffer range in `ranges` (in order, not overlapping) with
/// `replacement`, as one model edit (#1202 stage 3): one undo step, and,
/// while tracking changes, recorded like typing. Each replacement takes
/// the style of the text it replaces. They are applied from the last to
/// the first, so every range's offsets still hold when it is reached.
pub(crate) fn replace_ranges(buf: &gtk::TextBuffer, ranges: &[(usize, usize)], replacement: &str) -> bool {
    use letters_core::edit::Op;
    if ranges.is_empty() {
        return false;
    }
    let Some(live) = crate::live::of(buf) else { return false };
    let mut m = live.borrow_mut();
    let mut ops = Vec::new();
    for &(from, to) in ranges.iter().rev() {
        let (s, e) = (m.sequence_offset(from), m.sequence_offset(to));
        if e <= s {
            continue;
        }
        let style = letters_core::edit::slice(m.document(), s, e)
            .and_then(|paras| paras.into_iter().flat_map(|p| p.runs).find(|r| !r.text.is_empty()))
            .map(|r| r.style)
            .unwrap_or_default();
        ops.push(Op::Delete { at: s, len: e - s });
        if !replacement.is_empty() {
            ops.push(Op::Insert {
                at: s,
                content: vec![letters_core::Paragraph {
                    style: Default::default(),
                    runs: vec![letters_core::model::Run { text: replacement.to_string(), style }],
                }],
            });
        }
    }
    !ops.is_empty() && m.apply_user_ops(buf, &ops, false)
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
    let Some(page) = tv.selected_page() else { return };
    if let Some(view) = crate::page_container::find(&page.child()).and_then(|pc| pc.page_view()) {
        crate::page_edit::scroll_to_caret(&view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite_common::gtk_test::run as gtk_test;

    /// Applying a header and footer is one model edit that Undo takes back.
    /// It used to go only onto the buffer, outside the model's history:
    /// Undo then took back the typing before it and left the header.
    #[test]
    fn a_header_and_footer_are_one_undoable_model_edit() {
        gtk_test(|| {
            let (_pc, buf) = crate::doc_tab::make_doc_widget(None);
            let live = crate::live::of(&buf).unwrap();
            buf.place_cursor(&buf.end_iter());
            crate::page_edit::type_text(&buf, "typed");

            apply_header_footer(&buf, "Report", "Page {page}");
            let d = live.borrow_mut().document().clone();
            assert_eq!((d.header.as_deref(), d.footer.as_deref()), (Some("Report"), Some("Page {page}")));

            crate::live::undo(&buf, false);
            let d = live.borrow_mut().document().clone();
            assert_eq!((d.header, d.footer), (None, None), "Undo left the header in place");
            assert_eq!(d.paragraphs[0].text(), "typed", "Undo took back the typing instead");
            assert_eq!(header_footer_of(&buf), (String::new(), String::new()), "the dialog would offer the undone header");

            crate::live::undo(&buf, true);
            let d = live.borrow_mut().document().clone();
            assert_eq!(d.header.as_deref(), Some("Report"));
            assert_eq!(header_footer_of(&buf).0, "Report");
        });
    }

    /// Page Setup sets the open document's page, which the page view lays
    /// out and a save writes; one Undo puts the old page back. It used to
    /// change only the application's settings, so a document with a page
    /// of its own ignored it and none saved it.
    #[test]
    fn page_setup_is_the_documents_page_and_undoes() {
        gtk_test(|| {
            let (pc, buf) = crate::doc_tab::make_doc_widget(None);
            let live = crate::live::of(&buf).unwrap();
            let mut doc = letters_core::Document::from_plain_text("body");
            doc.page = Some(letters_core::model::PageGeometry { columns: 2, ..Default::default() });
            crate::bridge::load_document(&doc, &buf);
            let letter = letters_core::model::PageGeometry { width_pt: 792.0, height_pt: 612.0, margin_left_pt: 36.0, ..Default::default() };

            assert!(apply_page_geometry(&buf, letter));
            let page = live.borrow_mut().document().page.expect("the document has a page");
            assert_eq!((page.width_pt, page.height_pt, page.margin_left_pt), (792.0, 612.0, 36.0));
            assert_eq!(page.columns, 2, "the document's columns are kept");
            assert_eq!(crate::bridge::capture_from_buffer(&buf).page, Some(page), "a save writes it");
            let typeset = crate::doc_tab::typeset_for(&pc, &buf);
            assert_eq!(typeset.tree().pages[0].width_pt, 792.0, "the page view lays it out");

            crate::live::undo(&buf, false);
            assert_eq!(live.borrow_mut().document().page, doc.page, "Undo put the old page back");
            assert!(!apply_page_geometry(&buf, letters_core::model::PageGeometry { columns: 2, ..Default::default() }), "the same page is no edit");
        });
    }
}
