//! notes_pane.rs — the speaker notes pane under the slide.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Google Slides' and Keynote's presenter notes: a pane below the canvas
//! (beside the slide list, left of the inspector), its height set by
//! dragging the divider, with a placeholder when the slide has none. The
//! text is the slide's `notes`; every change goes through the controller
//! (`DecksController::set_notes`), so notes are undone with the deck's
//! history, a word at a time, and the deck's own undo is the only one: the
//! buffer's built-in undo is off.

use gtk4::{self as gtk, prelude::*};

pub struct NotesPane {
    /// The pane, for the divider's end child.
    pub widget: gtk::Box,
    pub buffer: gtk::TextBuffer,
    /// The text itself, for "Go to Speaker Notes" to focus.
    pub view: gtk::TextView,
}

pub fn build() -> NotesPane {
    let heading = gtk::Label::builder()
        .label("Speaker Notes")
        .xalign(0.0)
        .margin_start(12)
        .margin_top(6)
        .build();
    heading.add_css_class("caption-heading");
    heading.add_css_class("dim-label");

    let buffer = gtk::TextBuffer::new(None);
    buffer.set_enable_undo(false);
    let view = gtk::TextView::builder()
        .buffer(&buffer)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(12)
        .right_margin(12)
        .top_margin(6)
        .bottom_margin(12)
        .build();
    view.update_property(&[gtk::accessible::Property::Label("Speaker notes")]);
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&view)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(72)
        .vexpand(true)
        .build();

    // "Click to add speaker notes" while the slide has none.
    let placeholder = gtk::Label::builder()
        .label("Click to add speaker notes")
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .margin_start(12)
        .margin_top(6)
        .can_target(false)
        .build();
    placeholder.add_css_class("dim-label");
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&scrolled));
    overlay.add_overlay(&placeholder);
    {
        let placeholder = placeholder.clone();
        buffer.connect_changed(move |b| placeholder.set_visible(b.char_count() == 0));
    }

    let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
    widget.add_css_class("view");
    widget.append(&heading);
    widget.append(&overlay);
    NotesPane { widget, buffer, view }
}
