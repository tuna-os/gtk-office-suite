// SPDX-License-Identifier: GPL-3.0-or-later
// GTK save adapter. All entry points use one session transaction and writer.

use super::tab_data_get;
use adw::prelude::*;
use gtk4::{self as gtk, gio, prelude::*};
use libadwaita as adw;
use std::path::Path;
use std::rc::Rc;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SaveOutcome {
    Saved,
    Cancelled,
    Failed(String),
}

fn show_message(page: &adw::TabPage, title: &str, message: &str) {
    let dialog = adw::AlertDialog::new(Some(&suite_common::i18n(title)), Some(message));
    dialog.add_response("ok", &suite_common::i18n("_OK"));
    dialog.set_close_response("ok");
    // A failed save leaves the edit session intact; put the caret back
    // where the user was once the error is dismissed (LET-001 step 4).
    if let Some(view) = crate::page_container::find(&page.child()).and_then(|pc| pc.page_view()) {
        dialog.connect_closed(move |_| {
            view.grab_focus();
        });
    }
    dialog.present(page.child().root().and_downcast::<gtk::Window>().as_ref());
}

fn save_page_to_path(page: &adw::TabPage, path: &Path) -> SaveOutcome {
    let child = page.child();
    let Some(td) = tab_data_get(&child) else {
        return SaveOutcome::Failed(suite_common::i18n("Document session is unavailable."));
    };
    let Some(buf) = crate::page_container::buffer_of(&child) else {
        return SaveOutcome::Failed(suite_common::i18n("Document editor is unavailable."));
    };
    // The write stays inside `save_to` so a failure cannot advance the
    // savepoint. What the format drops was asked about before this
    // (`save_asking_about_loss`), so the report it returns is not shown again.
    // Parts the editor doesn't model but that are safe to keep (custom XML,
    // a thumbnail, ODF settings) come from the imported revision pinned at
    // open — never a re-read of the live source path, which may have been
    // deleted or replaced since (#1274, LET-001).
    let carried = td.0.borrow().carried_for_save();
    let result = td.0.borrow_mut().save_to(path.to_path_buf(), |path| {
        carried.write_with(path, |p| crate::bridge::save_buffer_to_file(&buf, p).map(|_| ()))?;
        // A document at a remote location is uploaded from its staged copy
        // (RFC-0003), inside the transaction so a failed upload doesn't
        // count as saved.
        suite_common::locations::commit_save(path)
    });
    let commit = match result {
        Ok(commit) => commit,
        Err(error) => return SaveOutcome::Failed(error),
    };
    // Session borrow is released before GTK emits modified/title notifications.
    buf.set_modified(false);
    page.set_needs_attention(false);
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        page.set_title(name);
    }
    page.set_tooltip(&suite_common::locations::remote_uri(path).unwrap_or_else(|| path.to_string_lossy().into_owned()));
    // The recent-files list is stored as UTF-8 strings in GSettings, so a
    // path that is not UTF-8 saves normally and simply does not appear
    // there — better than refusing the save, which is what Letters used to
    // do for any such name.
    if let Some(path_str) = path.to_str() {
        suite_common::push_recent_file(&gio::Settings::new("org.tunaos.letters"), path_str);
    }
    if let Some(warning) = commit.recovery_warning {
        show_message(page, "Document saved; recovery cleanup failed", &warning);
    }
    // A shared document's save is also published as a version (RFC-0005 2.3);
    // a failed publish never fails the save.
    #[cfg(feature = "collab")]
    crate::collab_session::after_save(&buf, path);
    SaveOutcome::Saved
}

/// The features `report` says the format drops, one bullet each, or `None`
/// when it holds everything.
///
/// The report is built from the document's actual contents (see
/// `letters_core::save`), so this stays quiet for an unstyled document
/// saved as plain text rather than warning on every `.txt` save.
fn dropped_features(report: &suite_common::interop::CompatibilityReport) -> Option<String> {
    let dropped: Vec<String> = report
        .destructive_features()
        .iter()
        .map(|feature| format!("\u{2022} {}", suite_common::i18n(&feature.label)))
        .collect();
    (!dropped.is_empty()).then(|| dropped.join("\n"))
}

/// Where the page's last "Save Anyway" was given: asked once per target,
/// not on every Ctrl+S into the same file.
const LOSS_CONFIRMED_KEY: &str = "letters-loss-confirmed-path";

/// Save to `path`, but first ask when its format cannot hold what the
/// document has (#1206). Cancel writes nothing: the file on disk keeps its
/// bytes and the tab stays unsaved. Once the user saves anyway, later saves
/// to the same path don't ask again.
fn save_asking_about_loss(
    page: &adw::TabPage,
    path: &Path,
    complete: impl FnOnce(SaveOutcome) + 'static,
) {
    use letters_core::session::SourceState;
    let finish = {
        let page = page.clone();
        move |outcome: SaveOutcome, complete: Box<dyn FnOnce(SaveOutcome)>| {
            if let SaveOutcome::Failed(error) = &outcome {
                show_message(&page, "Could Not Save File", error);
            }
            complete(outcome);
        }
    };
    let child = page.child();
    // The imported revision's state, read once: whether the source still
    // resolves to the bytes the tab opened, what that revision held that no
    // save can keep, and whether this save would overwrite that source.
    // Sessions without provenance (untitled tabs) keep the old live read.
    let (source_state, source_report, source_name, overwrites_source, at_stake, carried_names) =
        tab_data_get(&child)
            .map(|td| {
                let session = td.0.borrow();
                let state = session.source_state();
                let (report, name, overwrites) = match state {
                    SourceState::NoContext => {
                        let live = session.file.clone();
                        (
                            live.as_ref().map(|p| letters_core::loss::content_a_save_drops(p)),
                            live.as_ref()
                                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())),
                            live.as_deref() == Some(path),
                        )
                    }
                    _ => (
                        session.stored_source_loss().cloned(),
                        session.provenance_source_name(),
                        session.provenance_source_matches(path),
                    ),
                };
                (
                    state,
                    report,
                    name,
                    overwrites,
                    session.has_unrecoverable_content(),
                    session.imported_carried_names(),
                )
            })
            .unwrap_or((SourceState::NoContext, None, None, false, false, Vec::new()));
    // A "Save Anyway" was given for these bytes, not for whatever the path
    // holds now: a source deleted or replaced since un-confirms it.
    let confirmed = unsafe { child.data::<std::path::PathBuf>(LOSS_CONFIRMED_KEY) }
        .is_some_and(|p| unsafe { p.as_ref() }.as_path() == path)
        && !(at_stake && matches!(source_state, SourceState::Missing | SourceState::Replaced) && overwrites_source);
    // An unknown extension has no report; the write refuses it with the
    // reason, as before.
    let format = (!confirmed).then(|| letters_core::save::format_for_path(path).ok()).flatten();
    // The model report is recomputed for the *target* format on every Save
    // As: bold warns for `.txt`, never for `.docx` (LET-001 step 3).
    let format_loss = format.and_then(|format| {
        let buf = crate::page_container::buffer_of(&child)?;
        let doc = crate::bridge::document_of(&buf);
        let report = letters_core::save::compatibility_report(&doc, format);
        Some((format, dropped_features(&report)?))
    });
    // What the file the tab came from holds that Letters never read and
    // can't carry (macros, embedded objects): any save leaves it out
    // (#1274). The imported revision's scan, not a re-read of the live
    // path — so a file Letters wrote never asks, and a replaced file
    // never warns about bytes the user didn't open.
    let source_loss = format.and_then(|_| {
        let report = source_report.as_ref()?;
        Some((source_name.clone()?, dropped_features(report)?))
    });
    // A missing or replaced source with anything at stake always asks,
    // even when neither report has bullets (carried-only packages): writing
    // without the carried parts would drop them silently.
    let origin_changed = at_stake && matches!(source_state, SourceState::Missing | SourceState::Replaced);
    let names_origin = origin_changed && source_loss.is_none() && !carried_names.is_empty();
    if format_loss.is_none() && source_loss.is_none() && !names_origin {
        finish(save_page_to_path(page, path), Box::new(complete));
        return;
    }
    // Save-a-Copy is offered only when this save would overwrite the
    // (missing or changed) original location; a Save As elsewhere already
    // is one.
    let offer_copy = origin_changed && overwrites_source;
    let heading = match &format_loss {
        Some((format, _)) => suite_common::i18n("Save as %s?").replace("%s", &suite_common::i18n(format.label())),
        None if names_origin && matches!(source_state, SourceState::Missing) => {
            suite_common::i18n("Original Unavailable")
        }
        None if names_origin => suite_common::i18n("File Changed on Disk"),
        None => suite_common::i18n("Save Without This Content?"),
    };
    let mut sections = Vec::new();
    if let Some((_, dropped)) = &format_loss {
        sections.push(format!("{}\n\n{}", suite_common::i18n("This format cannot hold:"), dropped));
    }
    if let Some((name, dropped)) = &source_loss {
        let preamble = if matches!(source_state, SourceState::Missing) {
            suite_common::i18n("%s is no longer available, and it held content Letters can't keep:").replace("%s", name)
        } else if matches!(source_state, SourceState::Replaced) {
            suite_common::i18n("%s changed on disk since it was opened, and it held content Letters can't keep:")
                .replace("%s", name)
        } else {
            suite_common::i18n("%s has content Letters can't keep:").replace("%s", name)
        };
        sections.push(format!("{preamble}\n\n{dropped}"));
    }
    if names_origin {
        let name = source_name.clone().unwrap_or_default();
        let parts = describe_parts(&carried_names);
        sections.push(if matches!(source_state, SourceState::Missing) {
            suite_common::i18n("%s is no longer there, so what Letters kept from it (%s) cannot be recovered from the original. Saving recreates the file without those parts.")
                .replacen("%s", &name, 1)
                .replacen("%s", &parts, 1)
        } else {
            suite_common::i18n("%s changed on disk since it was opened. Saving keeps the current text and does not carry anything over from the changed file (%s).")
                .replacen("%s", &name, 1)
                .replacen("%s", &parts, 1)
        });
    }
    if format_loss.is_some() {
        sections.push(suite_common::i18n("Saving keeps the text and loses these. Save as ODT or DOCX to keep everything."));
    }
    if let Some((name, _)) = &source_loss {
        sections.push(if overwrites_source {
            suite_common::i18n("Saving replaces the file without them. Save a copy instead to keep the original as it is.")
        } else if origin_changed {
            suite_common::i18n("The new file will not have them.")
        } else {
            suite_common::i18n("The new file will not have them; %s keeps them.").replace("%s", name)
        });
    }
    if names_origin && source_loss.is_none() {
        sections.push(if overwrites_source {
            suite_common::i18n("Save a copy to choose where the text goes, or Save Anyway to write it here without those parts.")
        } else {
            suite_common::i18n("The new file will not have those parts.")
        });
    }
    let body = sections.join("\n\n");
    let dialog = adw::AlertDialog::new(Some(&heading), Some(&body));
    dialog.add_response("cancel", &suite_common::i18n("_Cancel"));
    if offer_copy {
        dialog.add_response("copy", &suite_common::i18n("Save a _Copy"));
    }
    dialog.add_response("save", &suite_common::i18n("_Save Anyway"));
    dialog.set_response_appearance("save", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let parent = child.root().and_downcast::<gtk::Window>();
    let page = page.clone();
    let path = path.to_path_buf();
    dialog.choose(parent.as_ref(), None::<&gio::Cancellable>, move |response| {
        if response == "save" {
            unsafe { page.child().set_data(LOSS_CONFIRMED_KEY, path.clone()) };
            finish(save_page_to_path(&page, &path), Box::new(complete));
        } else if response == "copy" {
            save_with_prompt(&page, true, complete);
        } else {
            complete(SaveOutcome::Cancelled);
        }
    });
}

/// The carried parts at stake, for the dialog: up to three names, then a
/// count. Bodies stay short; the full list is in the package.
fn describe_parts(names: &[String]) -> String {
    if names.len() > 3 {
        format!("{}, {} and {} more", names[0], names[1], names.len() - 2)
    } else {
        names.join(", ")
    }
}


/// Capture the target page before awaiting a chooser. Changing the selected
/// tab must never redirect a pending save to another document.
pub(super) fn save_with_prompt(
    page: &adw::TabPage,
    force_save_as: bool,
    complete: impl FnOnce(SaveOutcome) + 'static,
) {
    if !force_save_as {
        let Some(td) = tab_data_get(&page.child()) else {
            let error = suite_common::i18n("Document session is unavailable.");
            show_message(page, "Could Not Save File", &error);
            complete(SaveOutcome::Failed(error));
            return;
        };
        let path = td.0.borrow().file.clone();
        if let Some(path) = path {
            save_asking_about_loss(page, &path, complete);
            return;
        }
    }
    let dialog = gtk::FileDialog::new();
    let filter = gtk::FileFilter::new();
    // Straight from the writer's own list, so the dialog cannot offer a
    // format that has no writer behind it (#436).
    for format in letters_core::save::SaveFormat::ALL {
        filter.add_suffix(format.extension());
    }
    filter.set_name(Some(&suite_common::i18n("Documents")));
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));
    let path = tab_data_get(&page.child()).and_then(|td| td.0.borrow().file.clone());
    let name = path
        .as_ref()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            // An unset key — or one left holding a format Letters no
            // longer writes, such as the "rtf" the Preferences window used
            // to offer — falls back to ODT rather than pre-filling a name
            // the save would then refuse.
            let settings = gio::Settings::new("org.tunaos.letters");
            let extension = letters_core::save::SaveFormat::from_extension(
                &settings.string("default-format"),
            )
            .unwrap_or(letters_core::save::SaveFormat::Odt)
            .extension();
            format!("Untitled.{extension}")
        });
    dialog.set_initial_name(Some(&name));
    let parent = page.child().root().and_downcast::<gtk::Window>();
    let page = page.clone();
    dialog.save(parent.as_ref(), None::<&gio::Cancellable>, move |result| {
        let outcome = match result {
            Ok(file) => match suite_common::locations::save_location(&file) {
                Ok(path) => {
                    save_asking_about_loss(&page, &path, complete);
                    return;
                }
                Err(error) => SaveOutcome::Failed(error),
            },
            Err(error)
                if error.matches(gtk::DialogError::Dismissed)
                    || error.matches(gio::IOErrorEnum::Cancelled) =>
            {
                SaveOutcome::Cancelled
            }
            Err(error) => SaveOutcome::Failed(error.to_string()),
        };
        if let SaveOutcome::Failed(ref error) = outcome {
            show_message(&page, "Could Not Save File", error);
        }
        complete(outcome);
    });
}

pub(super) fn do_save(tv: &adw::TabView, _stack: &gtk::Stack) {
    if let Some(page) = tv.selected_page() {
        save_with_prompt(&page, false, |_| {});
    }
}

pub(super) fn close_all_dirty_pages(
    win: adw::ApplicationWindow,
    tv: adw::TabView,
    mut queue: std::collections::VecDeque<adw::TabPage>,
    force_close: Rc<std::cell::Cell<bool>>,
) {
    let Some(page) = queue.pop_front() else {
        force_close.set(true);
        win.close();
        return;
    };
    tv.set_selected_page(&page);
    save_with_prompt(&page, false, move |outcome| {
        if outcome == SaveOutcome::Saved {
            // Yield between documents, avoiding recursive synchronous saves
            // and giving GTK a chance to process close/dialog notifications.
            gtk::glib::idle_add_local_once(move || {
                close_all_dirty_pages(win, tv, queue, force_close);
            });
        }
    });
}
