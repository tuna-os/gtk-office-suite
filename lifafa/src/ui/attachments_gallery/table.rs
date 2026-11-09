//! One row of the gallery's table view.

use adw::prelude::*;
use relm4::ComponentSender;

use crate::i18n::i18n;
use crate::models::{ext_of, GalleryItem};

use super::cell::{add_item_gestures, add_quick_actions, deleting_over};
use super::labels::date_text;
use super::{
    icon_color_class, icon_for, thumbnail_texture, AttachmentsGallery, Thumbnail,
    TABLE_ACTIONS_WIDTH,
};

/// One table row: mini thumbnail/type icon, name, sender, type, date, size —
/// the same widths as the header buttons, so the columns line up.
pub(super) fn build_row(
    index: usize,
    item: &GalleryItem,
    deleting: bool,
    sender: &ComponentSender<AttachmentsGallery>,
) -> gtk::ListBoxRow {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    line.add_css_class("gallery-table-row");

    // Cache-only mini thumbnail: a decode already paid for is shown, but a
    // 24px row never spawns a render of its own.
    let lead: gtk::Widget = match item.data.as_ref().map(|d| thumbnail_texture(&item.name, d)) {
        Some(Thumbnail::Ready(tex)) => {
            let pic = gtk::Picture::for_paintable(&tex);
            pic.set_content_fit(gtk::ContentFit::Cover);
            pic.set_size_request(28, 28);
            pic.add_css_class("gallery-table-thumb");
            pic.upcast()
        }
        _ => {
            let img = gtk::Image::from_icon_name(icon_for(&item.name));
            img.set_pixel_size(20);
            img.set_size_request(28, 28);
            img.add_css_class("gallery-file-icon");
            img.add_css_class(icon_color_class(&item.name));
            img.upcast()
        }
    };
    line.append(&lead);

    let name = gtk::Label::new(Some(&item.name));
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    name.set_tooltip_text(Some(&format!("{} — {}", item.subject, item.human_size())));
    line.append(&name);

    let sender_label = gtk::Label::new(Some(&item.from_name));
    sender_label.set_xalign(0.0);
    sender_label.set_width_request(170);
    sender_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    sender_label.set_max_width_chars(1);
    line.append(&sender_label);

    // Every fixed-width cell is clipped to its column (the width request is
    // the floor, the one-char max the ceiling): a value wider than its
    // column would otherwise widen the cell, and with the row's width
    // fixed, the expanding Name cell would give up the difference — sliding
    // Sender left of its header on that row alone.
    let fixed = |text: &str, width: i32, xalign: f32| {
        let l = gtk::Label::new(Some(text));
        l.set_xalign(xalign);
        l.set_width_request(width);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        l.set_max_width_chars(1);
        l.add_css_class("dim-label");
        l
    };
    // No extension: say so, rather than showing the name again.
    let ext = ext_of(&item.name);
    let kind_text = if ext.is_empty() {
        i18n("File")
    } else {
        ext.to_ascii_uppercase()
    };
    let kind = fixed(&kind_text, 90, 0.0);
    line.append(&kind);

    let date = fixed(&date_text(item.timestamp), 110, 0.0);
    line.append(&date);

    let size = fixed(&item.human_size(), 90, 1.0);
    size.add_css_class("numeric");
    line.append(&size);

    // The same quick actions the grid cells carry, as a trailing column.
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    actions.set_width_request(TABLE_ACTIONS_WIDTH);
    actions.set_halign(gtk::Align::End);
    add_quick_actions(&actions, item, index, sender, |icon, tip| {
        let b = gtk::Button::from_icon_name(icon);
        b.add_css_class("flat");
        b.set_valign(gtk::Align::Center);
        b.set_tooltip_text(Some(tip));
        b
    });
    if deleting {
        line.append(&deleting_over(&actions));
    } else {
        line.append(&actions);
    }

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&line));
    if deleting {
        // The ground on the row itself, where the hover and selection
        // highlights are drawn, so it has their shape.
        row.add_css_class("attachment-deleting");
        row.set_can_target(false);
        return row;
    }

    // The same gestures the grid cells carry: right-click for the context
    // menu, double-click to open externally (single click previews).
    add_item_gestures(&row, index, sender);
    row
}
