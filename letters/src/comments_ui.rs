// SPDX-License-Identifier: GPL-3.0-or-later
//
// comments_ui.rs — comments in the window (letters_core::comments).
//
// - `app.add-comment` (Ctrl+Alt+M): comment on the selection, or on the
//   word at the caret, by the user (`review_ui::author`).
// - `app.next-comment`, and `app.resolve-comment` / `app.delete-comment`
//   for the thread at the caret.
// - The sidebar's Comments view lists the threads in document order: the
//   text each is on, its comments, a reply field, Resolve (or Reopen) and
//   Delete. The page shows open threads' text tinted, with a mark in the
//   right margin (`page_view`); clicking a mark opens this view.
//
// Every change is a model op (letters_core::edit::Op), one undo step.

use adw::prelude::*;
use gtk4::{self as gtk, glib};
use libadwaita as adw;
use letters_core::comments::{self, Thread};

/// The buffer range a new comment is on: the selection, or the word at the
/// caret. None when there is neither.
fn target(buf: &gtk::TextBuffer) -> Option<(usize, usize)> {
    if let Some((a, b)) = buf.selection_bounds() {
        return Some((a.offset() as usize, b.offset() as usize));
    }
    let mut s = buf.iter_at_mark(&buf.get_insert());
    if !s.inside_word() && !s.ends_word() {
        return None;
    }
    if !s.starts_word() {
        s.backward_word_start();
    }
    let mut e = s;
    e.forward_word_end();
    (e.offset() > s.offset()).then(|| (s.offset() as usize, e.offset() as usize))
}

/// Comment `text` on buffer range `a..b` of `buf`, as the user. The new
/// thread's id.
pub fn add(buf: &gtk::TextBuffer, a: usize, b: usize, text: &str) -> Option<u32> {
    let live = crate::live::of(buf)?;
    let mut live = live.borrow_mut();
    let (sa, sb) = (live.sequence_offset(buf, a), live.sequence_offset(buf, b));
    let mut id = None;
    live.edit_comments(buf, |doc| {
        let (ops, new) = comments::add(doc, sa, sb, &crate::review_ui::author(), &letters_core::track::now(), text).unwrap_or_default();
        id = (!ops.is_empty()).then_some(new);
        ops
    });
    drop(live);
    // The text is as it was: keep the selection on it.
    buf.select_range(&buf.iter_at_offset(b as i32), &buf.iter_at_offset(a as i32));
    id
}

/// Apply `f`'s comment ops to `buf`'s model.
fn edit(buf: &gtk::TextBuffer, f: impl FnOnce(&letters_core::Document) -> Vec<letters_core::edit::Op>) {
    if let Some(live) = crate::live::of(buf) {
        live.borrow_mut().edit_comments(buf, f);
    }
    crate::live::sync_actions(buf);
}

/// Ask for a comment and add it on the selection (or the word at the caret).
fn ask(tv: &adw::TabView) {
    let Some(buf) = crate::dialogs::active_buffer(tv) else { return };
    let Some((a, b)) = target(&buf) else { return };
    let quoted: String = buf.text(&buf.iter_at_offset(a as i32), &buf.iter_at_offset(b as i32), false).chars().take(80).collect();
    let dialog = adw::AlertDialog::new(Some(&suite_common::i18n("Add Comment")), Some(&format!("“{}”", quoted.trim())));
    let entry = gtk::Entry::builder().placeholder_text(suite_common::i18n("Comment")).activates_default(true).build();
    entry.update_property(&[gtk::accessible::Property::Label(&suite_common::i18n("Comment text"))]);
    dialog.set_extra_child(Some(&entry));
    dialog.add_responses(&[("cancel", &suite_common::i18n("_Cancel")), ("comment", &suite_common::i18n("_Comment"))]);
    dialog.set_default_response(Some("comment"));
    dialog.set_response_appearance("comment", adw::ResponseAppearance::Suggested);
    let parent = tv.root().and_downcast::<gtk::Window>();
    let tv = tv.clone();
    let field = entry.clone();
    dialog.clone().choose(parent.as_ref(), None::<&gtk::gio::Cancellable>, move |response| {
        let text = entry.text().trim().to_string();
        if response.as_str() == "comment" && !text.is_empty() {
            add(&buf, a, b, &text);
            crate::live::sync_actions(&buf);
        }
        crate::dialogs::focus_active_view(&tv);
    });
    // Type straight into it.
    dialog.set_focus(Some(&field));
}

/// The thread whose text holds the caret, if any.
fn thread_at_caret(buf: &gtk::TextBuffer) -> Option<Thread> {
    let caret = buf.iter_at_mark(&buf.get_insert()).offset().max(0) as usize;
    let live = crate::live::of(buf)?;
    let threads = live.borrow_mut().comment_threads(buf);
    threads.into_iter().find(|(_, r)| r.is_some_and(|(a, b)| a <= caret && caret <= b)).map(|(t, _)| t)
}

/// Register the comment actions.
pub fn register_actions(app: &adw::Application, tv: &adw::TabView) {
    let action = |name: &str, f: Box<dyn Fn(&adw::TabView)>| {
        let a = gtk::gio::SimpleAction::new(name, None);
        let tv = tv.clone();
        a.connect_activate(move |_, _| f(&tv));
        app.add_action(&a);
    };
    action("add-comment", Box::new(ask));
    action(
        "next-comment",
        Box::new(|tv| {
            let Some(buf) = crate::dialogs::active_buffer(tv) else { return };
            let Some(live) = crate::live::of(&buf) else { return };
            let threads = live.borrow_mut().comment_threads(&buf);
            let starts: Vec<usize> = threads.iter().filter(|(t, _)| !t.comment.resolved).filter_map(|(_, r)| r.map(|r| r.0)).collect();
            let caret = buf.iter_at_mark(&buf.get_insert()).offset().max(0) as usize;
            if let Some(at) = starts.iter().find(|s| **s > caret).or(starts.first()) {
                buf.place_cursor(&buf.iter_at_offset(*at as i32));
                crate::dialogs::focus_active_view(tv);
            }
        }),
    );
    for (name, resolve) in [("resolve-comment", true), ("delete-comment", false)] {
        action(
            name,
            Box::new(move |tv| {
                let Some(buf) = crate::dialogs::active_buffer(tv) else { return };
                let Some(t) = thread_at_caret(&buf) else { return };
                let id = t.comment.id;
                edit(&buf, |doc| if resolve { comments::set_resolved(doc, id, true) } else { comments::delete(doc, id) });
            }),
        );
    }
    app.set_accels_for_action("app.add-comment", &["<Primary><Alt>m"]);
    suite_common::actions::register_labels(&[
        ("app.add-comment", &suite_common::i18n("Add Comment")),
        ("app.next-comment", &suite_common::i18n("Next Comment")),
        ("app.resolve-comment", &suite_common::i18n("Resolve Comment")),
        ("app.delete-comment", &suite_common::i18n("Delete Comment")),
    ]);
}

/// "Author · date" over a comment's text.
fn comment_box(c: &letters_core::Comment) -> gtk::Box {
    let who = gtk::Label::builder()
        .label(format!("{} · {}", c.author, c.date.replace('T', " ").trim_end_matches('Z')))
        .xalign(0.0)
        .wrap(true)
        .build();
    who.add_css_class("caption-heading");
    let body = gtk::Label::builder().label(&c.text).xalign(0.0).wrap(true).wrap_mode(gtk4::pango::WrapMode::WordChar).build();
    let b = gtk::Box::new(gtk::Orientation::Vertical, 2);
    b.append(&who);
    b.append(&body);
    b
}

/// A thread's row: the text it is on, its comments, a reply field, and
/// Resolve (or Reopen) and Delete.
fn thread_row(buf: &gtk::TextBuffer, t: &Thread) -> gtk::ListBoxRow {
    let id = t.comment.id;
    let summary = format!("{}: {}", t.comment.author, t.comment.text);
    let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let quote = match &t.anchor {
        Some(a) => format!("“{}”", a.text.replace(letters_core::layout::OBJECT, "▫").chars().take(60).collect::<String>().trim()),
        None => suite_common::i18n("The text was deleted"),
    };
    let quote = gtk::Label::builder().label(&quote).xalign(0.0).wrap(true).build();
    quote.add_css_class("dim-label");
    column.append(&quote);
    column.append(&comment_box(&t.comment));
    for r in &t.replies {
        let reply = comment_box(r);
        reply.set_margin_start(12);
        column.append(&reply);
    }
    if !t.comment.resolved {
        let entry = gtk::Entry::builder().placeholder_text(suite_common::i18n("Reply…")).build();
        entry.update_property(&[gtk::accessible::Property::Label(&format!("{} {summary}", suite_common::i18n("Reply to")))]);
        let buf = buf.clone();
        entry.connect_activate(move |e| {
            let text = e.text().trim().to_string();
            if !text.is_empty() {
                edit(&buf, |doc| {
                    comments::reply(doc, id, &crate::review_ui::author(), &letters_core::track::now(), &text).map(|(ops, _)| ops).unwrap_or_default()
                });
            }
        });
        column.append(&entry);
    }
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let resolved = t.comment.resolved;
    let (label, icon) = if resolved { ("Reopen", "edit-undo-symbolic") } else { ("Resolve", "object-select-symbolic") };
    for (label, icon, delete) in [(label, icon, false), ("Delete", "user-trash-symbolic", true)] {
        let b = gtk::Button::from_icon_name(icon);
        b.add_css_class("flat");
        b.set_tooltip_text(Some(&suite_common::i18n(label)));
        b.update_property(&[gtk::accessible::Property::Label(&format!("{}: {summary}", suite_common::i18n(label)))]);
        let buf = buf.clone();
        b.connect_clicked(move |_| edit(&buf, |doc| if delete { comments::delete(doc, id) } else { comments::set_resolved(doc, id, !resolved) }));
        buttons.append(&b);
    }
    column.append(&buttons);
    if resolved {
        column.add_css_class("dim-label");
    }
    column.set_margin_top(6);
    column.set_margin_bottom(6);
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&column));
    row.update_property(&[gtk::accessible::Property::Label(&summary)]);
    row
}

/// The sidebar's Comments view: the active document's comment threads.
pub fn comments_view(tv: &adw::TabView) -> gtk::Widget {
    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    list.set_selection_mode(gtk::SelectionMode::None);
    list.update_property(&[gtk::accessible::Property::Label("Comments")]);
    let scroll = gtk::ScrolledWindow::builder().child(&list).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
    let none = adw::StatusPage::builder()
        .title("No Comments")
        .description("Select some text and add a comment (Ctrl+Alt+M).")
        .icon_name("chat-bubble-text-symbolic")
        .build();
    none.add_css_class("compact");
    let stack = gtk::Stack::new();
    stack.add_named(&scroll, Some("list"));
    stack.add_named(&none, Some("none"));
    let starts: std::rc::Rc<std::cell::RefCell<Vec<Option<usize>>>> = Default::default();

    let refresh = {
        let (tv, list, stack, starts) = (tv.clone(), list.clone(), stack.clone(), starts.clone());
        std::rc::Rc::new(move || {
            if !stack.is_mapped() {
                return;
            }
            let Some(buf) = crate::dialogs::active_buffer(&tv) else { return };
            let Some(live) = crate::live::of(&buf) else { return };
            let threads = live.borrow_mut().comment_threads(&buf);
            while let Some(r) = list.first_child() {
                list.remove(&r);
            }
            for (t, _) in &threads {
                list.append(&thread_row(&buf, t));
            }
            *starts.borrow_mut() = threads.iter().map(|(_, r)| r.map(|r| r.0)).collect();
            stack.set_visible_child_name(if threads.is_empty() { "none" } else { "list" });
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
    stack.connect_map(move |_| refresh());
    {
        let tv = tv.clone();
        list.set_activate_on_single_click(true);
        list.connect_row_activated(move |_, row| {
            let Some(buf) = crate::dialogs::active_buffer(&tv) else { return };
            if let Some(Some(at)) = starts.borrow().get(row.index().max(0) as usize) {
                buf.place_cursor(&buf.iter_at_offset(*at as i32));
            }
            crate::dialogs::focus_active_view(&tv);
        });
    }
    stack.upcast()
}

/// Open the sidebar on the Comments view (a margin mark was clicked).
pub fn show(widget: &impl IsA<gtk::Widget>) {
    let _ = widget.activate_action("app.show-comments", None::<&glib::Variant>);
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

    /// A comment is a tag in the Draft view and a thread in the model, both
    /// read back the same; replying, resolving and deleting are one undo
    /// step each, and survive a whole-buffer re-read.
    #[test]
    fn comments_are_marks_both_views_agree_on() {
        gtk_test(|| {
            let buf = tab("one two three");
            let id = add(&buf, 4, 7, "Which two?").expect("added");
            let live = crate::live::of(&buf).unwrap();
            let threads = live.borrow_mut().comment_threads(&buf);
            assert_eq!(threads.len(), 1);
            assert_eq!((threads[0].0.anchor.as_ref().unwrap().text.as_str(), threads[0].1), ("two", Some((4, 7))));
            // Typing inside the comment is in it; the Draft view agrees.
            buf.insert(&mut buf.iter_at_offset(5), "w");
            let doc = live.borrow_mut().document(&buf).clone();
            assert_eq!(crate::bridge::capture_with_starts(&buf).0, doc);
            assert_eq!(comments::thread(&doc, id).unwrap().anchor.unwrap().text, "twwo");
            edit(&buf, |d| comments::reply(d, id, "Ada", "2026-09-26T10:00:00Z", "The second").unwrap().0);
            edit(&buf, |d| comments::set_resolved(d, id, true));
            let doc = live.borrow_mut().document(&buf).clone();
            let t = comments::thread(&doc, id).unwrap();
            assert!(t.comment.resolved && t.replies.len() == 1);
            crate::live::undo(&buf, false);
            crate::live::undo(&buf, false);
            let doc = live.borrow_mut().document(&buf).clone();
            let t = comments::thread(&doc, id).unwrap();
            assert!(!t.comment.resolved && t.replies.is_empty(), "undone: {t:?}");
            edit(&buf, |d| comments::delete(d, id));
            let doc = live.borrow_mut().document(&buf).clone();
            assert!(doc.comments.is_empty() && doc.paragraphs[0].runs.iter().all(|r| r.style.comments.is_empty()));
            assert_eq!(buf.text(&buf.start_iter(), &buf.end_iter(), false), "one twwo three");
        });
    }
}
