// SPDX-License-Identifier: GPL-3.0-or-later
//
// PageContainer — a tab's document area, in one of two views (ADR 0010):
//
// - Print Layout (the default): the laid-out pages (page_view.rs), where
//   lines, page breaks, headers and footers are where they will print.
// - Draft: the text in one continuous, pageless sheet at the page's text
//   width (Google Docs' "pageless", DESIGN-UI). It used to draw grey page
//   rectangles with one GtkTextView laid across all of them and their gaps,
//   which pretended to show pages that the text never flowed into — the
//   pages are Print Layout's job now.

use gtk4::{self as gtk, gio, glib, prelude::*};
use gtk4::subclass::prelude::*;
use libadwaita as adw;
use std::cell::Cell;

pub const A4_WIDTH_PT: f64 = 595.0;
pub const A4_HEIGHT_PT: f64 = 842.0;
const DEFAULT_MARGIN_TB: f64 = 72.0;
const DEFAULT_MARGIN_LR: f64 = 72.0;

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
        pub header_text: std::cell::RefCell<String>,
        pub footer_text: std::cell::RefCell<String>,
        pub zoom_level: Cell<f64>,
        pub column_count: Cell<u32>,
        /// Pointer is over the widget — margin guides show only then
        /// (DESIGN-UI: margins visible on hover only).
        pub pointer_over: Cell<bool>,
        /// Print Layout (ADR 0010): show the laid-out pages instead of the
        /// editable Draft view.
        pub print_layout: Cell<bool>,
        /// The Print Layout view and the scrolled window holding it; a
        /// second child, after the Draft editor's scrolled window.
        pub print_scroll: std::cell::RefCell<Option<gtk::ScrolledWindow>>,
        pub page_view: std::cell::RefCell<Option<crate::page_view::PageView>>,
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
            self.zoom_level.set(100.0);
            self.column_count.set(1);
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

            if self.print_layout.get() {
                // The page view draws its own pages on this backdrop.
                let is_dark = adw::StyleManager::default().is_dark();
                let bg = if is_dark { 0.13 } else { 0.753 };
                snapshot.append_color(
                    &gtk4::gdk::RGBA::new(bg, bg, bg, 1.0),
                    &gtk4::graphene::Rect::new(0.0, 0.0, w as f32, h as f32),
                );
                self.parent_snapshot(snapshot);
                return;
            }

            // Draft: one pageless sheet.
            let is_dark = adw::StyleManager::default().is_dark();
            let bg = if is_dark { 0.13 } else { 0.753 };
            snapshot.append_color(
                &gtk4::gdk::RGBA::new(bg, bg, bg, 1.0),
                &gtk4::graphene::Rect::new(0.0, 0.0, w as f32, h as f32),
            );
            let (px, sw, scale) = self.obj().sheet_geometry();
            // A cairo node at (0, 0) moved by a transform node: GTK's
            // Broadway renderer draws a cairo node's content from (0, 0)
            // whatever its bounds.
            let node = gtk4::gsk::CairoNode::new(&gtk4::graphene::Rect::new(0.0, 0.0, (sw + 4.0) as f32, h as f32));
            let cr = node.draw_context();
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.10);
            cr.rectangle(2.0, 0.0, sw, h);
            let _ = cr.fill();
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.rectangle(0.0, 0.0, sw, h);
            let _ = cr.fill();
            // Margin guides show on hover only (DESIGN-UI).
            if self.pointer_over.get() {
                let (ml, mr) = (self.margin_left.get() * scale, self.margin_right.get() * scale);
                cr.set_source_rgba(0.85, 0.85, 0.85, 0.5);
                cr.set_line_width(0.5);
                cr.set_dash(&[4.0, 4.0], 0.0);
                for x in [ml, sw - mr] {
                    cr.move_to(x, 0.0);
                    cr.line_to(x, h);
                    let _ = cr.stroke();
                }
            }
            drop(cr);
            let to = gtk4::gsk::Transform::new().translate(&gtk4::graphene::Point::new(px as f32, 0.0));
            snapshot.append_node(gtk4::gsk::TransformNode::new(&node, Some(&to)));
            self.parent_snapshot(snapshot);
        }

        fn measure(&self, _orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let pw = self.page_width.get() as i32;
            let ph = self.page_height.get() as i32;
            (0, pw.max(ph), -1, -1)
        }

        // Children must be allocated here, not in snapshot(): GTK4 derives
        // mapping, focusability, and the AT-SPI tree from real allocations.
        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let child = match self.obj().first_child() {
                Some(c) => c,
                None => return,
            };
            let print = self.print_layout.get();
            child.set_child_visible(!print);
            if let Some(ps) = self.print_scroll.borrow().as_ref() {
                ps.set_child_visible(print);
                if print {
                    ps.size_allocate(&gtk4::Allocation::new(0, 0, width.max(1), height.max(1)), -1);
                    return;
                }
            }
            if width <= 0 || height <= 0 { return; }
            // The text column of the pageless sheet, the full height.
            let (px, sw, scale) = self.obj().sheet_geometry();
            let (ml, mr) = (self.margin_left.get() * scale, self.margin_right.get() * scale);
            let cx = (px + ml) as i32;
            let cw = ((sw - ml - mr) as i32).max(1);
            let (cy, ch) = (0, height.max(1));
            child.size_allocate(&gtk4::Allocation::new(cx, cy, cw, ch), -1);
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
        let this: Self = glib::Object::builder().build();
        let motion = gtk::EventControllerMotion::new();
        {
            let w = this.clone();
            motion.connect_enter(move |_, _, _| {
                w.imp().pointer_over.set(true);
                w.queue_draw();
            });
        }
        {
            let w = this.clone();
            motion.connect_leave(move |_| {
                w.imp().pointer_over.set(false);
                w.queue_draw();
            });
        }
        this.add_controller(motion);
        this
    }

    /// The Draft sheet: (left edge x, width, pixels per point). The page's
    /// width at the view's zoom (100% is 96/72 px per point, as in Print
    /// Layout), narrowed to fit the window.
    fn sheet_geometry(&self) -> (f64, f64, f64) {
        let imp = self.imp();
        let (w, pw) = (self.width() as f64, imp.page_width.get());
        if w <= 0.0 || pw <= 0.0 {
            return (0.0, 0.0, 1.0);
        }
        let pad = 24.0;
        let physical = crate::page_view::PX_PER_PT * imp.zoom_level.get() / 100.0;
        let scale = physical.min(((w - 2.0 * pad) / pw).max(0.1));
        let sw = pw * scale;
        (((w - sw) / 2.0).floor(), sw, scale)
    }

    /// On-screen page geometry: (left edge x, pixel width) in this
    /// widget's coordinates, of the Draft sheet or Print Layout's first
    /// page. The ruler aligns its origin to it.
    pub fn page_screen_geometry(&self) -> (f64, f64) {
        if self.width() <= 0 {
            return (0.0, 0.0);
        }
        if let Some(view) = self.page_view().filter(|v| self.is_print_layout() && v.page_count() > 0) {
            let (x, _, w, _) = view.page_rect(0);
            let origin = view.compute_point(self, &gtk4::graphene::Point::new(x as f32, 0.0));
            return (origin.map_or(x, |p| p.x() as f64), w);
        }
        let (x, w, _) = self.sheet_geometry();
        (x, w)
    }

    /// Add the Print Layout view as this container's second child. Called
    /// once, after the Draft editor has been parented.
    pub fn attach_page_view(&self) -> crate::page_view::PageView {
        let view = crate::page_view::PageView::new();
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_child(Some(&view));
        scroll.set_parent(self);
        scroll.set_child_visible(false);
        self.imp().print_scroll.replace(Some(scroll));
        self.imp().page_view.replace(Some(view.clone()));
        view
    }

    /// The Print Layout view, once attached.
    pub fn page_view(&self) -> Option<crate::page_view::PageView> {
        self.imp().page_view.borrow().clone()
    }

    /// Switch between Print Layout and the Draft editor.
    pub fn set_print_layout(&self, on: bool) {
        self.imp().print_layout.set(on);
        self.queue_resize();
        self.queue_draw();
    }

    pub fn is_print_layout(&self) -> bool {
        self.imp().print_layout.get()
    }


    pub fn set_page_size(&self, width_pt: f64, height_pt: f64) {
        let imp = self.imp();
        imp.page_width.set(width_pt);
        imp.page_height.set(height_pt);
        self.queue_resize();
    }

    /// Current page size in points. Mirrors `set_page_size`; the setters here
    /// had no getters, which left the geometry unobservable from outside the
    /// widget and so untestable.
    pub fn page_size(&self) -> (f64, f64) {
        let imp = self.imp();
        (imp.page_width.get(), imp.page_height.get())
    }

    /// Current margins in points, in `set_margins` order.
    pub fn margins(&self) -> (f64, f64, f64, f64) {
        let imp = self.imp();
        (
            imp.margin_top.get(),
            imp.margin_bottom.get(),
            imp.margin_left.get(),
            imp.margin_right.get(),
        )
    }

    pub fn set_margins(&self, top: f64, bottom: f64, left: f64, right: f64) {
        let imp = self.imp();
        imp.margin_top.set(top);
        imp.margin_bottom.set(bottom);
        imp.margin_left.set(left);
        imp.margin_right.set(right);
        self.queue_resize();
    }

    /// Set the header text template. Use {page} for page number.
    pub fn set_header_text(&self, text: &str) {
        self.imp().header_text.replace(text.to_string());
        self.queue_resize();
    }

    /// Get the current header text template.
    pub fn header_text(&self) -> String {
        self.imp().header_text.borrow().clone()
    }

    /// Set the footer text template. Use {page} for page number.
    pub fn set_footer_text(&self, text: &str) {
        self.imp().footer_text.replace(text.to_string());
        self.queue_resize();
    }

    /// Get the current footer text template.
    pub fn footer_text(&self) -> String {
        self.imp().footer_text.borrow().clone()
    }

    /// Set zoom level (50-200).
    pub fn set_zoom(&self, level: f64) {
        self.imp().zoom_level.set(level.clamp(50.0, 200.0));
        if let Some(view) = self.page_view() {
            view.set_zoom(self.zoom_level());
        }
        self.queue_resize();
    }

    /// Get current zoom level.
    pub fn zoom_level(&self) -> f64 {
        self.imp().zoom_level.get()
    }

    /// Set column count for multi-column rendering.
    pub fn set_column_count(&self, count: u32) {
        self.imp().column_count.set(count.max(1));
        self.queue_resize();
    }

    /// Current column count. Mirrors `set_column_count`.
    pub fn column_count(&self) -> u32 {
        self.imp().column_count.get()
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
        self.set_column_count(settings.int("column-count").max(1) as u32);
        self.set_zoom(settings.double("zoom-level").clamp(50.0, 200.0));
    }

    pub fn reload_settings(&self, settings: &gio::Settings) {
        self.load_from_settings(settings);
    }
}

impl Default for PageContainer {
    fn default() -> Self { Self::new() }
}


#[cfg(test)]
mod tests {
    use super::*;

    use suite_common::gtk_test::run as gtk_test;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "expected {expected}, got {actual}"
        );
    }

    fn container() -> PageContainer {
        PageContainer::new()
    }

    #[test]
    fn header_and_footer_round_trip() {
        gtk_test(|| {
            let c = container();
            assert_eq!(c.header_text(), "");
            assert_eq!(c.footer_text(), "");
            c.set_header_text("Page {page}");
            c.set_footer_text("TunaOS — {page}/{total}");
            assert_eq!(c.header_text(), "Page {page}");
            assert_eq!(c.footer_text(), "TunaOS — {page}/{total}");
        });
    }

    #[test]
    fn zoom_is_clamped_to_50_200() {
        gtk_test(|| {
            let c = container();
            assert_close(c.zoom_level(), 100.0);
            c.set_zoom(25.0);
            assert_close(c.zoom_level(), 50.0);
            c.set_zoom(500.0);
            assert_close(c.zoom_level(), 200.0);
            c.set_zoom(133.0);
            assert_close(c.zoom_level(), 133.0);
        });
    }

    #[test]
    fn screen_geometry_is_zero_before_allocation() {
        gtk_test(|| {
            let c = container();
            // Widget not allocated → no meaningful page geometry yet.
            let (gx, gw) = c.page_screen_geometry();
            assert_close(gx, 0.0);
            assert_close(gw, 0.0);
            c.set_page_size(595.0, 842.0);
            let (gx, gw) = c.page_screen_geometry();
            assert_close(gx, 0.0);
            assert_close(gw, 0.0);
        });
    }
}
