// sidebar.rs — Slide sidebar list and management controls.
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk4::{self as gtk, gdk, glib, prelude::*};
use gtk4::cairo;
use decks_core::engine::{MasterSlide, Slide};

const THUMB_W: i32 = 160;
const THUMB_H: i32 = 90;

/// Offscreen render of one slide as a texture (the strip's preview).
pub(crate) fn slide_thumbnail(
    slides: &[Slide],
    masters: &[MasterSlide],
    index: usize,
) -> Option<gtk::Picture> {
    render_thumbnail(slides, masters, index, THUMB_W, THUMB_H, None)
}

/// Slide `index` drawn by the canvas's own renderer at `w`×`h`, as a
/// picture: the strip's thumbnails (the editor's chrome, `chrome` None)
/// and the template chooser's previews (`Chrome::Preview`: the slide alone).
pub fn render_thumbnail(
    slides: &[Slide],
    masters: &[MasterSlide],
    index: usize,
    w: i32,
    h: i32,
    chrome: Option<crate::canvas::Chrome>,
) -> Option<gtk::Picture> {
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).ok()?;
    {
        let cr = cairo::Context::new(&surface).ok()?;
        if let Some(chrome) = chrome {
            crate::canvas::draw_slide_in(&cr, w as f64, h as f64, slides, index, masters, chrome);
        } else {
            // Thumbnails never show selection, so the accent is unused.
            crate::canvas::draw_slide(&cr, w as f64, h as f64, slides, index, None, masters, (0.0, 0.5, 1.0));
        }
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().ok()?.to_vec();
    let bytes = glib::Bytes::from_owned(data);
    let texture = gdk::MemoryTexture::new(
        w,
        h,
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &bytes,
        stride,
    );
    let pic = gtk::Picture::for_paintable(&texture);
    pic.set_size_request(w, h);
    pic.set_can_shrink(true);
    Some(pic)
}

// ── Thumbnails for the rows in view only (#1282) ────────────────────────
//
// The strip drew every slide's thumbnail on each rebuild, so a 300-slide
// deck rendered 300 slides offscreen to show the dozen in view. With a
// source set, a rebuild lays the rows out with sized placeholders, and
// the thumbnails of the rows in (or near) the viewport are drawn as they
// scroll into it, from the deck as it is then.

/// Renders slide `index` of the live deck as a thumbnail.
pub type ThumbnailSource = std::rc::Rc<dyn Fn(usize) -> Option<gtk::Picture>>;

const SOURCE_KEY: &str = "decks-thumbnail-source";
const PENDING: &str = "thumbnail-pending";

/// Draw `list`'s thumbnails lazily from `source`, as rows come into view
/// of `scrolled`.
pub fn set_thumbnail_source(list: &gtk::ListBox, scrolled: &gtk::ScrolledWindow, source: ThumbnailSource) {
    // SAFETY: stored and read back as the same type, under this key only.
    unsafe { list.set_data(SOURCE_KEY, source) };
    let adj = scrolled.vadjustment();
    for signal in ["value-changed", "changed"] {
        let list = list.downgrade();
        adj.connect_local(signal, false, move |_| {
            if let Some(list) = list.upgrade() {
                render_visible_thumbnails(&list);
            }
            None
        });
    }
}

fn source(list: &gtk::ListBox) -> Option<ThumbnailSource> {
    // SAFETY: only ever set by set_thumbnail_source, with this type.
    unsafe { list.data::<ThumbnailSource>(SOURCE_KEY).map(|p| p.as_ref().clone()) }
}

/// The rows whose band `(top, height)` meets `top..bottom`, widened by a
/// screen either way so a short scroll finds them drawn; `bands` is in
/// list order, so this stops at the first row past the end.
pub fn rows_in_view(bands: impl IntoIterator<Item = (f64, f64)>, top: f64, bottom: f64) -> Vec<usize> {
    let margin = bottom - top;
    let (lo, hi) = (top - margin, bottom + margin);
    let mut out = Vec::new();
    for (i, (y, h)) in bands.into_iter().enumerate() {
        if y > hi {
            break;
        }
        if y + h >= lo {
            out.push(i);
        }
    }
    out
}

/// Draw the pending thumbnails of the rows in view.
pub fn render_visible_thumbnails(list: &gtk::ListBox) {
    let Some(source) = source(list) else { return };
    let Some(adj) = list.parent().and_downcast::<gtk::Viewport>().and_then(|v| v.vadjustment()) else { return };
    let (top, bottom) = (adj.value(), adj.value() + adj.page_size());
    if adj.page_size() <= 0.0 {
        return;
    }
    let mut rows = Vec::new();
    let mut i = 0;
    while let Some(row) = list.row_at_index(i) {
        rows.push(row);
        i += 1;
    }
    let bands = rows.iter().map(|r| r.compute_bounds(list).map_or((f64::MAX, 0.0), |b| (b.y() as f64, b.height() as f64)));
    for index in rows_in_view(bands, top, bottom) {
        let Some(pic) = rows[index].child().and_then(|c| c.first_child()).and_downcast::<gtk::Picture>() else { continue };
        if pic.has_css_class(PENDING) {
            if let Some(new_pic) = source(index) {
                pic.set_paintable(new_pic.paintable().as_ref());
            }
            pic.remove_css_class(PENDING);
        }
    }
}

/// Refresh one row's thumbnail in place (no rebuild, no selection churn).
pub fn update_thumbnail(
    list: &gtk::ListBox,
    slides: &[Slide],
    masters: &[MasterSlide],
    index: usize,
) {
    let Some(row) = list.row_at_index(index as i32) else { return };
    let Some(column) = row.child() else { return };
    let Some(pic) = column.first_child().and_downcast::<gtk::Picture>() else { return };
    // Not drawn yet: it is drawn from the deck as it is when it scrolls in.
    if pic.has_css_class(PENDING) {
        return;
    }
    if let Some(new_pic) = slide_thumbnail(slides, masters, index) {
        pic.set_paintable(new_pic.paintable().as_ref());
    }
}

/// Rebuild the slide list widget from the current slides state.
pub fn rebuild_slide_list(
    list: &gtk::ListBox,
    slides: &[Slide],
    masters: &[MasterSlide],
    selected: usize,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let lazy = source(list).is_some();
    for (i, _slide) in slides.iter().enumerate() {
        let row = gtk::ListBoxRow::new();
        let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
        column.set_margin_start(8);
        column.set_margin_end(8);
        column.set_margin_top(6);
        column.set_margin_bottom(6);
        let pic = if lazy {
            let pic = gtk::Picture::new();
            pic.set_size_request(THUMB_W, THUMB_H);
            pic.set_can_shrink(true);
            pic.add_css_class(PENDING);
            Some(pic)
        } else {
            slide_thumbnail(slides, masters, i)
        };
        if let Some(pic) = pic {
            pic.add_css_class("card");
            column.append(&pic);
        }
        let label = gtk::Label::new(Some(&format!("Slide {}", i + 1)));
        label.add_css_class("caption");
        label.add_css_class("dim-label");
        label.set_halign(gtk::Align::Start);
        column.append(&label);
        row.set_child(Some(&column));
        list.append(&row);
    }
    if let Some(row) = list.row_at_index(selected as i32) {
        list.select_row(Some(&row));
    }
    if lazy {
        // Once the new rows have their places.
        let list = list.downgrade();
        glib::idle_add_local_once(move || {
            if let Some(list) = list.upgrade() {
                render_visible_thumbnails(&list);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::rows_in_view;

    /// A 300-slide deck (#1282): rebuilding the strip renders no thumbnail
    /// until rows are in view (it used to render all 300), within a p95
    /// budget; and the canvas draws its one slide within one too.
    #[test]
    fn a_300_slide_strip_renders_thumbnails_only_in_view_and_the_canvas_one_slide() {
        suite_common::gtk_test::run(|| {
            use gtk4::{self as gtk, cairo};
            use std::cell::Cell;
            use std::rc::Rc;
            use std::time::{Duration, Instant};
            let deck = decks_core::engine::Deck::new();
            let mut slide = deck.slides[0].clone();
            slide.objects.push(decks_core::engine::SlideObject::TextBox {
                text: "A line of text on every slide".into(),
                x: 100.0, y: 100.0, w: 600.0, h: 80.0,
                runs: Vec::new(), rotation: 0.0, body: Default::default(),
            });
            let slides = Rc::new(vec![slide; 300]);
            let masters = Rc::new(deck.masters.clone());
            let rendered = Rc::new(Cell::new(0usize));
            let list = gtk::ListBox::new();
            let scrolled = gtk::ScrolledWindow::new();
            scrolled.set_child(Some(&list));
            let (s, m, r) = (slides.clone(), masters.clone(), rendered.clone());
            super::set_thumbnail_source(&list, &scrolled, Rc::new(move |i| {
                r.set(r.get() + 1);
                super::slide_thumbnail(&s, &m, i)
            }));
            let measure = |name: &str, budget: Duration, f: &mut dyn FnMut()| {
                let mut samples: Vec<Duration> = (0..7).map(|_| { let t = Instant::now(); f(); t.elapsed() }).collect();
                samples.sort_unstable();
                let (p50, p95) = (samples[3], samples[6]);
                eprintln!("{name}: p50={p50:?} p95={p95:?} budget={budget:?}");
                assert!(p95 <= budget, "{name}: p95 {p95:?} over {budget:?}");
            };
            measure("300-slide strip rebuild", Duration::from_millis(1_000), &mut || {
                super::rebuild_slide_list(&list, &slides, &masters, 0);
            });
            let mut rows = 0;
            while list.row_at_index(rows).is_some() {
                rows += 1;
            }
            assert_eq!(rows, 300);
            assert_eq!(rendered.get(), 0, "a rebuild rendered thumbnails for rows not in view");

            let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 1280, 720).unwrap();
            measure("canvas frame of one slide in a 300-slide deck", Duration::from_millis(100), &mut || {
                let cr = cairo::Context::new(&surface).unwrap();
                crate::canvas::draw_slide(&cr, 1280.0, 720.0, &slides, 299, None, &masters, (0.0, 0.5, 1.0));
            });
        });
    }

    #[test]
    fn rows_in_view_is_the_viewport_and_a_screen_either_side() {
        // 300 rows of 120 px; a 600 px viewport at 12,000 px.
        let bands = (0..300).map(|i| (i as f64 * 120.0, 120.0));
        let rows = rows_in_view(bands, 12_000.0, 12_600.0);
        assert_eq!(rows.first(), Some(&94)); // ends exactly at the margin
        assert_eq!(rows.last(), Some(&110));
        assert!(rows.len() < 20, "{} rows", rows.len());
        // Unplaced rows (no bounds yet) are never in view.
        assert!(rows_in_view([(f64::MAX, 0.0)], 0.0, 600.0).is_empty());
    }
}
