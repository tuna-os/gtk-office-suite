// SPDX-License-Identifier: GPL-3.0-or-later
//
// style_picker.rs — the paragraph-style picker (DESIGN-UI "Styles first").
//
// A toolbar button names the caret's paragraph style and opens a list in
// which every style is drawn in its own look: each row is shaped by the
// layout engine's own shaper (`PangoShaper`) with the document's own body
// font and heading styles, so a row looks like the paragraph will look on
// the page. Choosing one restyles the selected paragraphs on the live model
// (`LiveModel::restyle`, `SetParaStyle` ops): one undo step, in either view.
//
// The list offers the styles the model and the page view both carry: body
// text and the six heading levels. (The old dropdown's Title, Subtitle, Code
// and Blockquote were buffer tags the page view never drew and a save
// dropped; they return when the layout engine renders them.)

use gtk4::{self as gtk, prelude::*};
use letters_core::layout::{self, pango::PangoShaper, LayoutOptions};
use letters_core::{ParaStyle, Paragraph, Run};
use libadwaita as adw;
use std::rc::Rc;

/// The styles the picker offers, in order.
pub const STYLES: [&str; 7] = ["Normal", "Heading 1", "Heading 2", "Heading 3", "Heading 4", "Heading 5", "Heading 6"];

/// The name the picker shows for a paragraph's style.
pub fn style_name(style: &ParaStyle) -> String {
    match (style.heading, style.named_style.as_deref()) {
        (_, Some(name @ ("Title" | "Subtitle"))) => name.to_string(),
        (Some(level), _) => format!("Heading {}", level.clamp(1, 6)),
        _ => "Normal".to_string(),
    }
}

/// `style` restyled as the picker's `name`. Picker styles replace one
/// another (a heading is no longer a Title); alignment, lists, spacing and
/// the rest of the paragraph's properties are kept.
pub fn restyled(style: &ParaStyle, name: &str) -> ParaStyle {
    let heading = name.strip_prefix("Heading ").and_then(|l| l.parse::<u8>().ok()).filter(|l| (1..=6).contains(l));
    ParaStyle { heading, named_style: None, ..style.clone() }
}

/// Restyle the selected paragraphs (or the caret's) of `buf` as `name`, on
/// the live model. `false` if the tab has no model or the op did not apply.
pub fn apply(buf: &gtk::TextBuffer, name: &str) -> bool {
    let Some(live) = crate::live::of(buf) else { return false };
    let at = |m: &gtk::TextMark| buf.iter_at_mark(m).offset().max(0) as usize;
    let (a, b) = (at(&buf.get_insert()), at(&buf.selection_bound()));
    let done = live.borrow_mut().restyle(buf, a.min(b), a.max(b), |s| restyled(s, name));
    crate::live::sync_actions(buf);
    done
}

/// One style's preview: its name as a paragraph in that style, shaped the
/// way the page view shapes it.
fn preview_layout(name: &str, opts: &LayoutOptions) -> gtk::pango::Layout {
    let para = Paragraph {
        runs: vec![Run { text: name.to_string(), style: Default::default() }],
        style: restyled(&ParaStyle::default(), name),
    };
    PangoShaper::new().layout(&layout::paragraph_request(&para, 400.0, opts))
}

/// A row drawing `name` in its style, at screen size (72 pt per 96 px),
/// shrunk only if a document's own heading look is taller than a row.
fn preview_row(name: &'static str, opts: &Rc<std::cell::RefCell<LayoutOptions>>) -> gtk::ListBoxRow {
    let area = gtk::DrawingArea::new();
    area.set_content_width(220);
    area.set_content_height(34);
    let opts = opts.clone();
    area.set_draw_func(move |area, cr, w, h| {
        let layout = preview_layout(name, &opts.borrow());
        let (_, logical) = layout.extents();
        let (lw, lh) = (f64::from(logical.width()) / 1024.0, f64::from(logical.height()) / 1024.0);
        let k = (96.0 / 72.0_f64).min(f64::from(h) / lh.max(1.0)).min(f64::from(w) / lw.max(1.0));
        let fg = area.color();
        cr.set_source_rgba(fg.red().into(), fg.green().into(), fg.blue().into(), fg.alpha().into());
        cr.translate(0.0, (f64::from(h) - lh * k) / 2.0);
        cr.scale(k, k);
        pangocairo::functions::show_layout(cr, &layout);
    });
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&area));
    row.update_property(&[gtk::accessible::Property::Label(name)]);
    row.set_tooltip_text(Some(name));
    row
}

/// The picker for the window's tabs: a toolbar button following the
/// active tab's caret.
pub fn build(tv: &adw::TabView) -> gtk::MenuButton {
    let label = gtk::Label::new(Some("Normal"));
    label.set_width_chars(9);
    label.set_xalign(0.0);
    let button = gtk::MenuButton::builder()
        .child(&label)
        .tooltip_text(suite_common::i18n("Paragraph style"))
        .build();
    button.update_property(&[gtk::accessible::Property::Label(&suite_common::i18n("Paragraph style"))]);

    // The active document's look: its body font and heading styles.
    let opts = Rc::new(std::cell::RefCell::new(LayoutOptions::default()));
    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    for name in STYLES {
        list.append(&preview_row(name, &opts));
    }
    let popover = gtk::Popover::new();
    popover.set_child(Some(&list));
    button.set_popover(Some(&popover));
    {
        let tv = tv.clone();
        let popover = popover.clone();
        list.connect_row_activated(move |_, row| {
            let name = STYLES[row.index().max(0) as usize];
            popover.popdown();
            if let Some(buf) = crate::dialogs::active_buffer(&tv) {
                apply(&buf, name);
            }
            crate::dialogs::focus_active_view(&tv);
        });
    }
    // Opening: draw the rows in the document's look, mark the current one.
    {
        let (tv, list, opts, label) = (tv.clone(), list.clone(), opts.clone(), label.clone());
        popover.connect_show(move |_| {
            let Some(buf) = crate::dialogs::active_buffer(&tv) else { return };
            if let Some(live) = crate::live::of(&buf) {
                *opts.borrow_mut() = LayoutOptions::default().for_document(live.borrow_mut().document(&buf));
            }
            let current = STYLES.iter().position(|s| *s == label.text());
            list.select_row(current.and_then(|i| list.row_at_index(i as i32)).as_ref());
            let mut child = list.first_child();
            while let Some(c) = child {
                if let Some(area) = c.first_child() {
                    area.queue_draw();
                }
                child = c.next_sibling();
            }
        });
    }
    // The readout follows the caret and every edit (undo included).
    crate::dialogs::watch_active_buffer(tv, move |buf, _| {
        let Some(live) = crate::live::of(buf) else { return };
        let off = buf.iter_at_mark(&buf.get_insert()).offset().max(0) as usize;
        let style = live.borrow_mut().paragraph_style_at(buf, off);
        label.set_text(&style.map(|s| style_name(&s)).unwrap_or_else(|| "Normal".into()));
    });
    button
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite_common::gtk_test::run as gtk_test;

    #[test]
    fn picker_styles_replace_each_other_and_keep_the_rest() {
        let body = ParaStyle { alignment: letters_core::Alignment::Center, named_style: Some("Title".into()), ..Default::default() };
        let h2 = restyled(&body, "Heading 2");
        assert_eq!((h2.heading, h2.named_style.as_deref(), h2.alignment), (Some(2), None, letters_core::Alignment::Center));
        assert_eq!(style_name(&h2), "Heading 2");
        assert_eq!(style_name(&body), "Title");
        let normal = restyled(&h2, "Normal");
        assert_eq!((normal.heading, style_name(&normal).as_str()), (None, "Normal"));
    }

    #[test]
    fn a_preview_is_shaped_like_the_page_draws_that_style() {
        // The document's own heading look reaches the preview.
        let mut opts = LayoutOptions::default();
        let (_, body) = preview_layout("Normal", &opts).extents();
        let (_, h1) = preview_layout("Heading 1", &opts).extents();
        assert!(h1.height() > body.height(), "a heading preview is larger than body text");
        opts.heading_styles = vec![letters_core::RunStyle { font_size_hp: Some(80), ..Default::default() }];
        let (_, big) = preview_layout("Heading 1", &opts).extents();
        assert!(big.height() > h1.height(), "the document's 40pt Heading 1 is previewed at 40pt");
    }

    #[test]
    fn choosing_a_style_is_one_model_op_and_one_undo_step() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            let live = crate::live::LiveModel::attach(&buf);
            crate::bridge::load_document(&letters_core::Document::from_plain_text("one\ntwo\nthree"), &buf);
            // A selection from "one" into "two" restyles both paragraphs.
            buf.select_range(&buf.iter_at_offset(1), &buf.iter_at_offset(5));
            assert!(apply(&buf, "Heading 2"));
            let styles = |buf: &gtk::TextBuffer| -> Vec<Option<u8>> {
                live.borrow_mut().document(buf).paragraphs.iter().map(|p| p.style.heading).collect()
            };
            assert_eq!(styles(&buf), [Some(2), Some(2), None]);
            // The Draft view shows it, and reads back the same document.
            assert_eq!(crate::bridge::capture_with_starts(&buf).0.paragraphs[1].style.heading, Some(2));
            crate::live::undo(&buf, false);
            assert_eq!(styles(&buf), [None, None, None]);
        });
    }
}
