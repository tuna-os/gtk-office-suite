// SPDX-License-Identifier: GPL-3.0-or-later
//
// collab_status.rs — the two widgets a session adds to a Letters window
// (RFC-0005 5.1): the header bar's status button and the two-versions banner.
//
// Neither exists for a document that is not shared: the button is hidden and
// the banner is not revealed. Colour is never the only signal: the button is
// an icon with a tooltip and an accessible name that says the state in words.

use gtk4::{self as gtk, prelude::*};
use libadwaita as adw;

use crate::collab_logic::Words;

/// What the status popover shows.
pub(crate) struct StatusModel {
    pub words: Words,
    pub people: String,
    pub last_update: Option<String>,
    pub problem: Option<String>,
}

/// The header bar's sharing button and its popover.
#[derive(Clone)]
pub(crate) struct Status {
    pub button: gtk::MenuButton,
    people: gtk::Label,
    last: gtk::Label,
    problem: gtk::Label,
}

fn label(css: &[&str]) -> gtk::Label {
    let l = gtk::Label::builder().xalign(0.0).wrap(true).selectable(false).build();
    for c in css {
        l.add_css_class(c);
    }
    l
}

impl Status {
    pub(crate) fn new() -> Status {
        let (people, last, problem) = (label(&[]), label(&["caption", "dim-label"]), label(&["error"]));
        let copy = gtk::Button::with_label(&suite_common::i18n("Copy Join Code"));
        copy.set_action_name(Some("app.collab-copy-code"));
        let stop = gtk::Button::with_label(&suite_common::i18n("Stop Sharing"));
        stop.set_action_name(Some("app.collab-leave"));
        stop.add_css_class("destructive-action");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        for w in [people.upcast_ref::<gtk::Widget>(), last.upcast_ref(), problem.upcast_ref(), copy.upcast_ref(), stop.upcast_ref()] {
            content.append(w);
        }
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&content));
        let button = gtk::MenuButton::builder().icon_name("network-transmit-receive-symbolic").popover(&popover).visible(false).build();
        Status { button, people, last, problem }
    }

    /// Show `model`, or hide the button when the selected document is not in
    /// a session.
    pub(crate) fn update(&self, model: Option<&StatusModel>) {
        let Some(m) = model else {
            self.button.set_visible(false);
            return;
        };
        self.button.set_icon_name(m.words.icon);
        self.button.set_tooltip_text(Some(&m.words.label));
        self.button.update_property(&[gtk::accessible::Property::Label(&m.words.label)]);
        self.people.set_text(&m.people);
        self.last.set_text(m.last_update.as_deref().unwrap_or(""));
        self.last.set_visible(m.last_update.is_some());
        self.problem.set_text(m.problem.as_deref().unwrap_or(""));
        self.problem.set_visible(m.problem.is_some());
        self.button.set_visible(true);
    }
}

/// The banner of a document that has two versions this session does not
/// hold both of (RFC-0005 2.5): a lasting state of the document, so a banner
/// and not a toast. Its button opens the Resolve dialog.
pub(crate) fn banner() -> adw::Banner {
    let b = adw::Banner::builder()
        .title(suite_common::i18n("Two versions of this document exist"))
        .button_label(suite_common::i18n("Resolve…"))
        .action_name("app.collab-resolve")
        .build();
    b.set_revealed(false);
    b
}
