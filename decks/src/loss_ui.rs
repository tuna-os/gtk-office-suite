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
/// came from, `source`, has content the save would leave out. Inspection
/// failure blocks replacement, but permits an explicitly unverified copy
/// to a fresh, distinct destination. A new deck has no source.
/// Once a save has gone ahead the file is Decks' own, so the next save
/// doesn't ask.
pub(crate) fn save_after_asking(window: &adw::ApplicationWindow, source: Option<&str>, target: &str, save: impl FnOnce() + 'static) {
    let report = source.map(decks_core::loss::content_a_save_drops);
    if let Some(report) = report.as_ref().filter(|r| r.has_hard_errors()) {
        let details = report.features.iter()
            .filter(|f| f.disposition == suite_common::interop::FeatureDisposition::HardError)
            .map(|f| f.detail.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let recovery_copy = source.is_some_and(|source| decks_core::loss::can_save_unverified_copy(source, target))
            && report.features.iter().filter(|f| f.disposition == suite_common::interop::FeatureDisposition::HardError)
                .all(|f| f.id == "source-inspection-failed");
        if recovery_copy {
            let body = format!(
                "{}\n\n{}",
                suite_common::i18n("Decks could not check the original file. This unverified copy may be missing content that was not imported. Save Copy writes your current edits to a new file without changing the original. The destination must not already exist."),
                details,
            );
            let dialog = adw::AlertDialog::new(Some(&suite_common::i18n("Save an Unverified Copy?")), Some(&body));
            dialog.add_response("cancel", &suite_common::i18n("_Cancel"));
            dialog.add_response("copy", &suite_common::i18n("_Save Copy"));
            dialog.set_response_appearance("copy", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");
            let (source, target, window) = (source.unwrap_or_default().to_string(), target.to_string(), window.clone());
            dialog.choose(Some(&window.clone()), None::<&gio::Cancellable>, move |response| {
                if response == "copy" {
                    if decks_core::loss::can_save_unverified_copy(&source, &target) {
                        save();
                    } else {
                        show_inspection_failure(&window, &details);
                    }
                }
            });
        } else {
            show_inspection_failure(window, &details);
        }
        return;
    }
    // One line per finding, not one label per feature: the choice needs the
    // affected slide/object/part and the reason (stable ids in
    // `decks_core::loss`), so a repair — a copy, a cancelled save, a fixed
    // source — can target that content. Planning never edits the deck and
    // never records history; it only reads the source file.
    let dropped: Vec<String> = report
        .as_ref()
        .map(|r| {
            let mut lines: Vec<String> = Vec::new();
            for f in r.destructive_features() {
                let line = format!("\u{2022} {}: {} \u{2014} {}", suite_common::i18n(&f.label), f.location, f.detail);
                if !lines.contains(&line) {
                    lines.push(line);
                }
            }
            lines
        })
        .unwrap_or_default();
    if dropped.is_empty() {
        save();
        return;
    }
    // Consent is scoped to this source revision, target and feature set: the
    // source is re-inspected when the user confirms, and anything else —
    // a failed inspection, a changed finding list — blocks the save instead
    // of reusing consent given for a different report. Cancelling writes
    // nothing and keeps dirty edits, source bytes and recovery state.
    let consented: Vec<(String, String, String)> = report
        .as_ref()
        .map(|r| {
            r.destructive_features().iter().map(|f| (f.id.clone(), f.location.clone(), f.detail.clone())).collect()
        })
        .unwrap_or_default();
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
    let (source_path, window) = (source.to_string(), window.clone());
    let window_for_save = window.clone();
    dialog.choose(Some(&window), None::<&gio::Cancellable>, move |response| {
        if response == "save" {
            let fresh = decks_core::loss::content_a_save_drops(&source_path);
            if fresh.has_hard_errors() {
                let details = fresh.features.iter()
                    .filter(|f| f.disposition == suite_common::interop::FeatureDisposition::HardError)
                    .map(|f| f.detail.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                show_inspection_failure(&window_for_save, &details);
            } else {
                let now: Vec<(String, String, String)> = fresh.destructive_features().iter()
                    .map(|f| (f.id.clone(), f.location.clone(), f.detail.clone())).collect();
                if now == consented {
                    save();
                } else {
                    show_source_changed(&window_for_save);
                }
            }
        }
    });
}

fn show_source_changed(window: &adw::ApplicationWindow) {
    let body = suite_common::i18n("The original file changed while the question was open, so the listed content may no longer match. Nothing was saved. Try saving again to review what changed.");
    suite_common::dialogs::show_message(Some(window), &suite_common::i18n("Source Changed"), &body);
}

fn show_inspection_failure(window: &adw::ApplicationWindow, details: &str) {
    let body = format!(
        "{}\n\n{}",
        suite_common::i18n("Decks could not check the original file for content this save might lose. Nothing was saved. Make the original file available and try again, or use Save As with a new filename to save an unverified copy of your current edits. An unverified copy cannot replace an existing file."),
        details,
    );
    suite_common::dialogs::show_message(Some(window), &suite_common::i18n("Could Not Save File"), &body);
}

/// Run `save` (a save to `target`), first asking when the imported
/// revision holds content the save would leave out — driven by the
/// session's stored report and provenance (DCK-002), never by re-reading
/// the live source path, which may have been deleted or replaced since
/// open. Read-only over the controller: planning never edits the deck
/// and never records history.
///
/// Consent binds the stored report to the source fingerprint *and* the
/// session revision at dialog time; any source change (re-measured live
/// by content hash, never mtime) or any applied/undone step before the
/// confirmation un-confirms it. A missing or replaced source never
/// authorizes overwriting the original: only the unverified-copy path
/// with its live fresh-destination re-check, as the inspection-failure
/// journey requires. Without a context (new documents, recovered
/// snapshots) this is exactly `save_after_asking`.
pub(crate) fn save_after_asking_with_context(
    window: &adw::ApplicationWindow,
    controller: std::rc::Rc<decks_core::DecksController>,
    source: Option<&str>,
    target: &str,
    save: impl FnOnce() + 'static,
) {
    use decks_core::SourceState;
    let Some(report) = controller.stored_source_loss() else {
        save_after_asking(window, source, target, save);
        return;
    };
    if controller.source_state() == SourceState::NoContext {
        save_after_asking(window, source, target, save);
        return;
    }
    let state = controller.source_state();
    let revision = controller.revision();
    let overwrites =
        source.is_some_and(|s| s == target) || controller.provenance_source_matches(target);
    // A missing or replaced source never authorizes an ordinary save: any
    // write is an unverified copy to a fresh destination, or nothing. The
    // copy flow re-checks destination freshness live, so overwriting and
    // fresh-target saves both route through it.
    if matches!(state, SourceState::Missing | SourceState::Replaced) {
        return save_or_block_unverified_copy(
            window,
            &controller,
            source.unwrap_or_default(),
            target,
            state,
            revision,
            save,
        );
    }
    if report.has_hard_errors() {
        return save_or_block_unverified_copy(
            window,
            &controller,
            source.unwrap_or_default(),
            target,
            state,
            revision,
            save,
        );
    }
    // One line per finding, as `save_after_asking`: the affected
    // slide/object/part and the reason, from the stored scan.
    let mut dropped: Vec<String> = Vec::new();
    for f in report.destructive_features() {
        let line = format!(
            "\u{2022} {}: {} \u{2014} {}",
            suite_common::i18n(&f.label),
            f.location,
            f.detail
        );
        if !dropped.contains(&line) {
            dropped.push(line);
        }
    }
    let source_name = controller
        .provenance_source_name()
        .or_else(|| source.map(str::to_string))
        .unwrap_or_default();
    // A missing or replaced source saved elsewhere: the new file will not
    // have what the original held, so the question says what changed on
    // disk and what is at stake.
    let mut sections: Vec<String> = Vec::new();
    if !dropped.is_empty() {
        let preamble = match state {
            SourceState::Missing => suite_common::i18n(
                "%s is no longer available, and it held content Decks can't keep:",
            )
            .replace("%s", &source_name),
            SourceState::Replaced => suite_common::i18n(
                "%s changed on disk since it was opened, and it held content Decks can't keep:",
            )
            .replace("%s", &source_name),
            _ => suite_common::i18n("%s has content Decks can't keep:").replace("%s", &source_name),
        };
        sections.push(format!("{preamble}\n\n{}", dropped.join("\n")));
    }
    let carried_names = controller.imported_carried_names();
    if matches!(state, SourceState::Missing | SourceState::Replaced)
        && dropped.is_empty()
        && !carried_names.is_empty()
    {
        let parts = carried_names.join(", ");
        sections.push(match state {
            SourceState::Missing => suite_common::i18n(
                "%s is no longer there, so what Decks kept from it (%s) cannot be recovered from the original. Saving recreates the file without those parts.",
            )
            .replacen("%s", &source_name, 1)
            .replacen("%s", &parts, 1),
            _ => suite_common::i18n(
                "%s changed on disk since it was opened. Saving keeps the current edits and does not carry anything over from the changed file (%s).",
            )
            .replacen("%s", &source_name, 1)
            .replacen("%s", &parts, 1),
        });
    }
    // A cross-format Save As drops the carried set whole
    // (`Carried::apply`): the dialog names every part, never silently.
    let cross_format_dropped = controller.carried_names_dropped_for(target);
    if !cross_format_dropped.is_empty() {
        sections.push(
            suite_common::i18n(
                "Saving in the other format will not keep these parts of the original: %s.",
            )
            .replace("%s", &cross_format_dropped.join(", ")),
        );
    }
    if sections.is_empty() {
        save();
        return;
    }
    let outcome = if overwrites {
        suite_common::i18n(
            "Saving replaces the file without them. Save a copy instead to keep the original as it is.",
        )
    } else {
        suite_common::i18n("The new file will not have them; %s keeps them.").replace("%s", &source_name)
    };
    let body = format!("{}\n\n{}", sections.join("\n\n"), outcome);
    let dialog = adw::AlertDialog::new(
        Some(&suite_common::i18n("Save Without This Content?")),
        Some(&body),
    );
    dialog.add_response("cancel", &suite_common::i18n("_Cancel"));
    dialog.add_response("save", &suite_common::i18n("_Save Anyway"));
    dialog.set_response_appearance("save", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let window_for_save = window.clone();
    dialog.choose(Some(window), None::<&gio::Cancellable>, move |response| {
        if response == "save" {
            // The consent was for this source revision, this session
            // revision and this target: a changed file or an intervening
            // edit un-confirms it instead of reusing it.
            if controller.source_state() != state || controller.revision() != revision {
                show_source_changed(&window_for_save);
            } else {
                save();
            }
        }
    });
}

/// The unverified-copy-or-block flow shared by stored hard errors and by
/// missing/replaced sources saved over the original: an explicitly
/// unverified copy to a fresh, distinct destination, or nothing. The
/// fresh-destination gate is re-checked live at confirmation, so a file
/// created while the question was open is never overwritten.
fn save_or_block_unverified_copy(
    window: &adw::ApplicationWindow,
    controller: &std::rc::Rc<decks_core::DecksController>,
    source: &str,
    target: &str,
    state: decks_core::SourceState,
    revision: u64,
    save: impl FnOnce() + 'static,
) {
    let details = if matches!(
        state,
        decks_core::SourceState::Missing | decks_core::SourceState::Replaced
    ) {
        let name = controller
            .provenance_source_name()
            .unwrap_or_else(|| source.to_string());
        let mut at_stake: Vec<String> = controller
            .stored_source_loss()
            .map(|r| {
                r.destructive_features()
                    .iter()
                    .map(|f| format!("{}: {}", f.location, f.detail))
                    .collect()
            })
            .unwrap_or_default();
        at_stake.extend(controller.imported_carried_names());
        let stake = if at_stake.is_empty() {
            String::new()
        } else {
            format!("\n\n{}", at_stake.join("\n"))
        };
        if matches!(state, decks_core::SourceState::Missing) {
            format!("{name} is no longer available.{stake}")
        } else {
            format!("{name} changed on disk since it was opened.{stake}")
        }
    } else {
        controller
            .stored_source_loss()
            .map(|r| {
                r.features
                    .iter()
                    .filter(|f| {
                        f.disposition == suite_common::interop::FeatureDisposition::HardError
                    })
                    .map(|f| f.detail.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    };
    // The copy is offered exactly when the destination is fresh and
    // distinct, and either the origin is gone/changed or every hard error
    // is the inspection failure the copy path exists for — the existing
    // gate, unchanged.
    let copy_offered = decks_core::loss::can_save_unverified_copy(source, target)
        && (matches!(
            state,
            decks_core::SourceState::Missing | decks_core::SourceState::Replaced
        ) || controller.stored_source_loss().is_some_and(|r| {
            r.features
                .iter()
                .filter(|f| f.disposition == suite_common::interop::FeatureDisposition::HardError)
                .all(|f| f.id == "source-inspection-failed")
        }));
    if copy_offered {
        let body = format!(
            "{}\n\n{}",
            if matches!(
                state,
                decks_core::SourceState::Missing | decks_core::SourceState::Replaced
            ) {
                suite_common::i18n("The original file cannot be checked, so this copy may be missing content that was not imported. Save Copy writes your current edits to a new file without changing the original. The destination must not already exist.")
            } else {
                suite_common::i18n("Decks could not check the original file. This unverified copy may be missing content that was not imported. Save Copy writes your current edits to a new file without changing the original. The destination must not already exist.")
            },
            details,
        );
        let dialog = adw::AlertDialog::new(
            Some(&suite_common::i18n("Save an Unverified Copy?")),
            Some(&body),
        );
        dialog.add_response("cancel", &suite_common::i18n("_Cancel"));
        dialog.add_response("copy", &suite_common::i18n("_Save Copy"));
        dialog.set_response_appearance("copy", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let (source_path, target_path, window) =
            (source.to_string(), target.to_string(), window.clone());
        let window_for_save = window.clone();
        let controller = controller.clone();
        dialog.choose(
            Some(&window.clone()),
            None::<&gio::Cancellable>,
            move |response| {
                if response == "copy" {
                    // The destination gate and the session binding are both
                    // re-checked live: a file created while the question was
                    // open, a changed source, or an intervening edit blocks.
                    if controller.revision() != revision {
                        show_source_changed(&window_for_save);
                    } else if decks_core::loss::can_save_unverified_copy(&source_path, &target_path)
                    {
                        save();
                    } else {
                        show_inspection_failure(&window_for_save, &details);
                    }
                }
            },
        );
    } else {
        show_inspection_failure(window, &details);
    }
}
