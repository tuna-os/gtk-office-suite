// SPDX-License-Identifier: GPL-3.0-or-later
//
// PageContainer — a tab's document area (ADR 0010): the laid-out pages
// (page_view.rs), where lines, page breaks, headers and footers are where
// they will print, on a grey backdrop. It also holds the tab's buffer.
//
// It used to have a second view, Draft: one GtkTextView over a pageless
// sheet. #1202 retired that view, then the hidden GtkTextView that stayed
// behind as the buffer's host; Print Layout is the one editing surface.

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
        /// The page view and the scrolled window holding it, the only child.
        pub print_scroll: std::cell::RefCell<Option<gtk::ScrolledWindow>>,
        pub page_view: std::cell::RefCell<Option<crate::page_view::PageView>>,
        /// The tab's buffer (doc_tab.rs), which the page view edits.
        pub buffer: std::cell::RefCell<Option<gtk::TextBuffer>>,
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
            // The page view draws its own pages on this backdrop.
            let is_dark = adw::StyleManager::default().is_dark();
            let bg = if is_dark { 0.13 } else { 0.753 };
            snapshot.append_color(
                &gtk4::gdk::RGBA::new(bg, bg, bg, 1.0),
                &gtk4::graphene::Rect::new(0.0, 0.0, w as f32, h as f32),
            );
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
            if let Some(ps) = self.print_scroll.borrow().as_ref() {
                ps.size_allocate(&gtk4::Allocation::new(0, 0, width.max(1), height.max(1)), -1);
            }
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

    /// On-screen page geometry: (left edge x, pixel width) in this
    /// widget's coordinates, of the first page; zero before there is one.
    /// The ruler aligns its origin to it.
    pub fn page_screen_geometry(&self) -> (f64, f64) {
        if self.width() <= 0 {
            return (0.0, 0.0);
        }
        match self.page_view().filter(|v| v.page_count() > 0) {
            Some(view) => {
                let (x, _, w, _) = view.page_rect(0);
                let origin = view.compute_point(self, &gtk4::graphene::Point::new(x as f32, 0.0));
                (origin.map_or(x, |p| p.x() as f64), w)
            }
            None => (0.0, 0.0),
        }
    }

    /// Add the page view as this container's child. Called once.
    pub fn attach_page_view(&self) -> crate::page_view::PageView {
        let view = crate::page_view::PageView::new();
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_child(Some(&view));
        scroll.set_parent(self);
        self.imp().print_scroll.replace(Some(scroll));
        self.imp().page_view.replace(Some(view.clone()));
        view
    }

    /// The page view, once attached.
    pub fn page_view(&self) -> Option<crate::page_view::PageView> {
        self.imp().page_view.borrow().clone()
    }

    /// Hold the tab's buffer, for the code that has the tab's widget and
    /// needs its document.
    pub fn set_buffer(&self, buf: &gtk::TextBuffer) {
        self.imp().buffer.replace(Some(buf.clone()));
    }

    /// The tab's buffer.
    pub fn buffer(&self) -> Option<gtk::TextBuffer> {
        self.imp().buffer.borrow().clone()
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

/// The page container in `widget`: itself, or the first among its
/// descendants. A tab's child is its page container (doc_tab.rs).
pub fn find(widget: &impl IsA<gtk::Widget>) -> Option<PageContainer> {
    let widget = widget.as_ref();
    if let Ok(pc) = widget.clone().downcast::<PageContainer>() {
        return Some(pc);
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        if let Some(pc) = find(&c) {
            return Some(pc);
        }
        child = c.next_sibling();
    }
    None
}

/// The buffer of the tab whose widget is `widget`.
pub fn buffer_of(widget: &impl IsA<gtk::Widget>) -> Option<gtk::TextBuffer> {
    find(widget).and_then(|pc| pc.buffer())
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
