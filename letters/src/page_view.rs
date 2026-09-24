// SPDX-License-Identifier: GPL-3.0-or-later
//
// PageView — the read-only Print Layout view (ADR 0010). It draws the
// pages of a `letters_core::layout::pango::Typeset`: the same render tree
// and the same `draw_page` that print and PDF use, so what it shows is
// where the lines and page breaks really are.
//
// Zoom is physical: 100% is 96/72 px per point, a Letter page is 816 px
// wide, as in every other word processor and in the render lab's
// LibreOffice reference.

use gtk4::{self as gtk, glib, graphene, gsk, prelude::*};
use gtk4::subclass::prelude::*;
use letters_core::layout::pango::Typeset;
use std::cell::{Cell, RefCell};

/// Gap between pages and around them, in pixels.
const GAP_PX: f64 = 24.0;

/// Pixels per point at 100% zoom: CSS/GTK's 96 dpi.
pub const PX_PER_PT: f64 = 96.0 / 72.0;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct PageView {
        pub typeset: RefCell<Option<Typeset>>,
        /// Zoom percentage (100 = physical size).
        pub zoom: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PageView {
        const NAME: &'static str = "LettersPageView";
        type Type = super::PageView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("page-view");
            klass.set_accessible_role(gtk::AccessibleRole::Document);
        }
    }

    impl ObjectImpl for PageView {
        fn constructed(&self) {
            self.parent_constructed();
            self.zoom.set(100.0);
        }
    }

    impl WidgetImpl for PageView {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let (w, h) = self.obj().content_size();
            let n = if orientation == gtk::Orientation::Horizontal { w } else { h };
            let n = n.ceil() as i32;
            (n, n, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let typeset = self.typeset.borrow();
            let Some(typeset) = typeset.as_ref() else { return };
            let scale = obj.scale();
            // Only pages that intersect the visible area are drawn, except in
            // a render-lab capture, which paints pages outside the viewport.
            let visible = if suite_common::render_dump::active() { None } else { obj.visible_band() };
            for index in 0..typeset.tree().pages.len() {
                let (x, y, w, h) = obj.page_rect(index);
                if let Some((top, bottom)) = visible {
                    if y + h < top || y > bottom {
                        continue;
                    }
                }
                // A cairo node at (0, 0) moved by a transform node: GTK's
                // Broadway renderer draws a cairo node's content from (0, 0)
                // whatever its bounds (see PageContainer::snapshot).
                let node = gsk::CairoNode::new(&graphene::Rect::new(0.0, 0.0, (w + 4.0) as f32, (h + 4.0) as f32));
                let cr = node.draw_context();
                // Shadow, then paper.
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.12);
                cr.rectangle(2.0, 2.0, w, h);
                let _ = cr.fill();
                cr.set_source_rgb(1.0, 1.0, 1.0);
                cr.rectangle(0.0, 0.0, w, h);
                let _ = cr.fill();
                cr.rectangle(0.0, 0.0, w, h);
                cr.clip();
                cr.scale(scale, scale);
                typeset.draw_page(&cr, index);
                drop(cr);
                let to_page = gsk::Transform::new().translate(&graphene::Point::new(x as f32, y as f32));
                snapshot.append_node(gsk::TransformNode::new(&node, Some(&to_page)));
            }
        }
    }
}

glib::wrapper! {
    pub struct PageView(ObjectSubclass<imp::PageView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for PageView {
    fn default() -> Self {
        Self::new()
    }
}

impl PageView {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    /// Show `typeset`'s pages.
    pub fn set_typeset(&self, typeset: Typeset) {
        self.imp().typeset.replace(Some(typeset));
        self.queue_resize();
        self.queue_draw();
    }

    /// Number of laid-out pages (0 before the first layout).
    pub fn page_count(&self) -> usize {
        self.imp().typeset.borrow().as_ref().map_or(0, |t| t.tree().pages.len())
    }

    /// Zoom percentage; 100 is physical size.
    pub fn set_zoom(&self, percent: f64) {
        self.imp().zoom.set(percent.clamp(25.0, 400.0));
        self.queue_resize();
    }

    pub fn zoom(&self) -> f64 {
        self.imp().zoom.get()
    }

    /// Pixels per point at the current zoom.
    pub fn scale(&self) -> f64 {
        PX_PER_PT * self.imp().zoom.get() / 100.0
    }

    /// Page sizes in pixels.
    fn page_sizes(&self) -> Vec<(f64, f64)> {
        let s = self.scale();
        self.imp()
            .typeset
            .borrow()
            .as_ref()
            .map(|t| t.tree().pages.iter().map(|p| (p.width_pt * s, p.height_pt * s)).collect())
            .unwrap_or_default()
    }

    /// Width and height the pages need, with gaps.
    fn content_size(&self) -> (f64, f64) {
        let sizes = self.page_sizes();
        let w = sizes.iter().map(|s| s.0).fold(0.0, f64::max) + 2.0 * GAP_PX;
        let h = sizes.iter().map(|s| s.1 + GAP_PX).sum::<f64>() + GAP_PX;
        (w, h)
    }

    /// Rectangle (x, y, w, h) of page `index` in this widget's coordinates:
    /// pages stacked top to bottom, each centred horizontally.
    pub fn page_rect(&self, index: usize) -> (f64, f64, f64, f64) {
        let sizes = self.page_sizes();
        let width = f64::from(self.width()).max(self.content_size().0);
        let y = GAP_PX + sizes.iter().take(index).map(|s| s.1 + GAP_PX).sum::<f64>();
        let (w, h) = sizes.get(index).copied().unwrap_or((0.0, 0.0));
        (((width - w) / 2.0).floor(), y.floor(), w, h)
    }

    /// The vertical band of this widget visible through its scrolled
    /// window's viewport, if it has one.
    fn visible_band(&self) -> Option<(f64, f64)> {
        let viewport = self.parent()?.downcast::<gtk::Viewport>().ok()?;
        let adj = viewport.vadjustment()?;
        Some((adj.value(), adj.value() + adj.page_size()))
    }
}

/// Decode an image for the page view through GdkTexture, which reads every
/// format the Draft editor shows (the core's own loader reads PNG only).
pub fn load_image(path: &str) -> Option<cairo::ImageSurface> {
    let texture = gtk::gdk::Texture::from_filename(path).ok()?;
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, texture.width(), texture.height()).ok()?;
    let stride = surface.stride() as usize;
    {
        // GdkTexture::download writes premultiplied native-endian ARGB,
        // which is Cairo's ARGB32.
        let mut data = surface.data().ok()?;
        texture.download(&mut data, stride);
    }
    surface.mark_dirty();
    Some(surface)
}

#[cfg(test)]
mod tests {
    use super::*;
    use letters_core::layout::LayoutOptions;
    use letters_core::model::Document;
    use suite_common::gtk_test::run as gtk_test;

    #[test]
    fn pages_stack_at_physical_size() {
        gtk_test(|| {
            let v = PageView::new();
            assert_eq!(v.page_count(), 0);
            let text = "Line of text.\n".repeat(120);
            let opts = LayoutOptions {
                page: letters_core::model::PageGeometry { width_pt: 612.0, height_pt: 792.0, ..Default::default() },
                ..Default::default()
            };
            v.set_typeset(Typeset::new(Document::from_plain_text(&text), opts));
            assert!(v.page_count() >= 2, "120 lines need more than one page");
            let (_, y0, w0, h0) = v.page_rect(0);
            let (_, y1, _, _) = v.page_rect(1);
            let close = |a: f64, b: f64| (a - b).abs() < 1.0;
            assert!(close(w0, 816.0) && close(h0, 1056.0), "100% is 96 dpi: Letter is 816x1056 px, got {w0}x{h0}");
            assert!(close(y1, y0 + h0 + GAP_PX), "pages stack with one gap: {y0} {h0} {y1}");
            v.set_zoom(50.0);
            assert!(close(v.page_rect(0).2, 408.0));
        });
    }
}
