// SPDX-License-Identifier: GPL-3.0-or-later
//
// review_ui.rs — track changes in the window (letters_core::track).
//
// - `app.track-changes` (Ctrl+Alt+T), a toggle: while on, every edit made
//   on the page is recorded as a tracked change by the user (an insertion
//   underlined, a deletion struck through, in the author's colour). The
//   status bar says so. The Draft editor is read-only meanwhile: its edits
//   go straight into the buffer, not through the model's tracked ops.
// - Accept or reject the change at the caret (or every change touching the
//   selection), every change at once, or go to the next change.
// - The sidebar's Changes view lists them, each with Accept and Reject.
//
// Accepting and rejecting are model ops, each one undo step.

use gtk4::{self as gtk, glib, prelude::*};
use libadwaita as adw;
use std::cell::Cell;

thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
}

/// Whether tracking is on (new tabs start as the window is).
pub fn tracking() -> bool {
    TRACKING.with(Cell::get)
}

/// The name tracked changes are recorded under: the user's real name, else
/// their login.
pub fn author() -> String {
    let real = glib::real_name().to_string_lossy().trim().to_string();
    if real.is_empty() || real == "Unknown" {
        glib::user_name().to_string_lossy().to_string()
    } else {
        real
    }
}

/// Make `buf`'s model record tracked changes (or not), and its Draft
/// `editor` read-only while it does.
pub fn apply_to(buf: &gtk::TextBuffer, editor: Option<&gtk::TextView>, on: bool) {
    if let Some(live) = crate::live::of(buf) {
        live.borrow_mut().tracking = on.then(author);
    }
    if let Some(editor) = editor {
        editor.set_editable(!on);
        editor.set_tooltip_text(on.then_some("Tracking changes: edit in Print Layout"));
    }
}

fn caret_range(buf: &gtk::TextBuffer) -> (usize, usize) {
    let at = |m: &gtk::TextMark| buf.iter_at_mark(m).offset().max(0) as usize;
    let (a, b) = (at(&buf.get_insert()), at(&buf.selection_bound()));
    (a.min(b), a.max(b))
}

/// Register the review actions, and put the tracking indicator in
/// `status_bar`.
pub fn register_actions(app: &adw::Application, tv: &adw::TabView, status_bar: &gtk::Box) {
    let indicator = gtk::Label::new(Some(&suite_common::i18n("Tracking changes")));
    indicator.add_css_class("caption");
    indicator.add_css_class("accent");
    indicator.set_margin_start(12);
    indicator.set_visible(false);
    status_bar.append(&indicator);

    let toggle = gtk::gio::SimpleAction::new_stateful("track-changes", None, &false.to_variant());
    {
        let tv = tv.clone();
        toggle.connect_change_state(move |a, state| {
            let Some(on) = state.and_then(|v| v.get::<bool>()) else { return };
            a.set_state(&on.to_variant());
            TRACKING.with(|t| t.set(on));
            indicator.set_visible(on);
            for i in 0..tv.n_pages() {
                let child = tv.nth_page(i).child();
                if let Some(editor) = crate::dialogs::get_textview(&child) {
                    apply_to(&editor.buffer(), Some(&editor), on);
                }
            }
        });
    }
    app.add_action(&toggle);

    let simple = |name: &str, f: Box<dyn Fn(&gtk::TextBuffer)>| {
        let a = gtk::gio::SimpleAction::new(name, None);
        let tv = tv.clone();
        a.connect_activate(move |_, _| {
            if let Some(buf) = crate::dialogs::active_buffer(&tv) {
                f(&buf);
                crate::live::sync_actions(&buf);
            }
        });
        app.add_action(&a);
    };
    for accept in [true, false] {
        simple(
            if accept { "accept-change" } else { "reject-change" },
            Box::new(move |buf| {
                let (a, b) = caret_range(buf);
                if let Some(live) = crate::live::of(buf) {
                    live.borrow_mut().resolve_changes(buf, a, b, accept);
                }
            }),
        );
        simple(
            if accept { "accept-all-changes" } else { "reject-all-changes" },
            Box::new(move |buf| {
                if let Some(live) = crate::live::of(buf) {
                    live.borrow_mut().resolve_all_changes(buf, accept);
                }
            }),
        );
    }
    {
        let tv2 = tv.clone();
        simple(
            "next-change",
            Box::new(move |buf| {
                let Some(live) = crate::live::of(buf) else { return };
                let changes = live.borrow_mut().changes(buf);
                let caret = caret_range(buf).1;
                let next = changes.iter().find(|(_, at)| *at > caret).or(changes.first());
                if let Some((_, at)) = next {
                    buf.place_cursor(&buf.iter_at_offset(*at as i32));
                    crate::dialogs::focus_active_view(&tv2);
                }
            }),
        );
    }
    app.set_accels_for_action("app.track-changes", &["<Primary><Alt>t"]);
    suite_common::actions::register_labels(&[
        ("app.track-changes", &suite_common::i18n("Track Changes")),
        ("app.accept-change", &suite_common::i18n("Accept Change")),
        ("app.reject-change", &suite_common::i18n("Reject Change")),
        ("app.accept-all-changes", &suite_common::i18n("Accept All Changes")),
        ("app.reject-all-changes", &suite_common::i18n("Reject All Changes")),
        ("app.next-change", &suite_common::i18n("Next Change")),
    ]);
}

/// A change's row: who did what, when, and to what text; Accept and Reject.
fn change_row(buf: &gtk::TextBuffer, change: &letters_core::track::Change, at: usize) -> gtk::ListBoxRow {
    let rev = &change.revision;
    let did = match rev.kind {
        letters_core::RevisionKind::Insert => "inserted",
        letters_core::RevisionKind::Delete => "deleted",
    };
    let text: String = change.text.replace(letters_core::layout::OBJECT, "▫").chars().take(60).collect();
    let summary = format!("{} {did} “{}”", rev.author, text.trim());
    let title = gtk::Label::builder().label(&summary).xalign(0.0).wrap(true).build();
    let when = gtk::Label::builder().label(rev.date.replace('T', " ").trim_end_matches('Z')).xalign(0.0).build();
    when.add_css_class("caption");
    when.add_css_class("dim-label");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    for (accept, label, icon) in [(true, "Accept", "object-select-symbolic"), (false, "Reject", "window-close-symbolic")] {
        let b = gtk::Button::from_icon_name(icon);
        b.add_css_class("flat");
        b.set_tooltip_text(Some(label));
        b.update_property(&[gtk::accessible::Property::Label(&format!("{label}: {summary}"))]);
        let buf = buf.clone();
        b.connect_clicked(move |_| {
            if let Some(live) = crate::live::of(&buf) {
                live.borrow_mut().resolve_changes(&buf, at, at, accept);
            }
            crate::live::sync_actions(&buf);
        });
        buttons.append(&b);
    }
    let column = gtk::Box::new(gtk::Orientation::Vertical, 2);
    column.append(&title);
    column.append(&when);
    column.append(&buttons);
    column.set_margin_top(4);
    column.set_margin_bottom(4);
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&column));
    row.update_property(&[gtk::accessible::Property::Label(&summary)]);
    row
}

/// The sidebar's Changes view: the active document's tracked changes.
pub fn changes_view(tv: &adw::TabView) -> gtk::Widget {
    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    list.set_selection_mode(gtk::SelectionMode::None);
    list.update_property(&[gtk::accessible::Property::Label("Tracked changes")]);
    let scroll = gtk::ScrolledWindow::builder().child(&list).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
    let none = adw::StatusPage::builder()
        .title("No Tracked Changes")
        .description("Turn on Track Changes (Ctrl+Alt+T) to record edits for review.")
        .icon_name("document-edit-symbolic")
        .build();
    none.add_css_class("compact");
    let stack = gtk::Stack::new();
    stack.add_named(&scroll, Some("list"));
    stack.add_named(&none, Some("none"));
    let offsets: std::rc::Rc<std::cell::RefCell<Vec<usize>>> = Default::default();

    let refresh = {
        let (tv, list, stack, offsets) = (tv.clone(), list.clone(), stack.clone(), offsets.clone());
        std::rc::Rc::new(move || {
            if !stack.is_mapped() {
                return;
            }
            let Some(buf) = crate::dialogs::active_buffer(&tv) else { return };
            let Some(live) = crate::live::of(&buf) else { return };
            let changes = live.borrow_mut().changes(&buf);
            while let Some(r) = list.first_child() {
                list.remove(&r);
            }
            for (change, at) in &changes {
                list.append(&change_row(&buf, change, *at));
            }
            *offsets.borrow_mut() = changes.iter().map(|(_, at)| *at).collect();
            stack.set_visible_child_name(if changes.is_empty() { "none" } else { "list" });
        })
    };
    {
        let refresh = refresh.clone();
        crate::dialogs::watch_active_buffer(tv, move |_, changed| {
            if changed {
                refresh();
            }
        });
    }
    {
        let refresh = refresh.clone();
        stack.connect_map(move |_| refresh());
    }
    {
        let tv = tv.clone();
        list.set_activate_on_single_click(true);
        list.connect_row_activated(move |_, row| {
            let Some(buf) = crate::dialogs::active_buffer(&tv) else { return };
            if let Some(at) = offsets.borrow().get(row.index().max(0) as usize) {
                buf.place_cursor(&buf.iter_at_offset(*at as i32));
            }
            crate::dialogs::focus_active_view(&tv);
        });
    }
    stack.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite_common::gtk_test::run as gtk_test;

    fn tab(text: &str) -> gtk::TextBuffer {
        let buf = gtk::TextBuffer::new(None);
        crate::actions::register_formatting_tags(&buf);
        crate::live::LiveModel::attach(&buf);
        crate::bridge::load_document(&letters_core::Document::from_plain_text(text), &buf);
        buf
    }

    /// Typing and deleting on the page while tracking are recorded as
    /// changes the Draft view shows and reads back; accepting and rejecting
    /// are single undo steps.
    #[test]
    fn tracked_page_edits_are_marks_both_views_agree_on() {
        gtk_test(|| {
            let buf = tab("one two three");
            apply_to(&buf, None, true);
            buf.place_cursor(&buf.iter_at_offset(3));
            crate::page_edit::type_text(&buf, " and a half");
            let live = crate::live::of(&buf).unwrap();
            // Delete "two " on the page: the text stays, marked.
            let ops = [letters_core::edit::Op::Delete { at: 15, len: 4 }];
            assert!(live.borrow_mut().apply_user_ops(&buf, &ops, false));
            let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
            assert_eq!(text, "one and a half two three");
            let changes = live.borrow_mut().changes(&buf);
            let kinds: Vec<_> = changes.iter().map(|(c, at)| (c.revision.kind, c.text.clone(), *at)).collect();
            assert_eq!(
                kinds,
                [
                    (letters_core::RevisionKind::Insert, " and a half".to_string(), 3),
                    (letters_core::RevisionKind::Delete, "two ".to_string(), 15)
                ]
            );
            // Draft reads the same document back (tags carry the revisions).
            let doc = live.borrow_mut().document(&buf).clone();
            assert_eq!(crate::bridge::capture_with_starts(&buf).0, doc);
            // Accept the deletion at its offset; reject everything else.
            assert!(live.borrow_mut().resolve_changes(&buf, 16, 16, true));
            assert!(live.borrow_mut().resolve_all_changes(&buf, false));
            assert_eq!(buf.text(&buf.start_iter(), &buf.end_iter(), false), "one three");
            crate::live::undo(&buf, false);
            crate::live::undo(&buf, false);
            assert_eq!(buf.text(&buf.start_iter(), &buf.end_iter(), false), "one and a half two three");
        });
    }
}
