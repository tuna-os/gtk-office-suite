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
    let Some(path_str) = path.to_str() else {
        return SaveOutcome::Failed(suite_common::i18n(
            "This filename cannot be represented as UTF-8.",
        ));
    };
    let result = td.0.borrow_mut().save_to(path.to_path_buf(), |_| {
        crate::bridge::save_buffer_to_file(&buf, path_str)
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
    page.set_tooltip(path_str);
    suite_common::push_recent_file(&gio::Settings::new("org.tunaos.letters"), path_str);
    if let Some(warning) = commit.recovery_warning {
        show_message(page, "Document saved; recovery cleanup failed", &warning);
    }
    SaveOutcome::Saved
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
    for suffix in ["md", "txt", "docx", "odt"] {
        filter.add_suffix(suffix);
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
            let settings = gio::Settings::new("org.tunaos.letters");
            let extension = settings.string("default-format");
            format!(
                "Untitled.{}",
                if extension.is_empty() {
                    "odt"
                } else {
                    &extension
                }
            )
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
