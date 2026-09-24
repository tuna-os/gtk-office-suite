// SPDX-License-Identifier: GPL-3.0-or-later
//
// printing.rs — Print, Print Preview and Export as PDF, all drawn from the
// tab's laid-out pages (ADR 0010). They used to be three layouts of their
// own: printing and preview paginated unstyled plain text, and PDF export
// went through Typst. Now each is `Typeset::draw_page` on another Cairo
// surface, so paper, PDF and Print Layout show the same pages.

use gtk4::{self as gtk, gio, glib, prelude::*};
use libadwaita as adw;
use adw::prelude::*;
use std::rc::Rc;

use crate::page_container::PageContainer;

/// A page setup for a page `w_pt` × `h_pt`, so the printer is asked for the
/// document's paper, not the system default.
fn page_setup(w_pt: f64, h_pt: f64) -> gtk::PageSetup {
    let setup = gtk::PageSetup::new();
    let (w, h, orientation) = if w_pt > h_pt {
        (h_pt, w_pt, gtk::PageOrientation::Landscape)
    } else {
        (w_pt, h_pt, gtk::PageOrientation::Portrait)
    };
    let paper = gtk::PaperSize::new_custom("letters-document", "Document", w, h, gtk::Unit::Points);
    setup.set_paper_size(&paper);
    setup.set_orientation(orientation);
    setup.set_top_margin(0.0, gtk::Unit::Points);
    setup.set_bottom_margin(0.0, gtk::Unit::Points);
    setup.set_left_margin(0.0, gtk::Unit::Points);
    setup.set_right_margin(0.0, gtk::Unit::Points);
    setup
}

/// Print the tab's pages.
pub fn print(win: &adw::ApplicationWindow, container: &PageContainer, buf: &gtk::TextBuffer) {
    let typeset = Rc::new(crate::doc_tab::typeset_for(container, buf));
    let pages = typeset.tree().pages.len();
    let Some(first) = typeset.tree().pages.first() else { return };
    let op = gtk::PrintOperation::new();
    op.set_n_pages(pages as i32);
    // The document's margins are in the page's content; the whole sheet is
    // ours, in points from its corner.
    op.set_use_full_page(true);
    op.set_unit(gtk::Unit::Points);
    op.set_default_page_setup(Some(&page_setup(first.width_pt, first.height_pt)));
    {
        let t = typeset.clone();
        op.connect_request_page_setup(move |_, _, nth, setup| {
            if let Some(p) = t.tree().pages.get(nth as usize) {
                let s = page_setup(p.width_pt, p.height_pt);
                setup.set_paper_size(&s.paper_size());
                setup.set_orientation(s.orientation());
            }
        });
    }
    op.connect_draw_page(move |_, ctx, nth| {
        typeset.draw_page(&ctx.cairo_context(), nth as usize);
    });
    op.set_export_filename("output.pdf");
    let _ = op.run(gtk::PrintOperationAction::PrintDialog, Some(win));
}

/// Ask where to save, then write the tab's pages as a PDF.
pub fn export_pdf(win: &adw::ApplicationWindow, container: &PageContainer, buf: &gtk::TextBuffer) {
    let typeset = crate::doc_tab::typeset_for(container, buf);
    let dlg = gtk::FileDialog::new();
    let f = gtk::FileFilter::new();
    f.add_pattern("*.pdf");
    f.set_name(Some("PDF"));
    let fl = gio::ListStore::new::<gtk::FileFilter>();
    fl.append(&f);
    dlg.set_filters(Some(&fl));
    dlg.set_initial_name(Some("Untitled.pdf"));
    let w = win.clone();
    dlg.save(Some(win), None::<&gio::Cancellable>, move |result: Result<gio::File, glib::Error>| {
        let Some(path) = result.ok().and_then(|f| f.path()) else { return };
        if let Err(e) = typeset.write_pdf(&path) {
            suite_common::show_error_dialog(Some(&w), &suite_common::i18n("Could not export PDF"), &e);
        }
    });
}

/// Show the tab's pages one at a time, as they will print.
pub fn preview(win: &adw::ApplicationWindow, container: &PageContainer, buf: &gtk::TextBuffer) {
    let typeset = Rc::new(crate::doc_tab::typeset_for(container, buf));
    let total = typeset.tree().pages.len().max(1);
    let window = adw::Window::builder()
        .title("Print Preview")
        .default_width(700)
        .default_height(900)
        .modal(true)
        .transient_for(win)
        .build();
    let current = Rc::new(std::cell::Cell::new(0usize));
    let label = gtk::Label::new(Some(&format!("Page 1 of {total}")));
    let drawing = gtk::DrawingArea::new();
    drawing.set_vexpand(true);
    {
        let (t, cur) = (typeset.clone(), current.clone());
        drawing.set_draw_func(move |_, cr, w, h| {
            let is_dark = adw::StyleManager::default().is_dark();
            let bg = if is_dark { 0.13 } else { 0.75 };
            cr.set_source_rgb(bg, bg, bg);
            let _ = cr.paint();
            let Some(page) = t.tree().pages.get(cur.get()) else { return };
            let scale = ((f64::from(w) - 40.0) / page.width_pt).min((f64::from(h) - 40.0) / page.height_pt).max(0.05);
            let (pw, ph) = (page.width_pt * scale, page.height_pt * scale);
            cr.translate(((f64::from(w) - pw) / 2.0).floor(), ((f64::from(h) - ph) / 2.0).floor());
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.rectangle(0.0, 0.0, pw, ph);
            let _ = cr.fill();
            cr.scale(scale, scale);
            t.draw_page(cr, cur.get());
        });
    }
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    toolbar.set_margin_top(8);
    toolbar.set_margin_bottom(8);
    toolbar.set_margin_start(8);
    toolbar.set_margin_end(8);
    let prev = gtk::Button::from_icon_name("go-previous-symbolic");
    prev.set_tooltip_text(Some("Previous page"));
    let next = gtk::Button::from_icon_name("go-next-symbolic");
    next.set_tooltip_text(Some("Next page"));
    for (button, step) in [(&prev, -1isize), (&next, 1)] {
        let (cur, l, d) = (current.clone(), label.clone(), drawing.clone());
        button.connect_clicked(move |_| {
            let page = cur.get().saturating_add_signed(step).min(total - 1);
            cur.set(page);
            l.set_text(&format!("Page {} of {total}", page + 1));
            d.queue_draw();
        });
    }
    toolbar.append(&prev);
    toolbar.append(&label);
    toolbar.append(&next);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&toolbar);
    content.append(&drawing);
    window.set_content(Some(&content));
    window.present();
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite_common::gtk_test::run as gtk_test;

    #[test]
    fn a_landscape_page_asks_for_landscape_paper() {
        gtk_test(|| {
            let s = page_setup(792.0, 612.0);
            assert_eq!(s.orientation(), gtk::PageOrientation::Landscape);
            assert_eq!(s.paper_width(gtk::Unit::Points).round(), 792.0);
            assert_eq!(s.paper_height(gtk::Unit::Points).round(), 612.0);
            let p = page_setup(612.0, 792.0);
            assert_eq!(p.orientation(), gtk::PageOrientation::Portrait);
            assert_eq!(p.top_margin(gtk::Unit::Points), 0.0, "the document's margins are in the page");
        });
    }
}
