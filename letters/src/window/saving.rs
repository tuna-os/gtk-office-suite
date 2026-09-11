// SPDX-License-Identifier: GPL-3.0-or-later
// GTK save adapter. All entry points use one session transaction and writer.

use super::{get_textview, tab_data_get};
use adw::prelude::*;
use gtk4::{self as gtk, gio, prelude::*};
use libadwaita as adw;
use std::path::Path;
use std::rc::Rc;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SaveOutcome {
    Saved,
    NeedsPath,
    Cancelled,
    Failed(String),
}

fn show_message(page: &adw::TabPage, title: &str, message: &str) {
    let dialog = adw::AlertDialog::new(Some(&suite_common::i18n(title)), Some(message));
    dialog.add_response("ok", &suite_common::i18n("_OK"));
    dialog.set_close_response("ok");
    dialog.present(page.child().root().and_downcast::<gtk::Window>().as_ref());
}

fn save_page_to_path(page: &adw::TabPage, path: &Path) -> SaveOutcome {
    let child = page.child();
    let Some(td) = tab_data_get(&child) else {
        return SaveOutcome::Failed(suite_common::i18n("Document session is unavailable."));
    };
    let Some(buf) = get_textview(&child).map(|view| view.buffer()) else {
        return SaveOutcome::Failed(suite_common::i18n("Document editor is unavailable."));
    };
    // The report comes back out of the session transaction, which only
    // passes through `Result<(), String>`: the write has to stay inside
    // `save_to` so a failure cannot advance the savepoint.
    let mut report = None;
    let result = td.0.borrow_mut().save_to(path.to_path_buf(), |path| {
        report = Some(crate::bridge::save_buffer_to_file(&buf, path)?);
        Ok(())
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
    page.set_tooltip(&path.to_string_lossy());
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
    if let Some(report) = report {
        if let Some(message) = loss_message(&report) {
            show_message(page, "Saved, with formatting this format cannot hold", &message);
        }
    }
    SaveOutcome::Saved
}

/// What the chosen format dropped, or `None` when it held everything.
///
/// The report is built from the document's actual contents (see
/// `letters_core::save`), so this stays quiet for an unstyled document
/// saved as plain text rather than warning on every `.txt` save.
fn loss_message(report: &suite_common::interop::CompatibilityReport) -> Option<String> {
    let dropped: Vec<String> = report
        .destructive_features()
        .iter()
        .map(|feature| format!("\u{2022} {}", suite_common::i18n(&feature.label)))
        .collect();
    if dropped.is_empty() {
        return None;
    }
    Some(format!(
        "{}\n\n{}",
        suite_common::i18n("The file was written, but this format cannot hold:"),
        dropped.join("\n")
    ))
}

fn save_page(page: &adw::TabPage) -> SaveOutcome {
    let Some(td) = tab_data_get(&page.child()) else {
        return SaveOutcome::Failed(suite_common::i18n("Document session is unavailable."));
    };
    let path = td.0.borrow().file.clone();
    match path {
        Some(path) => save_page_to_path(page, &path),
        None => SaveOutcome::NeedsPath,
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
        let outcome = save_page(page);
        if outcome != SaveOutcome::NeedsPath {
            if let SaveOutcome::Failed(ref error) = outcome {
                show_message(page, "Could not save document", error);
            }
            complete(outcome);
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
            Ok(file) => match file.path() {
                Some(path) => save_page_to_path(&page, &path),
                None => SaveOutcome::Failed(suite_common::i18n(
                    "The selected location is not a local file.",
                )),
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
            show_message(&page, "Could not save document", error);
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
