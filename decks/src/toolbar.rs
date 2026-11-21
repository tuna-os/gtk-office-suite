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

/// Build the Decks editing toolbar with formatting and present buttons.
pub fn build_decks_toolbar() -> gtk::Box {
    // libadwaita's toolbar style pads the bar on every side; with only
    // side margins its buttons sat flush against the header bar.
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    toolbar.add_css_class("toolbar");

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


    // The Insert buttons go between these and Present (insert_bar.rs).

    let present = gtk::Button::builder()
        .icon_name("view-fullscreen-symbolic").tooltip_text("Present (F5)").build();
    present.set_margin_start(12);
    present.add_css_class("flat");
    present.add_css_class("suggested-action");
    toolbar.append(&present);

    toolbar
}
