//! Insert Link (Ctrl+Shift+K): ask for a URL and link the selection to it.
//!
//! It used to replace the selection with the Markdown text "[text](text)",
//! with nothing selected at the document's start, and never asked for a
//! URL. The link is now a model mark (#1202 stage 3): the selection keeps
//! its text and formatting and gains the link; with nothing selected the
//! URL itself is inserted at the caret, linked; an empty URL removes the
//! selection's link. Each is one undo step.

use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use letters_core::edit::{MarkKey, Op};
use letters_core::model::Run;
use letters_core::{Paragraph, RunStyle};

/// Link buffer offsets `from..to` to `url` (`None` unlinks), or with an
/// empty range insert `url` there as linked text. False when the buffer
/// has no live model or there is nothing to do.
pub(crate) fn set_link(buf: &gtk::TextBuffer, from: usize, to: usize, url: Option<&str>) -> bool {
    let Some(live) = crate::live::of(buf) else { return false };
    let mut m = live.borrow_mut();
    let (s, e) = (m.sequence_offset(buf, from), m.sequence_offset(buf, to));
    if e > s {
        let value = RunStyle { link: url.map(str::to_string), ..Default::default() };
        let applied = m.apply_user_ops(buf, &[Op::Mark { start: s, end: e, key: MarkKey::Link, value }], false);
        drop(m);
        // A mark moves no text: the selection is where it was.
        if applied {
            buf.select_range(&buf.iter_at_offset(from as i32), &buf.iter_at_offset(to as i32));
        }
        return applied;
    }
    let Some(url) = url.filter(|u| !u.is_empty()) else { return false };
    let mut run = Run::plain(url);
    run.style.link = Some(url.to_string());
    let applied = m.apply_user_ops(buf, &[Op::Insert { at: s, content: vec![Paragraph { style: Default::default(), runs: vec![run] }] }], false);
    drop(m);
    if applied {
        buf.place_cursor(&buf.iter_at_offset((from + url.chars().count()) as i32));
    }
    applied
}

/// The link on the first character of `from..to`, if any.
fn link_at(buf: &gtk::TextBuffer, from: usize, to: usize) -> Option<String> {
    let live = crate::live::of(buf)?;
    let mut m = live.borrow_mut();
    let (s, e) = (m.sequence_offset(buf, from), m.sequence_offset(buf, to));
    let paras = letters_core::edit::slice(m.document(buf), s, e.max(s + 1))?;
    paras.into_iter().flat_map(|p| p.runs).find(|r| !r.text.is_empty()).and_then(|r| r.style.link)
}

/// Ask for a URL and link the selection, or insert it at the caret.
fn ask(tv: &adw::TabView) {
    let Some(buf) = crate::dialogs::active_buffer(tv) else { return };
    let (from, to) = buf
        .selection_bounds()
        .map(|(a, b)| (a.offset().max(0) as usize, b.offset().max(0) as usize))
        .unwrap_or_else(|| {
            let caret = buf.iter_at_mark(&buf.get_insert()).offset().max(0) as usize;
            (caret, caret)
        });
    let existing = if to > from { link_at(&buf, from, to) } else { None };
    let prompt = suite_common::dialogs::prompt(
        &suite_common::i18n("Insert Link"),
        None,
        &suite_common::i18n("Link address"),
        existing.as_deref().unwrap_or(""),
        &suite_common::i18n("_Link"),
    );
    prompt.entry.set_placeholder_text(Some("https://"));
    let parent = tv.root();
    let tv = tv.clone();
    prompt.present(parent.as_ref(), move |url| {
        if let Some(url) = url {
            let url = url.trim();
            set_link(&buf, from, to, Some(url).filter(|u| !u.is_empty()));
            crate::live::sync_actions(&buf);
        }
        crate::dialogs::focus_active_view(&tv);
    });
}

/// Register `app.insertlink`.
pub fn register_actions(app: &adw::Application, tv: &adw::TabView) {
    let a = gtk::gio::SimpleAction::new("insertlink", None);
    let tv = tv.clone();
    a.connect_activate(move |_, _| ask(&tv));
    app.add_action(&a);
    // Ctrl+K belongs to the command palette (DESIGN-UI.md).
    app.set_accels_for_action("app.insertlink", &["<Primary><Shift>k"]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite_common::gtk_test::run as gtk_test;

    fn live_buffer(text: &str) -> gtk::TextBuffer {
        let buf = gtk::TextBuffer::new(None);
        crate::actions::register_formatting_tags(&buf);
        crate::bridge::render_to_buffer(&letters_core::Document::from_plain_text(text), &buf);
        let live = crate::live::LiveModel::attach(&buf);
        let _ = live.borrow_mut().document(&buf);
        buf
    }

    fn runs(buf: &gtk::TextBuffer) -> Vec<(String, Option<String>)> {
        let live = crate::live::of(buf).unwrap();
        let mut m = live.borrow_mut();
        m.document(buf).paragraphs[0].runs.iter().map(|r| (r.text.clone(), r.style.link.clone())).collect()
    }

    fn reads(buf: &gtk::TextBuffer) -> (usize, usize) {
        let live = crate::live::of(buf).unwrap();
        let m = live.borrow();
        (m.local_reads, m.full_reads)
    }

    #[test]
    fn a_selection_is_linked_in_place_and_unlinked_with_no_url() {
        gtk_test(|| {
            let buf = live_buffer("see gnome here");
            let before = reads(&buf);
            assert!(set_link(&buf, 4, 9, Some("https://gnome.org")));
            assert_eq!(
                runs(&buf),
                vec![("see ".into(), None), ("gnome".into(), Some("https://gnome.org".into())), (" here".into(), None)]
            );
            assert_eq!(reads(&buf), before, "the model read the buffer back instead of taking an op");
            let (a, b) = buf.selection_bounds().expect("the selection is kept");
            assert_eq!((a.offset(), b.offset()), (4, 9));
            assert_eq!(link_at(&buf, 4, 9).as_deref(), Some("https://gnome.org"), "the dialog offers the link to edit");

            assert!(set_link(&buf, 4, 9, None));
            assert_eq!(runs(&buf), vec![("see gnome here".into(), None)]);
            crate::live::undo(&buf, false);
            crate::live::undo(&buf, false);
            assert_eq!(runs(&buf), vec![("see gnome here".into(), None)]);
        });
    }

    #[test]
    fn with_nothing_selected_the_url_goes_in_at_the_caret() {
        gtk_test(|| {
            let buf = live_buffer("ab");
            assert!(set_link(&buf, 1, 1, Some("https://x.org")));
            assert_eq!(
                runs(&buf),
                vec![("a".into(), None), ("https://x.org".into(), Some("https://x.org".into())), ("b".into(), None)]
            );
            assert_eq!(buf.iter_at_mark(&buf.get_insert()).offset(), 14);
            crate::live::undo(&buf, false);
            assert_eq!(runs(&buf), vec![("ab".into(), None)]);
        });
    }
}
