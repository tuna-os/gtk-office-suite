// SPDX-License-Identifier: GPL-3.0-or-later
//
// PageView — the Print Layout view (ADR 0010). It draws the pages of a
// `letters_core::layout::pango::Typeset`: the same render tree and the same
// `draw_page` that print and PDF use, so what it shows is where the lines
// and page breaks really are. It edits the tab's GtkTextBuffer (still the
// document's live state; see page_edit.rs): the caret and selection are the
// buffer's, mapped onto the laid-out pages.
//
// Zoom is physical: 100% is 96/72 px per point, a Letter page is 816 px
// wide, as in every other word processor and in the render lab's
// LibreOffice reference.

use gtk4::{self as gtk, glib, graphene, gsk, prelude::*};
use gtk4::subclass::prelude::*;
use letters_core::layout::pango::{TextPos, Typeset};
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
        /// Buffer offset where each laid-out paragraph's text starts
        /// (`bridge::capture_with_starts`, taken with the typeset).
        pub starts: RefCell<Vec<usize>>,
        /// The buffer this view edits, once editable.
        pub buffer: RefCell<Option<gtk::TextBuffer>>,
        /// Zoom percentage (100 = physical size).
        pub zoom: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PageView {
        const NAME: &'static str = "LettersPageView";
        type Type = super::PageView;
        type ParentType = gtk::Widget;
        type Interfaces = (gtk::AccessibleText,);

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("page-view");
            // A multi-line text box, as GtkTextView is: screen readers read
            // and track the page view's text through GtkAccessibleText.
            klass.set_accessible_role(gtk::AccessibleRole::TextBox);
        }
    }

    impl ObjectImpl for PageView {
        fn constructed(&self) {
            self.parent_constructed();
            self.zoom.set(100.0);
        }
    }

    /// The text screen readers see is the buffer's, the text the page view
    /// edits; offsets are the buffer's char offsets.
    impl AccessibleTextImpl for PageView {
        fn contents(&self, start: u32, end: u32) -> Option<glib::Bytes> {
            let buf = self.buffer.borrow().clone()?;
            let (s, e) = (buf.iter_at_offset(start as i32), buf.iter_at_offset(end.min(i32::MAX as u32) as i32));
            Some(glib::Bytes::from_owned(buf.text(&s, &e, false).to_string().into_bytes()))
        }

        fn contents_at(&self, offset: u32, granularity: gtk::AccessibleTextGranularity) -> Option<(u32, u32, glib::Bytes)> {
            let buf = self.buffer.borrow().clone()?;
            let at = buf.iter_at_offset(offset as i32);
            let (mut s, mut e) = (at, at);
            match granularity {
                gtk::AccessibleTextGranularity::Character => {
                    e.forward_char();
                }
                gtk::AccessibleTextGranularity::Word => {
                    if !s.starts_word() {
                        s.backward_word_start();
                    }
                    e.forward_word_end();
                }
                gtk::AccessibleTextGranularity::Sentence => {
                    if !s.starts_sentence() {
                        s.backward_sentence_start();
                    }
                    e.forward_sentence_end();
                }
                // A line is a laid-out line on the page, not a paragraph.
                gtk::AccessibleTextGranularity::Line => {
                    let (ls, le) = self.obj().line_bounds(offset as usize).unwrap_or((offset as usize, offset as usize));
                    s = buf.iter_at_offset(ls as i32);
                    e = buf.iter_at_offset(le as i32);
                }
                _ => {
                    s.set_line_offset(0);
                    if !e.ends_line() {
                        e.forward_to_line_end();
                    }
                }
            }
            let text = buf.text(&s, &e, false).to_string();
            Some((s.offset() as u32, e.offset() as u32, glib::Bytes::from_owned(text.into_bytes())))
        }

        fn caret_position(&self) -> u32 {
            self.buffer.borrow().as_ref().map_or(0, |b| b.iter_at_mark(&b.get_insert()).offset().max(0) as u32)
        }

        fn selection(&self) -> Vec<gtk::AccessibleTextRange> {
            let Some(buf) = self.buffer.borrow().clone() else { return Vec::new() };
            match buf.selection_bounds() {
                Some((s, e)) => vec![gtk::AccessibleTextRange::new(s.offset() as usize, (e.offset() - s.offset()) as usize)],
                None => Vec::new(),
            }
        }

        fn attributes(&self, _offset: u32) -> Vec<(gtk::AccessibleTextRange, glib::GString, glib::GString)> {
            Vec::new()
        }

        fn default_attributes(&self) -> Vec<(glib::GString, glib::GString)> {
            Vec::new()
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
                let (caret, selection) = obj.caret_and_selection(typeset);
                // Selection behind the text, caret in front of it. Neither
                // is document content, so a render-lab capture leaves them
                // out, as it hides the Draft editor's caret.
                let chrome = !suite_common::render_dump::active();
                if chrome {
                    cr.set_source_rgba(0.21, 0.52, 0.89, 0.30);
                    for (page, x, top, w, h) in &selection {
                        if *page == index {
                            cr.rectangle(*x, *top, *w, *h);
                        }
                    }
                    let _ = cr.fill();
                }
                typeset.draw_page(&cr, index);
                if let (true, Some(c)) = (chrome && obj.has_focus() && selection.is_empty(), caret) {
                    if c.page == index {
                        cr.set_source_rgb(0.0, 0.0, 0.0);
                        cr.rectangle(c.x_pt, c.top_pt, 1.0 / scale, c.height_pt);
                        let _ = cr.fill();
                    }
                }
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
        @implements gtk::Accessible, gtk::AccessibleText, gtk::Buildable, gtk::ConstraintTarget;
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

    /// Show `typeset`'s pages. `starts` maps its paragraphs to the buffer
    /// (`bridge::capture_with_starts`).
    pub fn set_typeset(&self, typeset: Typeset, starts: Vec<usize>) {
        self.imp().typeset.replace(Some(typeset));
        self.imp().starts.replace(starts);
        self.queue_resize();
        self.queue_draw();
    }

    /// Lay out an edited version of the document, re-using the shaped
    /// paragraphs the edit did not touch.
    pub fn update_document(&self, doc: letters_core::Document, opts: letters_core::layout::LayoutOptions, starts: Vec<usize>) {
        if let Some(t) = self.imp().typeset.borrow_mut().as_mut() {
            t.update(doc, opts);
        }
        self.imp().starts.replace(starts);
        self.queue_resize();
        self.queue_draw();
    }

    /// The buffer this view edits.
    pub fn buffer(&self) -> Option<gtk::TextBuffer> {
        self.imp().buffer.borrow().clone()
    }

    pub(crate) fn set_buffer(&self, buf: &gtk::TextBuffer) {
        self.imp().buffer.replace(Some(buf.clone()));
        self.update_property(&[
            gtk::accessible::Property::Label(&suite_common::i18n("Print Layout")),
            gtk::accessible::Property::MultiLine(true),
        ]);
        // Tell assistive technologies what changed, as GtkTextView does.
        let v = self.downgrade();
        buf.connect_insert_text(move |_, at, text| {
            if let Some(v) = v.upgrade() {
                let start = at.offset().max(0) as u32;
                v.update_contents(gtk::AccessibleTextContentChange::Insert, start, start + text.chars().count() as u32);
            }
        });
        let v = self.downgrade();
        buf.connect_delete_range(move |_, s, e| {
            if let Some(v) = v.upgrade() {
                v.update_contents(gtk::AccessibleTextContentChange::Remove, s.offset().max(0) as u32, e.offset().max(0) as u32);
            }
        });
        let v = self.downgrade();
        buf.connect_mark_set(move |_, _, mark| {
            let Some(v) = v.upgrade() else { return };
            match mark.name().as_deref() {
                Some("insert") => v.update_caret_position(),
                Some("selection_bound") => v.update_selection_bound(),
                _ => {}
            }
        });
    }

    /// The document position of buffer offset `off`.
    fn text_pos(&self, typeset: &Typeset, off: usize) -> TextPos {
        let (para, offset) = crate::bridge::paragraph_offset(typeset.document(), &self.imp().starts.borrow(), off);
        TextPos { para, offset }
    }

    /// The buffer offset of document position `pos`.
    fn buffer_off(&self, typeset: &Typeset, pos: TextPos) -> Option<usize> {
        let para = typeset.document().paragraphs.get(pos.para)?;
        let start = *self.imp().starts.borrow().get(pos.para)?;
        Some(crate::bridge::buffer_offset(para, start, pos.offset))
    }

    /// Caret and selection rectangles for the buffer's current marks.
    fn caret_and_selection(
        &self,
        typeset: &Typeset,
    ) -> (Option<letters_core::layout::pango::Caret>, Vec<letters_core::layout::pango::SelectionRect>) {
        let Some(buf) = self.buffer() else { return (None, Vec::new()) };
        let insert = buf.iter_at_mark(&buf.get_insert()).offset().max(0) as usize;
        let bound = buf.iter_at_mark(&buf.selection_bound()).offset().max(0) as usize;
        let caret = typeset.caret(self.text_pos(typeset, insert));
        let selection = if insert == bound {
            Vec::new()
        } else {
            typeset.selection_rects(self.text_pos(typeset, insert), self.text_pos(typeset, bound))
        };
        (caret, selection)
    }

    /// The page under widget point (`x`, `y`), and the point in that page's
    /// points. A point between pages belongs to the nearer one.
    fn page_point(&self, x: f64, y: f64) -> Option<(usize, f64, f64)> {
        let n = self.page_count();
        let page = (0..n).min_by(|&a, &b| {
            let d = |i: usize| {
                let (_, py, _, ph) = self.page_rect(i);
                if y < py { py - y } else if y > py + ph { y - py - ph } else { 0.0 }
            };
            d(a).partial_cmp(&d(b)).unwrap_or(std::cmp::Ordering::Equal)
        })?;
        let (px, py, _, _) = self.page_rect(page);
        let s = self.scale();
        Some((page, (x - px) / s, (y - py) / s))
    }

    /// The buffer offset nearest to widget point (`x`, `y`).
    pub fn buffer_offset_at(&self, x: f64, y: f64) -> Option<usize> {
        let typeset = self.imp().typeset.borrow();
        let typeset = typeset.as_ref()?;
        let (page, xp, yp) = self.page_point(x, y)?;
        let pos = typeset.hit_test(page, xp, yp)?;
        self.buffer_off(typeset, pos)
    }

    /// The caret box for buffer offset `off`, in widget pixels: (x, y, h).
    pub fn caret_rect(&self, off: usize) -> Option<(f64, f64, f64)> {
        let typeset = self.imp().typeset.borrow();
        let typeset = typeset.as_ref()?;
        let c = typeset.caret(self.text_pos(typeset, off))?;
        let (px, py, _, _) = self.page_rect(c.page);
        let s = self.scale();
        Some((px + c.x_pt * s, py + c.top_pt * s, c.height_pt * s))
    }

    /// The buffer offset one line above (`dir` < 0) or below the caret at
    /// `off`, keeping its x: the next line's nearest position, across page
    /// boundaries. `None` at the first or last line.
    pub fn offset_on_adjacent_line(&self, off: usize, dir: i32) -> Option<usize> {
        let (x, y, h) = self.caret_rect(off)?;
        // Step past the gap between pages too.
        for step in [h * 0.75, h + GAP_PX + 4.0] {
            let ty = if dir < 0 { y - step * 0.5 - 1.0 } else { y + h + step * 0.5 };
            let target = self.buffer_offset_at(x, ty)?;
            if let Some((_, ny, _)) = self.caret_rect(target) {
                if (dir < 0 && ny < y - 0.5) || (dir > 0 && ny > y + 0.5) {
                    return Some(target);
                }
            }
        }
        None
    }

    /// The buffer offsets of the start and end of the laid-out line holding
    /// the caret at `off` (Home and End).
    pub fn line_bounds(&self, off: usize) -> Option<(usize, usize)> {
        let (_, y, h) = self.caret_rect(off)?;
        let mid = y + h / 2.0;
        Some((self.buffer_offset_at(-1e6, mid)?, self.buffer_offset_at(1e6, mid)?))
    }

    /// Write the pages this view shows as a PDF (the render lab compares its
    /// pages with the view's, pixel by pixel).
    pub fn write_pdf(&self, path: &std::path::Path) -> Result<(), String> {
        match self.imp().typeset.borrow().as_ref() {
            Some(t) => t.write_pdf(path),
            None => Err("nothing laid out yet".into()),
        }
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
            v.set_typeset(Typeset::new(Document::from_plain_text(&text), opts), Vec::new());
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
