// SPDX-License-Identifier: GPL-3.0-or-later
//
// PageContainer — Cairo custom widget that draws a white page rectangle
// on a gray desktop background, with configurable page size and margin lines.
// Wraps the GtkTextView editor to give it a word-processor look.

use gtk4::{self as gtk, gio, glib, prelude::*};
use gtk4::subclass::prelude::*;
use std::cell::Cell;

// ── A4 default page size in points (72 DPI) ───────────────────────────
pub const A4_WIDTH_PT: f64 = 595.0;
pub const A4_HEIGHT_PT: f64 = 842.0;

/// Top/bottom margin inside page in points (default 72 = 1 inch).
const DEFAULT_MARGIN_TB: f64 = 72.0;
/// Left/right margin inside page in points (default 72 = 1 inch).
const DEFAULT_MARGIN_LR: f64 = 72.0;

// ── GObject subclass ───────────────────────────────────────────────────

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

            // Scale to fit with padding
            let pad = 24.0;
            let scale = ((w - pad * 2.0) / pw).min((h - pad * 2.0) / ph).min(1.5);
            let sw = pw * scale;
            let sh = ph * scale;
            let px = (w - sw) / 2.0;
            let py = (h - sh) / 2.0;

            // ── Desktop background (fill entire area) ──
            snapshot.append_color(
                &gtk4::gdk::RGBA::new(0.753, 0.753, 0.753, 1.0), // #C0C0C0
                &gtk4::graphene::Rect::new(0.0, 0.0, w as f32, h as f32),
            );

            // Use append_cairo for custom drawing
            let cr = snapshot.append_cairo(&gtk4::graphene::Rect::new(
                px as f32 - 4.0, py as f32 - 4.0,
                (sw + 8.0) as f32, (sh + 8.0) as f32,
            ));

            // ── Drop shadow ──
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.10);
            draw_rounded_rect(&cr, px + 2.0, py + 2.0, sw, sh, 2.0);
            cr.fill().unwrap();

            // ── White page ──
            cr.set_source_rgb(1.0, 1.0, 1.0);
            draw_rounded_rect(&cr, px, py, sw, sh, 2.0);
            cr.fill().unwrap();

            // ── Page border ──
            cr.set_source_rgba(0.85, 0.85, 0.85, 0.8);
            cr.set_line_width(0.5);
            draw_rounded_rect(&cr, px, py, sw, sh, 2.0);
            cr.stroke().unwrap();

            // ── Margin lines ──
            let ml = self.margin_left.get() * scale;
            let mr = self.margin_right.get() * scale;
            let mt = self.margin_top.get() * scale;
            let mb = self.margin_bottom.get() * scale;

            cr.set_source_rgba(0.85, 0.85, 0.85, 0.5);
            cr.set_line_width(0.5);
            cr.set_dash(&[4.0, 4.0], 0.0);

            cr.move_to(px + ml, py);
            cr.line_to(px + ml, py + sh);
            cr.stroke().unwrap();

            cr.move_to(px + sw - mr, py);
            cr.line_to(px + sw - mr, py + sh);
            cr.stroke().unwrap();

            cr.move_to(px, py + mt);
            cr.line_to(px + sw, py + mt);
            cr.stroke().unwrap();

            cr.move_to(px, py + sh - mb);
            cr.line_to(px + sw, py + sh - mb);
            cr.stroke().unwrap();

            drop(cr); // Release Cairo context so child can draw

            // ── Position child within page content area ──
            if let Some(child) = self.obj().first_child() {
                let cx = (px + ml) as i32;
                let cy = (py + mt) as i32;
                let cw = ((sw - ml - mr) as i32).max(1);
                let ch = ((sh - mt - mb) as i32).max(1);
                child.size_allocate(&gtk4::Allocation::new(cx, cy, cw, ch), -1);
            }

            // Chain up to let child widget draw itself
            self.parent_snapshot(snapshot);
        }

        fn measure(&self, _orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let pw = self.page_width.get() as i32;
            let ph = self.page_height.get() as i32;
            // Return page size as natural, growing from 0
            (0, pw.max(ph), -1, -1)
        }

        fn size_allocate(&self, _width: i32, _height: i32, _baseline: i32) {
            // Content allocation handled in snapshot
        }
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

    pub fn load_from_settings(&self, settings: &gio::Settings) {
        let pw = settings.double("page-width-pt");
        let ph = settings.double("page-height-pt");
        if pw > 0.0 && ph > 0.0 {
            self.set_page_size(pw, ph);
        }
        self.set_margins(
            settings.double("page-margin-top"),
            settings.double("page-margin-bottom"),
            settings.double("page-margin-left"),
            settings.double("page-margin-right"),
        );
    }

    /// Reload page layout from GSettings.
    pub fn reload_settings(&self, settings: &gio::Settings) {
        self.load_from_settings(settings);
    }
}

impl Default for PageContainer {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

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
