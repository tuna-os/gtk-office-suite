//! loss_ui.rs — ask before a save drops content Decks can't keep.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! decks-readiness.md: "Missing media and unsupported animation/comment
//! content is preserved or blocked/warned by #374 before save." What is
//! dropped is `decks_core::loss` (read from the file the deck came from);
//! this is the question, Letters' "Save as …?" for a format that can't hold
//! a document (`letters/src/window/saving.rs`). Cancel writes nothing.

use adw::prelude::*;
use gtk4::gio;
use libadwaita as adw;

/// Run `save` (a save to `target`), first asking when the file the deck
/// came from, `source`, has content the save would leave out. Once a save
/// has gone ahead the file is Decks' own, so the next save doesn't ask.
pub(crate) fn save_after_asking(window: &adw::ApplicationWindow, source: Option<&str>, target: &str, save: impl FnOnce() + 'static) {
    let report = source.map(decks_core::loss::content_a_save_drops);
    let dropped: Vec<String> = report
        .as_ref()
        .map(|r| {
            let mut labels: Vec<String> = Vec::new();
            for f in r.destructive_features() {
                let label = format!("\u{2022} {}", suite_common::i18n(&f.label));
                if !labels.contains(&label) {
                    labels.push(label);
                }
            }
            labels
        })
        .unwrap_or_default();
    if dropped.is_empty() {
        save();
        return;
    }
    let source = source.unwrap_or_default();
    let name = |p: &str| std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let outcome = if source == target {
        suite_common::i18n("Saving replaces the file without them. Save a copy instead to keep the original as it is.")
    } else {
        suite_common::i18n("The new file will not have them; %s keeps them.").replace("%s", &name(source))
    };
    let body = format!(
        "{}\n\n{}\n\n{}",
        suite_common::i18n("%s has content Decks can't keep:").replace("%s", &name(source)),
        dropped.join("\n"),
        outcome
    );
    let dialog = adw::AlertDialog::new(Some(&suite_common::i18n("Save Without This Content?")), Some(&body));
    dialog.add_response("cancel", &suite_common::i18n("_Cancel"));
    dialog.add_response("save", &suite_common::i18n("_Save Anyway"));
    dialog.set_response_appearance("save", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.choose(Some(window), None::<&gio::Cancellable>, move |response| {
        if response == "save" {
            save();
        }
    });
}
