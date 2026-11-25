//! Per-account colour classes for the GTK chrome around the page.

use super::MessageView;
use crate::i18n::{i18n, ni18n_f};

use adw::prelude::*;
use relm4::prelude::*;

/// Style rules for a [`DisplayCss`](crate::ui::DisplayCss), each named by a
/// class. A message's own colors used to be the one rule a provider held, so
/// every message opened reloaded it and restyled the whole window (#323);
/// kept side by side, they leave a class change on one widget to move
/// between them.
pub(super) struct ClassRules {
    css: crate::ui::DisplayCss,
    rules: std::cell::RefCell<std::collections::BTreeMap<String, String>>,
}

impl ClassRules {
    pub(super) fn new() -> Self {
        ClassRules {
            css: crate::ui::DisplayCss::new(),
            rules: Default::default(),
        }
    }

    /// Add or replace rules, as (selector, declarations).
    pub(super) fn set(&self, new: Vec<(String, String)>) {
        let mut rules = self.rules.borrow_mut();
        rules.extend(new);
        self.css.load(
            rules
                .iter()
                .map(|(s, d)| format!("{s} {{ {d} }}\n"))
                .collect(),
        );
    }
}

/// Give `widget` the class `class` in place of any other starting with
/// `prefix`. Nothing is touched when it already wears it.
pub(super) fn wear_class(widget: &impl IsA<gtk::Widget>, prefix: &str, class: &str) {
    if widget.has_css_class(class) {
        return;
    }
    for c in widget.css_classes() {
        if c.starts_with(prefix) {
            widget.remove_css_class(&c);
        }
    }
    widget.add_css_class(class);
}

impl MessageView {
    /// What the empty pane is titled: how many rows are checked, while
    /// Gmail's x has some (#392).
    pub(super) fn empty_title(&self) -> String {
        let n = self.checked.0;
        if n > 0 {
            let count = n.to_string();
            if self.checked.1 {
                ni18n_f(
                    "{n} conversation selected",
                    "{n} conversations selected",
                    n as u32,
                    &[("n", &count)],
                )
            } else {
                ni18n_f(
                    "{n} message selected",
                    "{n} messages selected",
                    n as u32,
                    &[("n", &count)],
                )
            }
        } else if self.drafts_view {
            i18n("No draft selected")
        } else {
            i18n("No message selected")
        }
    }

    /// Whether the GTK header carries tag chips: a lone message shown
    /// full-bleed (not as a card) that has a tag or a Gmail label to show.
    pub(super) fn header_tags_shown(&self) -> bool {
        self.thread.len() == 1
            && !self.single_message_card
            && self
                .current
                .as_ref()
                .is_some_and(|c| !c.pills(&self.tags).is_empty())
    }

    /// Rebuild the GTK header's chips when the shown message's keywords or
    /// labels (or the tags) changed since they were last built.
    pub(super) fn sync_header_tags(&self, header_tags: &gtk::Box) {
        let shown = self
            .current
            .as_ref()
            .map(|c| (c.keywords.clone(), c.labels.clone()))
            .unwrap_or_default();
        if self.header_tags_rendered.borrow().as_ref() == Some(&shown) {
            return;
        }
        while let Some(child) = header_tags.first_child() {
            header_tags.remove(&child);
        }
        for pill in crate::models::pills(&shown.0, &shown.1, &self.tags) {
            let chip = crate::ui::message_row::pill_label(&pill);
            chip.set_ellipsize(gtk::pango::EllipsizeMode::End);
            chip.set_max_width_chars(20);
            header_tags.append(&chip);
        }
        *self.header_tags_rendered.borrow_mut() = Some(shown);
    }
}
