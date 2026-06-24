// toolbar.rs — Decks editing toolbar.
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk4::{self as gtk, prelude::*};

/// Find a toolbar button by its icon name.
pub fn find_toolbar_child(toolbar: &gtk::Box, icon: &str) -> Option<gtk::Button> {
    let mut iter = toolbar.first_child();
    while let Some(child) = iter {
        if let Ok(btn) = child.clone().downcast::<gtk::Button>() {
            if btn.icon_name().map(|n| n == icon).unwrap_or(false) {
                return Some(btn);
            }
        }
        iter = child.next_sibling();
    }
    None
}

/// Build the Decks editing toolbar with formatting, insert, and present buttons.
pub fn build_decks_toolbar() -> gtk::Box {
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    toolbar.set_margin_start(6);
    toolbar.set_margin_end(6);
    toolbar.set_halign(gtk::Align::Center);

    let bold = gtk::ToggleButton::builder()
        .icon_name("format-text-bold-symbolic").tooltip_text("Bold").build();
    let italic = gtk::ToggleButton::builder()
        .icon_name("format-text-italic-symbolic").tooltip_text("Italic").build();
    let underline = gtk::ToggleButton::builder()
        .icon_name("format-text-underline-symbolic").tooltip_text("Underline").build();
    for btn in [&bold, &italic, &underline] {
        btn.add_css_class("flat");
        toolbar.append(btn);
    }

    let sep = gtk::Separator::new(gtk::Orientation::Vertical);
    sep.set_margin_start(6); sep.set_margin_end(6);
    toolbar.append(&sep);

    let text_box = gtk::Button::builder()
        .icon_name("insert-text-symbolic").tooltip_text("Add Text Box").build();
    text_box.add_css_class("flat");
    toolbar.append(&text_box);

    let shape = gtk::Button::builder()
        .icon_name("insert-object-symbolic").tooltip_text("Add Shape (Rect/Circle)").build();
    shape.add_css_class("flat");
    toolbar.append(&shape);

    let image = gtk::Button::builder()
        .icon_name("insert-image-symbolic").tooltip_text("Add Image").build();
    image.add_css_class("flat");
    toolbar.append(&image);

    let sep2 = gtk::Separator::new(gtk::Orientation::Vertical);
    toolbar.append(&sep2);

    let present = gtk::Button::builder()
        .icon_name("view-fullscreen-symbolic").tooltip_text("Present (F5)").build();
    present.add_css_class("flat");
    present.add_css_class("suggested-action");
    toolbar.append(&present);

    toolbar
}
