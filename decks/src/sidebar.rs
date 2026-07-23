// sidebar.rs — Slide sidebar list and management controls.
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk4::{self as gtk, gdk, glib, prelude::*};
use gtk4::cairo;
use decks_core::engine::{MasterSlide, Slide};

const THUMB_W: i32 = 160;
const THUMB_H: i32 = 90;

/// Offscreen render of one slide as a texture (the strip's preview).
fn slide_thumbnail(
    slides: &[Slide],
    masters: &[MasterSlide],
    index: usize,
) -> Option<gtk::Picture> {
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, THUMB_W, THUMB_H).ok()?;
    {
        let cr = cairo::Context::new(&surface).ok()?;
        crate::canvas::draw_slide(
            &cr,
            THUMB_W as f64,
            THUMB_H as f64,
            slides,
            index,
            None,
            masters,
            (0.0, 0.5, 1.0), // thumbnails never show selection; unused
        );
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().ok()?.to_vec();
    let bytes = glib::Bytes::from_owned(data);
    let texture = gdk::MemoryTexture::new(
        THUMB_W,
        THUMB_H,
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &bytes,
        stride,
    );
    let pic = gtk::Picture::for_paintable(&texture);
    pic.set_size_request(THUMB_W, THUMB_H);
    pic.set_can_shrink(true);
    Some(pic)
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
    for (i, _slide) in slides.iter().enumerate() {
        let row = gtk::ListBoxRow::new();
        let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
        column.set_margin_start(8);
        column.set_margin_end(8);
        column.set_margin_top(6);
        column.set_margin_bottom(6);
        if let Some(pic) = slide_thumbnail(slides, masters, i) {
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
}
