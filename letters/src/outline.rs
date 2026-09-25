// SPDX-License-Identifier: GPL-3.0-or-later
//
// outline.rs — the document outline sidebar (DESIGN-UI, Docs: "a document
// outline sidebar built from headings").
//
// An AdwOverlaySplitView at the start of the window. Its list is the active
// document's headings (and Title/Subtitle), read from the live model with
// `letters_core::review::table_of_contents` — the same entries a table of
// contents uses — indented by level. It follows edits in either view, marks
// the section the caret is in, and a click (or Enter) moves the caret to
// that heading in the visible view.

use gtk4::{self as gtk, prelude::*};
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;

/// One outline row per heading: (level, text) is what is shown.
type Entries = Vec<(u8, String, usize)>;

fn row(level: u8, title: &str) -> gtk::ListBoxRow {
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .margin_start(i32::from(level.clamp(1, 6) - 1) * 14)
        .build();
    if level == 1 {
        label.add_css_class("heading");
    }
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&label));
    row.set_tooltip_text(Some(title));
    row
}

/// The index of the section containing buffer offset `caret`.
fn section_at(entries: &Entries, caret: usize) -> Option<usize> {
    entries.iter().rposition(|(_, _, start)| *start <= caret)
}

/// The split view holding `content` with the outline as its sidebar
/// (hidden), plus a header toggle and the app action `toggle-outline`.
pub fn build(
    tv: &adw::TabView,
    content: &impl IsA<gtk::Widget>,
    header: &adw::HeaderBar,
    app: &adw::Application,
    breakpoints: &[&adw::Breakpoint],
) -> adw::OverlaySplitView {
    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    list.update_property(&[gtk::accessible::Property::Label("Outline")]);
    let empty = adw::StatusPage::builder()
        .title("No Headings")
        .description("Headings you add appear here.")
        .icon_name("view-list-symbolic")
        .build();
    empty.add_css_class("compact");
    let stack = gtk::Stack::new();
    let scroll = gtk::ScrolledWindow::builder().child(&list).vexpand(true).build();
    stack.add_named(&scroll, Some("list"));
    stack.add_named(&empty, Some("empty"));
    let sidebar = adw::ToolbarView::new();
    let title = adw::HeaderBar::builder()
        .title_widget(&adw::WindowTitle::new("Outline", ""))
        .show_end_title_buttons(false)
        .show_start_title_buttons(false)
        .build();
    sidebar.add_top_bar(&title);
    sidebar.set_content(Some(&stack));

    let split = adw::OverlaySplitView::builder()
        .sidebar_position(gtk::PackType::Start)
        .show_sidebar(false)
        .min_sidebar_width(200.0)
        .max_sidebar_width(300.0)
        .content(content)
        .sidebar(&sidebar)
        .build();
    for b in breakpoints {
        b.add_setter(&split, "collapsed", Some(&true.to_value()));
    }

    let show = gtk::ToggleButton::builder()
        .icon_name("sidebar-show-symbolic")
        .tooltip_text(suite_common::i18n("Outline"))
        .build();
    show.update_property(&[gtk::accessible::Property::Label(&suite_common::i18n("Outline"))]);
    show.bind_property("active", &split, "show-sidebar").bidirectional().sync_create().build();
    header.pack_start(&show);
    {
        let a = gtk::gio::SimpleAction::new("toggle-outline", None);
        let show = show.clone();
        a.connect_activate(move |_, _| show.set_active(!show.is_active()));
        app.add_action(&a);
        app.set_accels_for_action("app.toggle-outline", &["<Primary><Alt>o"]);
        suite_common::actions::register_labels(&[("app.toggle-outline", &suite_common::i18n("Show Outline"))]);
    }

    let entries: Rc<RefCell<Entries>> = Rc::default();
    let refresh = {
        let (tv, list, stack, entries, split) = (tv.clone(), list.clone(), stack.clone(), entries.clone(), split.clone());
        Rc::new(move |changed: bool| {
            // Only an open outline pays for reading the headings.
            if !split.shows_sidebar() {
                return;
            }
            let Some(buf) = crate::dialogs::active_buffer(&tv) else { return };
            let Some(live) = crate::live::of(&buf) else { return };
            if changed {
                let now = live.borrow_mut().outline(&buf);
                let same = |a: &Entries, b: &Entries| a.iter().map(|e| (e.0, &e.1)).eq(b.iter().map(|e| (e.0, &e.1)));
                if !same(&now, &entries.borrow()) {
                    while let Some(r) = list.first_child() {
                        list.remove(&r);
                    }
                    for (level, title, _) in &now {
                        list.append(&row(*level, title));
                    }
                    stack.set_visible_child_name(if now.is_empty() { "empty" } else { "list" });
                }
                *entries.borrow_mut() = now;
            }
            let caret = buf.iter_at_mark(&buf.get_insert()).offset().max(0) as usize;
            let current = section_at(&entries.borrow(), caret).and_then(|i| list.row_at_index(i as i32));
            list.select_row(current.as_ref());
        })
    };
    {
        let refresh = refresh.clone();
        crate::dialogs::watch_active_buffer(tv, move |_, changed| refresh(changed));
    }
    // A closed sidebar is not part of the window (it would stay in the
    // accessibility tree otherwise); opening reads the headings afresh.
    sidebar.set_visible(false);
    split.connect_show_sidebar_notify(move |s| {
        sidebar.set_visible(s.shows_sidebar());
        refresh(true);
    });
    {
        let (tv, entries) = (tv.clone(), entries.clone());
        list.connect_row_activated(move |_, row| {
            let Some(buf) = crate::dialogs::active_buffer(&tv) else { return };
            let Some(start) = entries.borrow().get(row.index().max(0) as usize).map(|e| e.2) else { return };
            buf.place_cursor(&buf.iter_at_offset(start as i32));
            crate::dialogs::focus_active_view(&tv);
        });
    }
    split
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite_common::gtk_test::run as gtk_test;

    #[test]
    fn the_caret_is_in_the_last_heading_before_it() {
        let e: Entries = vec![(1, "A".into(), 0), (2, "B".into(), 10), (1, "C".into(), 30)];
        assert_eq!(section_at(&e, 0), Some(0));
        assert_eq!(section_at(&e, 29), Some(1));
        assert_eq!(section_at(&e, 99), Some(2));
        assert_eq!(section_at(&e[1..].to_vec(), 5), None);
    }

    #[test]
    fn the_outline_is_the_models_headings_and_follows_style_changes() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            let live = crate::live::LiveModel::attach(&buf);
            let mut doc = letters_core::Document::from_plain_text("Intro\nbody\nDetails\nmore");
            doc.paragraphs[0].style.heading = Some(1);
            crate::bridge::load_document(&doc, &buf);
            let outline = |buf: &gtk::TextBuffer| live.borrow_mut().outline(buf);
            assert_eq!(outline(&buf), [(1, "Intro".to_string(), 0)]);
            // A heading made with the style picker appears, at its buffer offset.
            let details = 11;
            buf.place_cursor(&buf.iter_at_offset(details + 2));
            assert!(crate::style_picker::apply(&buf, "Heading 2"));
            assert_eq!(outline(&buf), [(1, "Intro".to_string(), 0), (2, "Details".to_string(), details as usize)]);
        });
    }
}
