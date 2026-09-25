// SPDX-License-Identifier: GPL-3.0-or-later
//! Opening and saving documents at remote locations, with the UI they need
//! (RFC-0003): progress for a slow transfer, and a choice when the
//! document changed elsewhere. All three apps use these. The GIO work
//! itself is in [`crate::locations`].
//!
//! Nothing blocks the main loop. A transfer that takes longer than a second
//! shows a small dialog with a progress bar and a Cancel button (HIG:
//! don't flash feedback for quick operations). A save refused because the
//! document changed elsewhere asks what to do: Save as Copy (the
//! default), Overwrite or Reload. Escape cancels and leaves the document
//! unsaved.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk4::{self as gtk, gio, glib};
use libadwaita as adw;

use crate::i18n;
use crate::locations::{self, UploadError};

/// How a save of a document ended, once any remote upload and conflict
/// choice are done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    /// Saved where it was (or overwritten there, by choice).
    Saved,
    /// Saved as a copy at this new path, which the document now is.
    SavedAs(PathBuf),
    /// The user chose to discard their changes: reopen this path, which
    /// now holds the version from the location.
    Reload(PathBuf),
    /// Not saved; the document stays as it is, unsaved.
    Cancelled,
    /// Not saved; the error has already been shown.
    Failed,
}

/// What to do about a document that changed elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictChoice {
    SaveAsCopy,
    Overwrite,
    Reload,
    Cancel,
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn show_error(parent: &gtk::Widget, heading: &str, body: &str) {
    let dialog = adw::AlertDialog::builder().heading(heading).body(body).build();
    dialog.add_response("ok", &i18n("_OK"));
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("ok");
    dialog.present(Some(parent));
}

/// Ask what to do about `name`, which changed at its location since it was
/// opened. Save as Copy is the default: it loses nobody's work.
pub fn ask_changed_elsewhere(parent: &impl IsA<gtk::Widget>, name: &str, on_choice: impl FnOnce(ConflictChoice) + 'static) {
    let dialog = adw::AlertDialog::builder()
        .heading(i18n("Document Changed Elsewhere"))
        .body(format!(
            "{} {}",
            i18n("Someone else saved “%s” since you opened it.").replace("%s", name),
            i18n("Overwriting replaces their changes; reloading discards yours."),
        ))
        .build();
    dialog.add_response("reload", &i18n("_Reload"));
    dialog.add_response("copy", &i18n("Save as _Copy"));
    dialog.add_response("overwrite", &i18n("_Overwrite"));
    dialog.set_response_appearance("reload", adw::ResponseAppearance::Destructive);
    dialog.set_response_appearance("copy", adw::ResponseAppearance::Suggested);
    dialog.set_response_appearance("overwrite", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("copy"));
    dialog.set_close_response("cancel");
    let on_choice = RefCell::new(Some(on_choice));
    dialog.connect_response(None, move |_, response| {
        let choice = match response {
            "copy" => ConflictChoice::SaveAsCopy,
            "overwrite" => ConflictChoice::Overwrite,
            "reload" => ConflictChoice::Reload,
            _ => ConflictChoice::Cancel,
        };
        if let Some(f) = on_choice.borrow_mut().take() {
            f(choice);
        }
    });
    dialog.present(Some(parent.as_ref()));
}

/// A transfer's progress dialog, shown only if it runs longer than a
/// second. Cancel cancels `cancellable`.
struct Progress {
    dialog: adw::AlertDialog,
    bar: gtk::ProgressBar,
    done: Rc<Cell<bool>>,
}

impl Progress {
    fn start(parent: &gtk::Widget, heading: String, body: String, cancellable: &gio::Cancellable) -> Rc<Self> {
        let bar = gtk::ProgressBar::builder().show_text(true).build();
        let dialog = adw::AlertDialog::builder().heading(heading).body(body).extra_child(&bar).build();
        dialog.add_response("cancel", &i18n("_Cancel"));
        dialog.set_close_response("cancel");
        let c = cancellable.clone();
        dialog.connect_response(None, move |_, _| c.cancel());
        let me = Rc::new(Progress { dialog, bar, done: Rc::new(Cell::new(false)) });
        let (show, parent) = (me.clone(), parent.clone());
        glib::timeout_add_local_once(Duration::from_secs(1), move || {
            if !show.done.get() {
                show.dialog.present(Some(&parent));
                // An upload's size isn't reported as it goes: pulse.
                let pulse = show.clone();
                glib::timeout_add_local(Duration::from_millis(120), move || {
                    if pulse.done.get() {
                        return glib::ControlFlow::Break;
                    }
                    if pulse.bar.fraction() == 0.0 && pulse.bar.text().is_none() {
                        pulse.bar.pulse();
                    }
                    glib::ControlFlow::Continue
                });
            }
        });
        me
    }

    fn update(&self, done: u64, total: Option<u64>) {
        let mb = |b: u64| b as f64 / 1_000_000.0;
        match total {
            Some(total) if total > 0 => {
                self.bar.set_fraction((done as f64 / total as f64).min(1.0));
                self.bar.set_text(Some(&format!("{:.1} of {:.1} MB", mb(done), mb(total))));
            }
            _ => {
                self.bar.pulse();
                self.bar.set_text(Some(&format!("{:.1} MB", mb(done))));
            }
        }
    }

    fn finish(&self) {
        self.done.set(true);
        self.dialog.force_close();
    }
}

/// Open `file`: `on_done` gets a local path to read, at once for a local
/// file, or after downloading a remote one (with progress if it's slow).
/// A failure is shown to the user and not passed on; a cancel is silent.
pub fn open(parent: &impl IsA<gtk::Widget>, file: &gio::File, on_done: impl FnOnce(PathBuf) + 'static) {
    let parent = parent.as_ref().clone();
    if file.is_native() {
        if let Some(path) = file.path() {
            on_done(path);
            return;
        }
    }
    let file = file.clone();
    glib::spawn_future_local(async move {
        let name = file.basename().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let cancellable = gio::Cancellable::new();
        let progress = Progress::start(&parent, i18n("Opening “%s”").replace("%s", &name), file.uri().to_string(), &cancellable);
        let p = progress.clone();
        let result = gio::CancellableFuture::new(locations::download(&file, move |d, t| p.update(d, t)), cancellable).await;
        progress.finish();
        match result {
            Ok(Ok(path)) => on_done(path),
            Ok(Err(e)) => show_error(&parent, &i18n("Could Not Open Document"), &e),
            Err(_cancelled) => {}
        }
    });
}

/// After the app has written `path`: finish the save. A local path is
/// saved already. A staged remote one is uploaded, and if the document
/// changed elsewhere the user chooses what happens. `on_done` gets the
/// outcome; errors have been shown already.
pub fn finish_save(parent: &impl IsA<gtk::Widget>, path: &Path, on_done: impl FnOnce(SaveOutcome) + 'static) {
    let Some(location) = locations::remote_location(path) else {
        on_done(SaveOutcome::Saved);
        return;
    };
    let parent = parent.as_ref().clone();
    let path = path.to_path_buf();
    glib::spawn_future_local(async move {
        let outcome = save_remote(&parent, &path, &location, true).await;
        match outcome {
            Err(UploadError::ChangedElsewhere(_)) => {
                let (parent2, path2) = (parent.clone(), path.clone());
                ask_changed_elsewhere(&parent, &name_of(&path), move |choice| {
                    glib::spawn_future_local(async move {
                        on_done(resolve(&parent2, &path2, choice).await);
                    });
                });
            }
            Err(UploadError::Failed(e)) => {
                show_error(&parent, &i18n("Could Not Save Document"), &e);
                on_done(SaveOutcome::Failed);
            }
            Ok(saved) => on_done(saved),
        }
    });
}

/// Upload with a progress dialog. `Ok(Cancelled)` if the user cancelled.
async fn save_remote(parent: &gtk::Widget, path: &Path, location: &gio::File, check: bool) -> Result<SaveOutcome, UploadError> {
    let cancellable = gio::Cancellable::new();
    let progress = Progress::start(parent, i18n("Saving “%s”").replace("%s", &name_of(path)), location.uri().to_string(), &cancellable);
    let result = gio::CancellableFuture::new(locations::upload(path, check), cancellable).await;
    progress.finish();
    match result {
        Ok(Ok(())) => Ok(SaveOutcome::Saved),
        Ok(Err(e)) => Err(e),
        Err(_cancelled) => Ok(SaveOutcome::Cancelled),
    }
}

async fn resolve(parent: &gtk::Widget, path: &Path, choice: ConflictChoice) -> SaveOutcome {
    let Some(location) = locations::remote_location(path) else { return SaveOutcome::Failed };
    let failed = |e: String| {
        show_error(parent, &i18n("Could Not Save Document"), &e);
        SaveOutcome::Failed
    };
    match choice {
        ConflictChoice::Cancel => SaveOutcome::Cancelled,
        ConflictChoice::Overwrite => match save_remote(parent, path, &location, false).await {
            Ok(outcome) => outcome,
            Err(UploadError::ChangedElsewhere(e) | UploadError::Failed(e)) => failed(e),
        },
        ConflictChoice::Reload => {
            let cancellable = gio::Cancellable::new();
            let progress = Progress::start(parent, i18n("Opening “%s”").replace("%s", &name_of(path)), location.uri().to_string(), &cancellable);
            let p = progress.clone();
            let result = gio::CancellableFuture::new(locations::download(&location, move |d, t| p.update(d, t)), cancellable).await;
            progress.finish();
            match result {
                Ok(Ok(fresh)) => SaveOutcome::Reload(fresh),
                Ok(Err(e)) => failed(e),
                Err(_) => SaveOutcome::Cancelled,
            }
        }
        ConflictChoice::SaveAsCopy => {
            let Some(target) = choose_copy_location(parent, &location, path).await else {
                return SaveOutcome::Cancelled;
            };
            let staged = match locations::stage_copy(path, &target) {
                Ok(staged) => staged,
                Err(e) => return failed(e),
            };
            if locations::remote_location(&staged).is_none() {
                return SaveOutcome::SavedAs(staged);
            }
            match save_remote(parent, &staged, &target, false).await {
                Ok(SaveOutcome::Saved) => SaveOutcome::SavedAs(staged),
                Ok(other) => other,
                Err(UploadError::ChangedElsewhere(e) | UploadError::Failed(e)) => failed(e),
            }
        }
    }
}

/// A save dialog for the copy, next to the original, named "name (copy).ext".
async fn choose_copy_location(parent: &gtk::Widget, location: &gio::File, path: &Path) -> Option<gio::File> {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Document".into());
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let dialog = gtk::FileDialog::new();
    dialog.set_initial_name(Some(&format!("{stem} ({}){ext}", i18n("copy"))));
    if let Some(folder) = location.parent() {
        dialog.set_initial_folder(Some(&folder));
    }
    let window = parent.root().and_downcast::<gtk::Window>();
    dialog.save_future(window.as_ref()).await.ok()
}
