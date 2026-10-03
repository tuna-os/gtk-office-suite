//! Inserting content at the caret: a suite fragment pasted from the
//! clipboard, an image chosen with Insert Image.
//!
//! On a tab's document both are one model edit (#1202 stage 3): the
//! selection, if any, is replaced; the formatting, images and paragraph
//! styles of what is inserted go into the model as they are rather than
//! through buffer tags read back; and one undo takes it all out again.

use gtk4 as gtk;
use gtk4::prelude::*;
use letters_core::edit::Op;
use letters_core::fragment::Fragment;
use letters_core::model::Run;
use letters_core::Paragraph;

/// Replace the selection, or insert at the caret, with `content`. False
/// when the buffer has no live model or the model refused the edit.
fn replace_selection(buf: &gtk::TextBuffer, content: Vec<Paragraph>) -> bool {
    let Some(live) = crate::live::of(buf) else { return false };
    let (from, to) = buf
        .selection_bounds()
        .map(|(a, b)| (a.offset(), b.offset()))
        .unwrap_or_else(|| {
            let caret = buf.iter_at_mark(&buf.get_insert()).offset();
            (caret, caret)
        });
    let mut m = live.borrow_mut();
    let (s, e) = (m.sequence_offset(buf, from.max(0) as usize), m.sequence_offset(buf, to.max(0) as usize));
    let mut ops = Vec::new();
    if e > s {
        ops.push(Op::Delete { at: s, len: e - s });
    }
    // Where the inserted content ends: the caret goes there, after all of
    // it, not after its first paragraph as the projection would put it.
    let end = s + content.iter().map(|p| letters_core::edit::seq_len(&p.runs)).sum::<usize>() + content.len().saturating_sub(1);
    ops.push(Op::Insert { at: s, content });
    if !m.apply_user_ops(buf, &ops, false) {
        return false;
    }
    if let Some(off) = m.buffer_offset(buf, end) {
        drop(m);
        buf.place_cursor(&buf.iter_at_offset(off as i32));
    }
    true
}

/// Insert a suite fragment at the cursor, replacing the selection: styled
/// runs keep their style, paragraphs their paragraph style; a grid lands
/// as tab-separated lines (a real table paste needs the buffer table
/// support tracked in PARITY's bridge gaps).
pub(crate) fn insert_fragment(buf: &gtk::TextBuffer, frag: &Fragment) {
    let paras = match frag {
        Fragment::Text(paras) => paras.clone(),
        Fragment::Grid(_) => frag
            .to_plain()
            .split('\n')
            .map(|line| Paragraph { style: Default::default(), runs: vec![Run::plain(line)] })
            .collect(),
    };
    if replace_selection(buf, paras) {
        return;
    }
    match frag {
        Fragment::Text(paras) => {
            for (i, p) in paras.iter().enumerate() {
                if i > 0 {
                    buf.insert_at_cursor("\n");
                }
                for run in &p.runs {
                    let tags = crate::bridge::run_tags(buf, &run.style);
                    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
                    let mut iter = buf.iter_at_mark(&buf.get_insert());
                    if tags.is_empty() {
                        buf.insert(&mut iter, &run.text);
                    } else {
                        buf.insert_with_tags_by_name(&mut iter, &run.text, &tags);
                    }
                }
            }
        }
        Fragment::Grid(_) => {
            buf.insert_at_cursor(&frag.to_plain());
        }
    }
}

/// Insert the image at `path` at the cursor, replacing the selection. The
/// file name is its alt text. It used to insert the Markdown text
/// "![name](path)", and with nothing selected at the document's start.
pub(crate) fn insert_image(buf: &gtk::TextBuffer, path: &std::path::Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
    let mut run = Run::plain(name);
    run.style.image = Some(path.to_string_lossy().into_owned());
    replace_selection(buf, vec![Paragraph { style: Default::default(), runs: vec![run] }])
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

    fn reads(buf: &gtk::TextBuffer) -> (usize, usize) {
        let live = crate::live::of(buf).unwrap();
        let m = live.borrow();
        (m.local_reads, m.full_reads)
    }

    fn doc(buf: &gtk::TextBuffer) -> letters_core::Document {
        crate::live::of(buf).unwrap().borrow_mut().document(buf).clone()
    }

    /// A pasted fragment replaces the selection, keeps its formatting and
    /// paragraph styles, and undoes in one step. It used to be typed into
    /// the buffer next to the selection, which stayed.
    #[test]
    fn a_pasted_fragment_replaces_the_selection_as_one_model_edit() {
        gtk_test(|| {
            let buf = live_buffer("keep this out");
            let mut bold = Run::plain("in");
            bold.style.bold = true;
            let mut heading = Paragraph { style: Default::default(), runs: vec![Run::plain("Title")] };
            heading.style.heading = Some(1);
            let frag = Fragment::Text(vec![Paragraph { style: Default::default(), runs: vec![bold] }, heading]);
            buf.select_range(&buf.iter_at_offset(5), &buf.iter_at_offset(9));
            let before = reads(&buf);

            insert_fragment(&buf, &frag);
            let d = doc(&buf);
            let texts: Vec<String> = d.paragraphs.iter().map(|p| p.text()).collect();
            assert_eq!(texts, vec!["keep in".to_string(), "Title out".to_string()]);
            assert!(d.paragraphs[0].runs.iter().any(|r| r.text == "in" && r.style.bold), "{:?}", d.paragraphs[0].runs);
            assert_eq!(d.paragraphs[1].style.heading, Some(1));
            assert_eq!(reads(&buf), before, "the model read the buffer back instead of taking an op");
            assert_eq!(buf.iter_at_mark(&buf.get_insert()).offset(), 13, "the caret is after the pasted text");

            crate::live::undo(&buf, false);
            assert_eq!(doc(&buf).paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>(), vec!["keep this out".to_string()]);
        });
    }

    /// Insert Image puts an image at the caret. It used to insert the text
    /// "![name](path)", at the document's start when nothing was selected.
    #[test]
    fn insert_image_puts_an_image_at_the_caret() {
        gtk_test(|| {
            let buf = live_buffer("ab");
            buf.place_cursor(&buf.iter_at_offset(1));
            assert!(insert_image(&buf, std::path::Path::new("/pics/cat.png")));
            let d = doc(&buf);
            let runs: Vec<(String, Option<String>)> =
                d.paragraphs[0].runs.iter().map(|r| (r.text.clone(), r.style.image.clone())).collect();
            assert_eq!(
                runs,
                vec![("a".into(), None), ("cat.png".into(), Some("/pics/cat.png".into())), ("b".into(), None)]
            );
            crate::live::undo(&buf, false);
            assert_eq!(doc(&buf).paragraphs[0].text(), "ab");
        });
    }
}
