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
    // a thumbnail, ODF settings) are read from the file the tab came from
    // before the write replaces it, then put back (#1274).
    let source = td.0.borrow().file.clone();
    let result = td.0.borrow_mut().save_to(path.to_path_buf(), |path| {
        let carried = source.as_deref().map(suite_common::carry::capture).unwrap_or_default();
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
    let finish = {
        let page = page.clone();
        move |outcome: SaveOutcome, complete: Box<dyn FnOnce(SaveOutcome)>| {
            if let SaveOutcome::Failed(ref error) = outcome {
                show_message(&page, "Could Not Save File", error);
            }
            complete(outcome);
        }
    };
    let child = page.child();
    let confirmed = unsafe { child.data::<std::path::PathBuf>(LOSS_CONFIRMED_KEY) }
        .is_some_and(|p| unsafe { p.as_ref() }.as_path() == path);
    // An unknown extension has no report; the write refuses it with the
    // reason, as before.
    let format = (!confirmed).then(|| letters_core::save::format_for_path(path).ok()).flatten();
    let format_loss = format.and_then(|format| {
        let buf = crate::page_container::buffer_of(&child)?;
        let doc = crate::bridge::document_of(&buf);
        let report = letters_core::save::compatibility_report(&doc, format);
        Some((format, dropped_features(&report)?))
    });
    // What the file the tab came from holds that Letters never read and
    // can't carry (macros, embedded objects): any save leaves it out
    // (#1274). Read from disk now, so a file Letters wrote never asks.
    let source = tab_data_get(&child).and_then(|td| td.0.borrow().file.clone());
    let source_loss = format
        .and(source.as_ref())
        .and_then(|source| Some((source.clone(), dropped_features(&letters_core::loss::content_a_save_drops(source))?)));
    if format_loss.is_none() && source_loss.is_none() {
        finish(save_page_to_path(page, path), Box::new(complete));
        return;
    }
    let heading = match &format_loss {
        Some((format, _)) => suite_common::i18n("Save as %s?").replace("%s", &suite_common::i18n(format.label())),
        None => suite_common::i18n("Save Without This Content?"),
    };
    let mut sections = Vec::new();
    if let Some((_, dropped)) = &format_loss {
        sections.push(format!("{}\n\n{}", suite_common::i18n("This format cannot hold:"), dropped));
    }
    if let Some((source, dropped)) = &source_loss {
        let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        sections.push(format!("{}\n\n{}", suite_common::i18n("%s has content Letters can't keep:").replace("%s", &name), dropped));
    }
    if format_loss.is_some() {
        sections.push(suite_common::i18n("Saving keeps the text and loses these. Save as ODT or DOCX to keep everything."));
    }
    if let Some((source, _)) = &source_loss {
        sections.push(if source.as_path() == path {
            suite_common::i18n("Saving replaces the file without them. Save a copy instead to keep the original as it is.")
        } else {
            let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            suite_common::i18n("The new file will not have them; %s keeps them.").replace("%s", &name)
        });
    }
    let body = sections.join("\n\n");
    let dialog = adw::AlertDialog::new(Some(&heading), Some(&body));
    dialog.add_response("cancel", &suite_common::i18n("_Cancel"));
    dialog.add_response("save", &suite_common::i18n("_Save Anyway"));
    dialog.set_response_appearance("save", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let parent = child.root().and_downcast::<gtk::Window>();
    let page = page.clone();
    let path = path.to_path_buf();
    dialog.choose(parent.as_ref(), None::<&gio::Cancellable>, move |response| {
        if response != "save" {
            complete(SaveOutcome::Cancelled);
            return;
        }
        unsafe { page.child().set_data(LOSS_CONFIRMED_KEY, path.clone()) };
        finish(save_page_to_path(&page, &path), Box::new(complete));
    });
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
