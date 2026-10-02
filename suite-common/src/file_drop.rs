// SPDX-License-Identifier: GPL-3.0-or-later
//! Opening dropped files through the application's guarded open (#1316).

use gtk4::{self as gtk, gio, glib, prelude::*};

/// Open the files dropped on `widget` the way a file manager's open does:
/// through `GApplication::open`, the path each app guards (#1316). A
/// window that holds one document asks before a drop replaces unsaved
/// work, and Letters opens a new tab, because the drop and the file
/// manager now take the same path. Remote locations are staged by that
/// handler too (RFC-0003).
///
/// `claim` sees each dropped file first and returns true for one the
/// window handles itself (a picture dropped on a slide); the rest are
/// opened.
///
/// In test mode, the app action `test-drop-files` (a newline-separated
/// list of paths) drops those files, through the same code a real drop
/// runs: a synthetic drag is out of reach of the GUI tests.
pub fn open_files_on_drop(
    widget: &impl IsA<gtk::Widget>,
    app: &impl IsA<gio::Application>,
    claim: impl Fn(&gio::File) -> bool + 'static,
) -> gtk::DropTarget {
    let claim = std::rc::Rc::new(claim);
    let app_weak = app.as_ref().downgrade();
    let target = gtk::DropTarget::new(gtk4::gdk::FileList::static_type(), gtk4::gdk::DragAction::COPY);
    {
        let claim = claim.clone();
        let app_weak = app_weak.clone();
        target.connect_drop(move |_, val, _, _| {
            let (Ok(list), Some(app)) = (val.get::<gtk4::gdk::FileList>(), app_weak.upgrade()) else { return false };
            let files = list.files();
            if files.is_empty() {
                return false;
            }
            open_dropped(&app, &files, &*claim);
            true
        });
    }
    widget.add_controller(target.clone());
    if std::env::var_os("GTK_OFFICE_TEST_MODE").is_some() {
        let act = gio::SimpleAction::new("test-drop-files", Some(glib::VariantTy::STRING));
        act.connect_activate(move |_, param| {
            let (Some(paths), Some(app)) = (param.and_then(|p| p.get::<String>()), app_weak.upgrade()) else { return };
            let files: Vec<gio::File> = paths.lines().filter(|l| !l.is_empty()).map(gio::File::for_path).collect();
            open_dropped(&app, &files, &*claim);
        });
        if let Some(map) = app.dynamic_cast_ref::<gio::ActionMap>() {
            map.add_action(&act);
        }
    }
    target
}

fn open_dropped(app: &gio::Application, files: &[gio::File], claim: &dyn Fn(&gio::File) -> bool) {
    let rest: Vec<gio::File> = files.iter().filter(|f| !claim(f)).cloned().collect();
    if !rest.is_empty() {
        app.open(&rest, "");
    }
}
