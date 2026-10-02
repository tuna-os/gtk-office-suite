//! file_pick.rs — the open dialogs, with a test-mode way past them.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! GTK's file dialog can't be driven reliably over AT-SPI and xdotool on
//! the test display: its location entry races autocompletion and its
//! window takes focus on its own schedule. Under GTK_OFFICE_TEST_MODE, with
//! GTK_OFFICE_TEST_PICK naming a file that holds a path, `open` hands that
//! path over as the chosen file instead, so a journey proves everything
//! after the dialog (Add Image, Open). The dialog itself is GTK's. Like the
//! test snapshot (#104), nothing changes outside test mode.

use gtk4::{self as gtk, gio, glib, prelude::*};

/// The path a test chose, read afresh each time so one journey can pick
/// several files in turn.
fn test_pick() -> Option<std::path::PathBuf> {
    std::env::var_os("GTK_OFFICE_TEST_MODE")?;
    let file = std::env::var_os("GTK_OFFICE_TEST_PICK")?;
    let path = std::fs::read_to_string(file).ok()?;
    let path = path.trim();
    (!path.is_empty()).then(|| path.into())
}

/// `dialog.open` on `parent`, calling `done` with the choice.
pub(crate) fn open(dialog: &gtk::FileDialog, parent: &impl IsA<gtk::Window>, done: impl FnOnce(Result<gio::File, glib::Error>) + 'static) {
    if let Some(path) = test_pick() {
        done(Ok(gio::File::for_path(path)));
        return;
    }
    dialog.open(Some(parent), None::<&gio::Cancellable>, done);
}
