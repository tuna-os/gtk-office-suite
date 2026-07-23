// SPDX-License-Identifier: GPL-3.0-or-later
//
// print_preview.rs — Print preview dialog reusing PageContainer's Cairo rendering.

use gtk4::{self as gtk, gio, prelude::*};
use libadwaita as adw;
use adw::prelude::*;
use std::cell::Cell;

/// Show a print preview window for a text buffer.
pub fn show_print_preview(
    parent: &adw::ApplicationWindow,
    buf: &gtk::TextBuffer,
    settings: &gio::Settings,
    header_text: &str,
    footer_text: &str,
) {
    let config = crate::layout::LayoutConfig::from_settings(settings);
    let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
    let pango_ctx = gtk4::pango::Context::new();
    let pages = crate::layout::paginate(buf, &config, &pango_ctx);
    let total_pages = pages.len().max(1);

    let window = adw::Window::builder()
        .title("Print Preview")
        .default_width(700)
        .default_height(900)
        .modal(true)
        .transient_for(parent)
        .build();

    let current_page = std::rc::Rc::new(Cell::new(0usize));
    let cp_draw = current_page.clone();
    let total = total_pages;
    let page_label = gtk::Label::new(Some(&format!("Page 1 of {}", total)));

    let drawing = gtk::DrawingArea::new();
    let pw = config.page_width_pt;
    let ph = config.page_height_pt;
    let ml = config.margin_left;
    let mr = config.margin_right;
    let mt = config.margin_top;
    let mb = config.margin_bottom;
    let text_clone = text.clone();
    let hdr = header_text.to_string();
    let ftr = footer_text.to_string();

    drawing.set_draw_func(move |_area, cr, w, h| {
        let scale = ((w as f64 - 40.0) / pw).min((h as f64 - 80.0) / ph).min(1.5);
        let sw = pw * scale;
        let sh = ph * scale;
        let px = (w as f64 - sw) / 2.0;
        let py = (h as f64 - sh) / 2.0;

        // Desktop gray (theme-aware — tuna-os/gtk-office-suite#77)
        let is_dark = adw::StyleManager::default().is_dark();
        let desktop_bg = if is_dark { (0.13, 0.13, 0.13) } else { (0.75, 0.75, 0.75) };
        cr.set_source_rgb(desktop_bg.0, desktop_bg.1, desktop_bg.2);
        cr.rectangle(0.0, 0.0, w as f64, h as f64);
        cr.fill().unwrap();

        // Use shared page rendering from PageContainer
        let page_idx = cp_draw.get();
        crate::page_container::draw_page_to_cairo(
            cr, page_idx, px, py, sw, sh, scale,
            ml, mr, mt, mb, &hdr, &ftr,
        );

        // Render text for the current page
        if page_idx < total_pages {
            let page = &pages[page_idx];
            let page_text = if page.end_offset as usize <= text_clone.len() {
                &text_clone[page.start_offset as usize..page.end_offset as usize]
            } else { &text_clone };
            let layout = pangocairo::functions::create_layout(cr);
            layout.set_text(page_text);
            let content_w = (pw - ml - mr).max(10.0);
            layout.set_width((content_w * scale * pango::SCALE as f64) as i32);
            cr.set_source_rgb(0.0, 0.0, 0.0);
            cr.move_to(px + ml * scale, py + mt * scale);
            pangocairo::functions::show_layout(cr, &layout);
        }
    });

    // Navigation toolbar
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    toolbar.set_margin_top(8); toolbar.set_margin_bottom(8);
    toolbar.set_margin_start(8); toolbar.set_margin_end(8);

    let prev_btn = gtk::Button::with_label("⟨ Prev");
    let next_btn = gtk::Button::with_label("Next ⟩");

    {
        let cp = current_page.clone(); let pl = page_label.clone(); let da = drawing.clone();
        prev_btn.connect_clicked(move |_| {
            let p = cp.get();
            if p > 0 { cp.set(p - 1); pl.set_text(&format!("Page {} of {}", p, total)); da.queue_draw(); }
        });
    }
    {
        let cp = current_page.clone(); let pl = page_label.clone(); let da = drawing.clone();
        next_btn.connect_clicked(move |_| {
            let p = cp.get();
            if p + 1 < total_pages { cp.set(p + 1); pl.set_text(&format!("Page {} of {}", p + 2, total)); da.queue_draw(); }
        });
    }

    toolbar.append(&prev_btn);
    toolbar.append(&page_label);
    toolbar.append(&next_btn);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&toolbar);
    content.append(&drawing);

    window.set_content(Some(&content));
    window.present();
}
