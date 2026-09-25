//! insert_bar.rs — the Insert buttons in the header bar.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! docs/DESIGN-UI.md, "Insert buttons, not menus" (iWork's Text · Shape ·
//! Table · Media row): labelled, flat header-bar buttons, and a Shape
//! popover that is a searchable library of shapes, each drawn by the
//! canvas's own shape code. What each button inserts is decks_core::insert;
//! the buttons only fire the app actions (add-text-box, insert-shape,
//! insert-table, add-image), so keyboard and automation reach the same
//! code.

use adw::prelude::*;
use decks_core::engine::shape::ShapeStyle;
use decks_core::insert::{matches, shape_library};
use gtk4::{self as gtk, glib};
use libadwaita as adw;

/// A flat header-bar button with an icon and a label.
fn labelled(icon: &str, label: &str, tooltip: &str) -> adw::ButtonContent {
    let content = adw::ButtonContent::builder().icon_name(icon).label(label).use_underline(true).build();
    content.set_tooltip_text(Some(tooltip));
    content
}

fn button(icon: &str, label: &str, tooltip: &str, action: &str) -> gtk::Button {
    let b = gtk::Button::builder().child(&labelled(icon, label, tooltip)).action_name(action).build();
    b.add_css_class("flat");
    // AdwButtonContent labels the button by its visible label ("Table"),
    // and the tooltip ("Insert Table") becomes its accessible description.
    b.set_tooltip_text(Some(tooltip));
    b
}

/// A small drawing of a library shape, in the default style.
fn preview(kind: &decks_core::engine::shape::ShapeKind) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(56);
    area.set_content_height(40);
    let kind = kind.clone();
    area.set_draw_func(move |_, cr, w, h| {
        let style = ShapeStyle::default();
        crate::canvas::draw_shape(cr, &kind, &style, (4.0, 4.0, w as f64 - 8.0, h as f64 - 8.0), 1.0);
    });
    area
}

/// The Shape button: a popover with a search entry over the library.
fn shape_menu() -> gtk::MenuButton {
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search Shapes"));
    search.update_property(&[gtk::accessible::Property::Label("Search Shapes")]);
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(3)
        .min_children_per_line(3)
        .row_spacing(6)
        .column_spacing(6)
        .homogeneous(true)
        .build();
    let popover = gtk::Popover::new();
    let library = shape_library();
    for (i, s) in library.iter().enumerate() {
        let tile = gtk::Box::new(gtk::Orientation::Vertical, 6);
        tile.append(&preview(&s.kind));
        let label = gtk::Label::new(Some(s.name));
        label.add_css_class("caption");
        label.set_wrap(true);
        label.set_justify(gtk::Justification::Center);
        label.set_max_width_chars(10);
        tile.append(&label);
        let b = gtk::Button::builder().child(&tile).build();
        b.add_css_class("flat");
        b.update_property(&[gtk::accessible::Property::Label(s.name)]);
        b.set_action_name(Some("app.insert-shape"));
        b.set_action_target_value(Some(&(i as u32).to_variant()));
        let p = popover.clone();
        b.connect_clicked(move |_| p.popdown());
        grid.append(&b);
    }
    {
        let search = search.clone();
        grid.set_filter_func(move |child| {
            let i = child.index() as usize;
            shape_library().get(i).is_some_and(|s| matches(s, &search.text()))
        });
    }
    {
        let grid = grid.clone();
        search.connect_search_changed(move |_| grid.invalidate_filter());
    }
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.set_margin_top(6);
    body.set_margin_bottom(6);
    body.set_margin_start(6);
    body.set_margin_end(6);
    body.append(&search);
    body.append(&grid);
    popover.set_child(Some(&body));
    // Typing finds a shape straight away.
    {
        let search = search.clone();
        popover.connect_show(move |_| {
            search.set_text("");
            let s = search.clone();
            glib::idle_add_local_once(move || {
                s.grab_focus();
            });
        });
    }
    let menu = gtk::MenuButton::builder().child(&labelled("insert-object-symbolic", "_Shape", "Insert Shape")).popover(&popover).build();
    menu.add_css_class("flat");
    menu.set_tooltip_text(Some("Insert Shape"));
    menu.update_property(&[gtk::accessible::Property::Label("Insert Shape")]);
    menu
}

/// Pack the Insert buttons at the start of `header`: Text, Shape, Table,
/// Image.
pub fn build(header: &adw::HeaderBar) {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bar.append(&button("insert-text-symbolic", "_Text", "Insert Text Box", "app.add-text-box"));
    bar.append(&shape_menu());
    bar.append(&button("x-office-spreadsheet-symbolic", "T_able", "Insert Table", "app.insert-table"));
    bar.append(&button("insert-image-symbolic", "_Image", "Insert Image", "app.add-image"));
    header.pack_start(&bar);
}
