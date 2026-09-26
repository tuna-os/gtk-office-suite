// SPDX-License-Identifier: GPL-3.0-or-later
//
// toc_ui.rs — a table of contents in the window (letters_core::toc).
//
// - `app.insert-toc`: a table of contents before the paragraph at the
//   caret, listing headings 1-3 with their page numbers.
// - `app.update-toc`: regenerate every table of contents from the headings
//   as they are now, page numbers included.
//
// Page numbers come from laying the document out as the tab's Print Layout
// does (`doc_tab::layout_options`), again after the table of contents
// itself has moved headings on (`toc::settle`). Each is one undo step.

use gtk4::{self as gtk, prelude::*};
use libadwaita as adw;
use letters_core::{edit::Op, toc, Document};

/// Each paragraph's page, laid out as the tab at `pc` lays it out.
fn paginator(pc: Option<crate::page_container::PageContainer>) -> impl Fn(&Document) -> Vec<Option<usize>> {
    let opts = pc.as_ref().map(crate::doc_tab::layout_options).unwrap_or_default();
    move |doc: &Document| {
        let mut t = letters_core::layout::pango::Typeset::new(doc.clone(), opts.clone());
        t.set_image_loader(crate::page_view::load_image);
        t.paragraph_pages()
    }
}

/// Apply the table of contents ops `f` makes from the active document and
/// its paginator. `false` if there were none.
pub fn edit(tv: &adw::TabView, f: impl FnOnce(&Document, usize, &dyn Fn(&Document) -> Vec<Option<usize>>) -> Vec<Op>) -> bool {
    let Some(buf) = crate::dialogs::active_buffer(tv) else { return false };
    let Some(live) = crate::live::of(&buf) else { return false };
    let pc = tv.selected_page().and_then(|p| p.child().downcast::<crate::page_container::PageContainer>().ok());
    let paginate = paginator(pc);
    let caret = buf.iter_at_mark(&buf.get_insert()).offset().max(0) as usize;
    let mut live = live.borrow_mut();
    let at = live.sequence_offset(&buf, caret);
    let doc = live.document(&buf).clone();
    let para = letters_core::edit::locate(&doc, at).map_or(0, |(p, _)| p);
    let ops = f(&doc, para, &paginate);
    let done = !ops.is_empty() && live.apply_user_ops(&buf, &ops, false);
    drop(live);
    crate::live::sync_actions(&buf);
    done
}

/// Register the table of contents actions.
pub fn register_actions(app: &adw::Application, tv: &adw::TabView) {
    let action = |name: &str, f: fn(&adw::TabView)| {
        let a = gtk::gio::SimpleAction::new(name, None);
        let tv = tv.clone();
        a.connect_activate(move |_, _| f(&tv));
        app.add_action(&a);
    };
    action("insert-toc", |tv| {
        edit(tv, |doc, para, paginate| toc::settle(doc, toc::insert(doc, para), paginate));
    });
    action("update-toc", |tv| {
        edit(tv, |doc, _, paginate| toc::settle(doc, Vec::new(), paginate));
    });
    suite_common::actions::register_labels(&[
        ("app.insert-toc", &suite_common::i18n("Insert Table of Contents")),
        ("app.update-toc", &suite_common::i18n("Update Table of Contents")),
    ]);
}

#[cfg(test)]
mod tests {
    use gtk4::prelude::*;
    use suite_common::gtk_test::run as gtk_test;

    /// Inserting a table of contents puts the headings' entries, with
    /// their pages, before the caret's paragraph in both views; updating
    /// follows a new heading; each is one undo step.
    #[test]
    fn a_table_of_contents_is_inserted_and_updated() {
        gtk_test(|| {
            let buf = gtk4::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            let live = crate::live::LiveModel::attach(&buf);
            let mut doc = letters_core::Document::from_plain_text("Intro\ntext\nEnd");
            doc.paragraphs[0].style.heading = Some(1);
            doc.paragraphs[2].style.heading = Some(2);
            crate::bridge::load_document(&doc, &buf);
            let paginate = super::paginator(None);
            let apply = |f: &dyn Fn(&letters_core::Document) -> Vec<letters_core::edit::Op>| {
                let d = live.borrow_mut().document(&buf).clone();
                let ops = f(&d);
                assert!(live.borrow_mut().apply_user_ops(&buf, &ops, false));
            };
            apply(&|d| letters_core::toc::settle(d, letters_core::toc::insert(d, 0), &paginate));
            let text = || buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
            assert_eq!(text(), "Intro\t1\nEnd\t1\nIntro\ntext\nEnd");
            let d = live.borrow_mut().document(&buf).clone();
            assert_eq!(crate::bridge::capture_with_starts(&buf).0.paragraphs, d.paragraphs, "Draft reads the entries back");
            assert_eq!(d.paragraphs[1].style.toc, Some(2));
            // A new heading, then an update.
            buf.insert(&mut buf.end_iter(), "\nMore");
            let d = live.borrow_mut().document(&buf).clone();
            let at = letters_core::edit::paragraph_start(&d, 5);
            let restyle = [letters_core::edit::Op::SetParaStyle { at, style: letters_core::ParaStyle { heading: Some(1), ..Default::default() } }];
            assert!(live.borrow_mut().apply_user_ops(&buf, &restyle, false));
            apply(&|d| letters_core::toc::settle(d, Vec::new(), &paginate));
            assert_eq!(text(), "Intro\t1\nEnd\t1\nMore\t1\nIntro\ntext\nEnd\nMore");
            crate::live::undo(&buf, false);
            assert_eq!(text(), "Intro\t1\nEnd\t1\nIntro\ntext\nEnd\nMore");
        });
    }
}
