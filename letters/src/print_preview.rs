// SPDX-License-Identifier: GPL-3.0-or-later
//
// print_preview.rs — Print preview dialog showing paginated document
// as it will print, using the same layout engine as the editor.

use gtk4::{self as gtk, gio, glib, prelude::*};
use libadwaita as adw;
use adw::prelude::*;
use std::cell::Cell;

/// Show a print preview window for a text buffer.
pub fn show_print_preview(
    parent: &adw::ApplicationWindow,
    buf: &gtk::TextBuffer,
    settings: &gio::Settings,
) {
    let config = crate::layout::LayoutConfig::from_settings(settings);
    let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();

    // Create a PangoContext for layout measurement (use default)
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

    // Drawing area for page preview
    let drawing = gtk::DrawingArea::new();
    let pw = config.page_width_pt;
    let ph = config.page_height_pt;
    let ml = config.margin_left;
    let mr = config.margin_right;
    let mt = config.margin_top;
    let mb = config.margin_bottom;
    let text_clone = text.clone();

    drawing.set_draw_func(move |_area, cr, w, h| {
        let scale = ((w as f64 - 40.0) / pw).min((h as f64 - 80.0) / ph).min(1.5);
        let sw = pw * scale;
        let sh = ph * scale;
        let px = (w as f64 - sw) / 2.0;
        let py = (h as f64 - sh) / 2.0;

        // Desktop gray
        cr.set_source_rgb(0.75, 0.75, 0.75);
        cr.rectangle(0.0, 0.0, w as f64, h as f64);
        cr.fill().unwrap();

        // Drop shadow (matching PageContainer)
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.10);
        draw_rounded_preview_rect(cr, px + 2.0, py + 2.0, sw, sh, 2.0);
        cr.fill().unwrap();

        // White page
        cr.set_source_rgb(1.0, 1.0, 1.0);
        draw_rounded_preview_rect(cr, px, py, sw, sh, 2.0);
        cr.fill().unwrap();

        // Page border
        cr.set_source_rgba(0.85, 0.85, 0.85, 0.8);
        cr.set_line_width(0.5);
        draw_rounded_preview_rect(cr, px, py, sw, sh, 2.0);
        cr.stroke().unwrap();

        let cml = ml * scale; let cmr = mr * scale;
        let cmt = mt * scale; let cmb = mb * scale;

        // Margin lines (dashed)
        cr.set_source_rgba(0.85, 0.85, 0.85, 0.5);
        cr.set_dash(&[4.0, 4.0], 0.0);
        cr.move_to(px + cml, py); cr.line_to(px + cml, py + sh); cr.stroke().unwrap();
        cr.move_to(px + sw - cmr, py); cr.line_to(px + sw - cmr, py + sh); cr.stroke().unwrap();
        cr.move_to(px, py + cmt); cr.line_to(px + sw, py + cmt); cr.stroke().unwrap();
        cr.move_to(px, py + sh - cmb); cr.line_to(px + sw, py + sh - cmb); cr.stroke().unwrap();
        cr.set_dash(&[], 0.0);

        let page_idx = cp_draw.get();

        // Header (matching PageContainer)
        cr.set_source_rgba(0.5, 0.5, 0.5, 0.7);
        cr.set_font_size(9.0);
        let hdr = format!("Page {}", page_idx + 1);
        let ext = cr.text_extents(&hdr).ok();
        let hw = ext.map(|e| e.width()).unwrap_or(30.0);
        cr.move_to(px + (sw - hw) / 2.0, py + cmt - 8.0);
        let _ = cr.show_text(&hdr);

        // Footer (matching PageContainer — page number)
        let ftr = format!("{}", page_idx + 1);
        let fext = cr.text_extents(&ftr).ok();
        let fw = fext.map(|e| e.width()).unwrap_or(10.0);
        cr.move_to(px + (sw - fw) / 2.0, py + sh - cmb + 12.0);
        let _ = cr.show_text(&ftr);

        // Render text
        if page_idx < total_pages {
            let page = &pages[page_idx];
            let page_text = if page.end_offset as usize <= text_clone.len() {
                &text_clone[page.start_offset as usize..page.end_offset as usize]
            } else { &text_clone };
            let layout = pangocairo::functions::create_layout(cr);
            layout.set_text(page_text);
            layout.set_width(((pw - ml - mr) * scale * pango::SCALE as f64) as i32);
            cr.set_source_rgb(0.0, 0.0, 0.0);
            cr.move_to(px + cml, py + cmt);
            pangocairo::functions::show_layout(cr, &layout);
        }
    });

    // Navigation toolbar
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    toolbar.set_margin_top(8);
    toolbar.set_margin_bottom(8);
    toolbar.set_margin_start(8);
    toolbar.set_margin_end(8);

    let prev_btn = gtk::Button::with_label("⟨ Prev");
    let next_btn = gtk::Button::with_label("Next ⟩");

    {
        let cp = current_page.clone();
        let pl = page_label.clone();
        let da = drawing.clone();
        prev_btn.connect_clicked(move |_| {
            let p = cp.get();
            if p > 0 {
                cp.set(p - 1);
                pl.set_text(&format!("Page {} of {}", p, total));
                da.queue_draw();
            }
        });
    }
    {
        let cp = current_page.clone();
        let pl = page_label.clone();
        let da = drawing.clone();
        next_btn.connect_clicked(move |_| {
            let p = cp.get();
            if p + 1 < total_pages {
                cp.set(p + 1);
                pl.set_text(&format!("Page {} of {}", p + 2, total));
                da.queue_draw();
            }
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

fn draw_rounded_preview_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    let r = r.min(w / 2.0).min(h / 2.0);
    let pi = std::f64::consts::PI;
    cr.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
    cr.arc(x + r, y + h - r, r, pi / 2.0, pi);
    cr.arc(x + r, y + r, r, pi, 3.0 * pi / 2.0);
    cr.close_path();
}
