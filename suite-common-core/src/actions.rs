// actions.rs — GTK-free action label registry.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Every user-facing action an app registers gets a human-readable label
// here. The registry feeds the keyboard-shortcuts dialog and the command
// palette, and the palette-coverage test asserts that every GioAction an
// app exposes has an entry — the forcing function that keeps actions
// named and discoverable (DESIGN-UI.md §surfacing).

use std::cell::RefCell;

/// One labeled action: (`"app.bold"`, `"Bold"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionLabel {
    pub name: String,
    pub label: String,
}

thread_local! {
    static REGISTRY: RefCell<Vec<ActionLabel>> = const { RefCell::new(Vec::new()) };
}

/// Register labels for actions. Re-registering a name replaces its label,
/// so per-window setup can run more than once without duplicating entries.
pub fn register_labels(entries: &[(&str, &str)]) {
    REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        for (name, label) in entries {
            if let Some(e) = reg.iter_mut().find(|e| e.name == *name) {
                e.label = (*label).to_string();
            } else {
                reg.push(ActionLabel { name: (*name).to_string(), label: (*label).to_string() });
            }
        }
    });
}

/// All labeled actions, in registration order.
pub fn labeled_actions() -> Vec<ActionLabel> {
    REGISTRY.with(|r| r.borrow().clone())
}

/// Look up the label for a fully qualified action name.
pub fn label_for(name: &str) -> Option<String> {
    REGISTRY.with(|r| r.borrow().iter().find(|e| e.name == name).map(|e| e.label.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_lookup() {
        register_labels(&[("app.bold", "Bold"), ("app.italic", "Italic")]);
        assert_eq!(label_for("app.bold").as_deref(), Some("Bold"));
        assert!(labeled_actions().len() >= 2);
    }

    #[test]
    fn reregistering_replaces_not_duplicates() {
        register_labels(&[("app.x-test", "One")]);
        register_labels(&[("app.x-test", "Two")]);
        let hits: Vec<_> =
            labeled_actions().into_iter().filter(|e| e.name == "app.x-test").collect();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].label, "Two");
    }

    #[test]
    fn unknown_action_has_no_label() {
        assert_eq!(label_for("app.does-not-exist"), None);
    }
}
