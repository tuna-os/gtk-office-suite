// SPDX-License-Identifier: GPL-3.0-or-later
//
// PageContainer — Cairo custom widget that draws white page rectangles
// on a gray desktop background, with multi-page support and page breaks.

use gtk4::{self as gtk, gio, glib, prelude::*};
use gtk4::subclass::prelude::*;
use std::cell::Cell;

pub const A4_WIDTH_PT: f64 = 595.0;
pub const A4_HEIGHT_PT: f64 = 842.0;
const DEFAULT_MARGIN_TB: f64 = 72.0;
const DEFAULT_MARGIN_LR: f64 = 72.0;
const PAGE_GAP: f64 = 12.0;
const HEADER_HEIGHT: f64 = 36.0;  // header area in points
const FOOTER_HEIGHT: f64 = 36.0;  // footer area in points

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct PageContainer {
        pub page_width: Cell<f64>,
        pub page_height: Cell<f64>,
        pub margin_top: Cell<f64>,
        pub margin_bottom: Cell<f64>,
        pub margin_left: Cell<f64>,
        pub margin_right: Cell<f64>,
        pub page_count: Cell<usize>,
        pub header_text: std::cell::RefCell<String>,
        pub footer_text: std::cell::RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PageContainer {
        const NAME: &'static str = "PageContainer";
        type Type = super::PageContainer;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("page-container");
        }
    }

    impl ObjectImpl for PageContainer {
        fn constructed(&self) {
            self.parent_constructed();
            self.page_width.set(A4_WIDTH_PT);
            self.page_height.set(A4_HEIGHT_PT);
            self.margin_top.set(DEFAULT_MARGIN_TB);
            self.margin_bottom.set(DEFAULT_MARGIN_TB);
            self.margin_left.set(DEFAULT_MARGIN_LR);
            self.margin_right.set(DEFAULT_MARGIN_LR);
            self.page_count.set(1);
        }
        fn dispose(&self) {
            let obj = self.obj();
            while let Some(child) = obj.first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for PageContainer {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let w = self.obj().width() as f64;
            let h = self.obj().height() as f64;
            if w <= 0.0 || h <= 0.0 { return; }

            let pw = self.page_width.get();
            let ph = self.page_height.get();
            let n_pages = self.page_count.get().max(1);

            let pad = 24.0;
            let scale = ((w - pad * 2.0) / pw).min(1.5); // don't scale up too much
            let sw = pw * scale;
            let sh = ph * scale;
            let total_height = n_pages as f64 * sh + (n_pages as f64 - 1.0) * PAGE_GAP * scale;

            // Center horizontally, start from top with padding
            let px = (w - sw) / 2.0;
            let start_y = ((h - total_height) / 2.0).max(pad);

            // Fill entire widget with desktop gray
            snapshot.append_color(
                &gtk4::gdk::RGBA::new(0.753, 0.753, 0.753, 1.0),
                &gtk4::graphene::Rect::new(0.0, 0.0, w as f32, h as f32),
            );

            // Draw each page
            for page_idx in 0..n_pages {
                let page_y = start_y + page_idx as f64 * (sh + PAGE_GAP * scale);

                // Cairo region for this page + gap
                let cr = snapshot.append_cairo(&gtk4::graphene::Rect::new(
                    px as f32 - 4.0, page_y as f32 - 4.0,
                    (sw + 8.0) as f32, (sh + 8.0) as f32,
                ));

                // Drop shadow
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.10);
                draw_rounded_rect(&cr, px + 2.0, page_y + 2.0, sw, sh, 2.0);
                cr.fill().unwrap();

                // White page
                cr.set_source_rgb(1.0, 1.0, 1.0);
                draw_rounded_rect(&cr, px, page_y, sw, sh, 2.0);
                cr.fill().unwrap();

                // Page border
                cr.set_source_rgba(0.85, 0.85, 0.85, 0.8);
                cr.set_line_width(0.5);
                draw_rounded_rect(&cr, px, page_y, sw, sh, 2.0);
                cr.stroke().unwrap();

                // Margin lines
                let ml = self.margin_left.get() * scale;
                let mr = self.margin_right.get() * scale;
                let mt = self.margin_top.get() * scale;
                let mb = self.margin_bottom.get() * scale;

                cr.set_source_rgba(0.85, 0.85, 0.85, 0.5);
                cr.set_line_width(0.5);
                cr.set_dash(&[4.0, 4.0], 0.0);

                cr.move_to(px + ml, page_y);
                cr.line_to(px + ml, page_y + sh);
                cr.stroke().unwrap();

                cr.move_to(px + sw - mr, page_y);
                cr.line_to(px + sw - mr, page_y + sh);
                cr.stroke().unwrap();

                cr.move_to(px, page_y + mt);
                cr.line_to(px + sw, page_y + mt);
                cr.stroke().unwrap();

                cr.move_to(px, page_y + sh - mb);
                cr.line_to(px + sw, page_y + sh - mb);
                cr.stroke().unwrap();

                // ── Header text ──
                let header = self.header_text.borrow();
                if !header.is_empty() {
                    cr.set_dash(&[], 0.0);
                    cr.set_source_rgba(0.5, 0.5, 0.5, 0.7);
                    cr.set_font_size(9.0);
                    let hdr = header.replace("{page}", &(page_idx + 1).to_string());
                    // Center the header
                    let extents = cr.text_extents(&hdr).ok();
                    let w = extents.map(|e| e.width()).unwrap_or(50.0);
                    cr.move_to(px + (sw - w) / 2.0, page_y + mt - 8.0);
                    let _ = cr.show_text(&hdr);
                }

                // ── Footer text ──
                let footer = self.footer_text.borrow();
                if !footer.is_empty() {
                    cr.set_dash(&[], 0.0);
                    cr.set_source_rgba(0.5, 0.5, 0.5, 0.7);
                    cr.set_font_size(9.0);
                    let ftr = footer.replace("{page}", &(page_idx + 1).to_string());
                    let ftr_ext = cr.text_extents(&ftr).ok();
                    let fw = ftr_ext.map(|e| e.width()).unwrap_or(50.0);
                    cr.move_to(px + (sw - fw) / 2.0, page_y + sh - mb + 12.0);
                    let _ = cr.show_text(&ftr);
                }

                // Page number in the gap (between pages)
                if page_idx > 0 {
                    let gap_center_y = page_y - PAGE_GAP * scale / 2.0;
                    cr.set_dash(&[], 0.0);
                    cr.set_source_rgba(0.5, 0.5, 0.5, 0.6);
                    cr.set_font_size(10.0);
                    let label = format!("Page {}", page_idx + 1);
                    cr.move_to(px + sw / 2.0 - 15.0, gap_center_y + 4.0);
                    let _ = cr.show_text(&label);
                }

                drop(cr);
            }

            // Position child: span all pages' content areas
            if let Some(child) = self.obj().first_child() {
                let ml = self.margin_left.get() * scale;
                let mt = self.margin_top.get() * scale;
                let mr = self.margin_right.get() * scale;
                let mb = self.margin_bottom.get() * scale;

                let content_per_page = sh - mt - mb;
                let total_content = n_pages as f64 * content_per_page + (n_pages as f64 - 1.0) * PAGE_GAP * scale;

                let cx = (px + ml) as i32;
                let cy = (start_y + mt) as i32;
                let cw = ((sw - ml - mr) as i32).max(1);
                let ch = (total_content as i32).max(1);

                child.size_allocate(&gtk4::Allocation::new(cx, cy, cw, ch), -1);
            }

            self.parent_snapshot(snapshot);
        }

        fn measure(&self, _orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let pw = self.page_width.get() as i32;
            let ph = self.page_height.get() as i32;
            (0, pw.max(ph), -1, -1)
        }

        fn size_allocate(&self, _width: i32, _height: i32, _baseline: i32) {}
    }
}

glib::wrapper! {
    pub struct PageContainer(ObjectSubclass<imp::PageContainer>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PageContainer {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn set_page_size(&self, width_pt: f64, height_pt: f64) {
        let imp = self.imp();
        imp.page_width.set(width_pt);
        imp.page_height.set(height_pt);
        self.queue_draw();
    }

    pub fn set_margins(&self, top: f64, bottom: f64, left: f64, right: f64) {
        let imp = self.imp();
        imp.margin_top.set(top);
        imp.margin_bottom.set(bottom);
        imp.margin_left.set(left);
        imp.margin_right.set(right);
        self.queue_draw();
    }

    /// Set the number of pages to render.
    pub fn set_page_count(&self, count: usize) {
        self.imp().page_count.set(count.max(1));
        self.queue_draw();
    }

    /// Set the header text template. Use {page} for page number.
    pub fn set_header_text(&self, text: &str) {
        self.imp().header_text.replace(text.to_string());
        self.queue_draw();
    }

    /// Set the footer text template. Use {page} for page number.
    pub fn set_footer_text(&self, text: &str) {
        self.imp().footer_text.replace(text.to_string());
        self.queue_draw();
    }

    pub fn load_from_settings(&self, settings: &gio::Settings) {
        let pw = settings.double("page-width-pt");
        let ph = settings.double("page-height-pt");
        if pw > 0.0 && ph > 0.0 { self.set_page_size(pw, ph); }
        self.set_margins(
            settings.double("page-margin-top"),
            settings.double("page-margin-bottom"),
            settings.double("page-margin-left"),
            settings.double("page-margin-right"),
        );
    }

    pub fn reload_settings(&self, settings: &gio::Settings) {
        self.load_from_settings(settings);
    }
}

impl Default for PageContainer {
    fn default() -> Self { Self::new() }
}

/// Draw a single page to a Cairo context. Shared by PageContainer snapshot and print preview.
pub fn draw_page_to_cairo(
    cr: &cairo::Context,
    page_idx: usize,
    page_x: f64, page_y: f64, page_w: f64, page_h: f64,
    scale: f64,
    margin_left: f64, margin_right: f64, margin_top: f64, margin_bottom: f64,
    header_text: &str, footer_text: &str,
) {
    let ml = margin_left * scale;
    let mr = margin_right * scale;
    let mt = margin_top * scale;
    let mb = margin_bottom * scale;

    // Drop shadow
    cr.save().unwrap();
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.10);
    draw_rounded_rect(cr, page_x + 2.0, page_y + 2.0, page_w, page_h, 2.0);
    cr.fill().unwrap();
    cr.restore().unwrap();

    // White page
    cr.set_source_rgb(1.0, 1.0, 1.0);
    draw_rounded_rect(cr, page_x, page_y, page_w, page_h, 2.0);
    cr.fill().unwrap();

    // Page border
    cr.set_source_rgba(0.85, 0.85, 0.85, 0.8);
    cr.set_line_width(0.5);
    draw_rounded_rect(cr, page_x, page_y, page_w, page_h, 2.0);
    cr.stroke().unwrap();

    // Margin lines (dashed)
    cr.save().unwrap();
    cr.set_source_rgba(0.85, 0.85, 0.85, 0.5);
    cr.set_line_width(0.5);
    cr.set_dash(&[4.0, 4.0], 0.0);
    cr.move_to(page_x + ml, page_y);
    cr.line_to(page_x + ml, page_y + page_h);
    cr.stroke().unwrap();
    cr.move_to(page_x + page_w - mr, page_y);
    cr.line_to(page_x + page_w - mr, page_y + page_h);
    cr.stroke().unwrap();
    cr.move_to(page_x, page_y + mt);
    cr.line_to(page_x + page_w, page_y + mt);
    cr.stroke().unwrap();
    cr.move_to(page_x, page_y + page_h - mb);
    cr.line_to(page_x + page_w, page_y + page_h - mb);
    cr.stroke().unwrap();
    cr.restore().unwrap();

    // Header text
    if !header_text.is_empty() {
        cr.save().unwrap();
        cr.set_source_rgba(0.5, 0.5, 0.5, 0.7);
        cr.set_font_size(9.0);
        let hdr = header_text.replace("{page}", &(page_idx + 1).to_string());
        let ext = cr.text_extents(&hdr).ok();
        let hw = ext.map(|e| e.width()).unwrap_or(50.0);
        cr.move_to(page_x + (page_w - hw) / 2.0, page_y + mt - 8.0);
        let _ = cr.show_text(&hdr);
        cr.restore().unwrap();
    }

    // Footer text
    if !footer_text.is_empty() {
        cr.save().unwrap();
        cr.set_source_rgba(0.5, 0.5, 0.5, 0.7);
        cr.set_font_size(9.0);
        let ftr = footer_text.replace("{page}", &(page_idx + 1).to_string());
        let fext = cr.text_extents(&ftr).ok();
        let fw = fext.map(|e| e.width()).unwrap_or(50.0);
        cr.move_to(page_x + (page_w - fw) / 2.0, page_y + page_h - mb + 12.0);
        let _ = cr.show_text(&ftr);
        cr.restore().unwrap();
    }
}

fn draw_rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    let r = r.min(w / 2.0).min(h / 2.0);
    let pi = std::f64::consts::PI;
    cr.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
    cr.arc(x + r, y + h - r, r, pi / 2.0, pi);
    cr.arc(x + r, y + r, r, pi, 3.0 * pi / 2.0);
    cr.close_path();
}
