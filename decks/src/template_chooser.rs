//! template_chooser.rs — New from Template: the built-in themes, each
//! previewed by the canvas's own renderer.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! docs/DESIGN-UI.md, "Templates that look finished" (Keynote's theme
//! chooser): an AdwDialog with a grid of themes, one chosen at a time,
//! and Cancel / Create in its header bar. Each tile is the theme's title
//! slide drawn by `canvas::draw_slide` (through `sidebar::render_thumbnail`)
//! from the deck `decks_core::templates::deck` builds, so what the tile
//! shows is what Create opens. The tiles are a group of toggle buttons: a
//! choice among a few, reachable by keyboard and by assistive technology.

use std::rc::Rc;

use adw::prelude::*;
use decks_core::templates::{deck, templates};
use gtk4 as gtk;
use libadwaita as adw;

const PREVIEW_W: i32 = 224;
const PREVIEW_H: i32 = 126;

/// A theme's tile: its title slide over its name.
fn tile(index: usize, name: &str, description: &str) -> gtk::ToggleButton {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
    if let Some((slides, masters)) = deck(index) {
        if let Some(preview) = crate::sidebar::render_thumbnail(&slides, &masters, 0, PREVIEW_W, PREVIEW_H, Some(crate::canvas::Chrome::Preview)) {
            preview.add_css_class("card");
            preview.set_can_shrink(false);
            preview.set_alternative_text(Some(name));
            body.append(&preview);
        }
    }
    let label = gtk::Label::new(Some(name));
    body.append(&label);
    let button = gtk::ToggleButton::builder().child(&body).tooltip_text(description).build();
    button.add_css_class("flat");
    button.update_property(&[gtk::accessible::Property::Label(name), gtk::accessible::Property::Description(description)]);
    button
}

/// Show the chooser over `parent`. Create calls `choose` with the chosen
/// template's index; Cancel (or Escape) chooses nothing.
pub fn present(parent: &impl IsA<gtk::Widget>, choose: impl Fn(usize) + 'static) {
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(3)
        .row_spacing(18)
        .column_spacing(18)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .valign(gtk::Align::Start)
        .build();
    let mut tiles: Vec<gtk::ToggleButton> = Vec::new();
    for (i, t) in templates().iter().enumerate() {
        let button = tile(i, t.name, t.description);
        if let Some(first) = tiles.first() {
            button.set_group(Some(first));
        }
        grid.append(&button);
        tiles.push(button);
    }
    if let Some(first) = tiles.first() {
        first.set_active(true);
    }
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(&grid)
        .build();

    let cancel = gtk::Button::with_mnemonic("_Cancel");
    let create = gtk::Button::with_label("Create");
    create.add_css_class("suggested-action");
    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    header.pack_start(&cancel);
    header.pack_end(&create);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&scrolled));

    let dialog = adw::Dialog::builder().title("Choose a Theme").content_width(800).child(&view).build();
    // Enter creates, as in any dialog with a suggested action.
    dialog.set_default_widget(Some(&create));
    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| {
            dialog.close();
        });
    }
    let tiles = Rc::new(tiles);
    {
        let dialog = dialog.clone();
        let tiles = tiles.clone();
        create.connect_clicked(move |_| {
            let chosen = tiles.iter().position(|t| t.is_active()).unwrap_or(0);
            dialog.close();
            choose(chosen);
        });
    }
    dialog.present(Some(parent));
    if let Some(first) = tiles.first() {
        first.grab_focus();
    }
}
