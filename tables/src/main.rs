use gtk4::prelude::*;
use gtk4::gio;
mod charts;
mod chart_dialog;
mod window;
mod persistence;
mod window_dialogs;
mod preferences;
pub mod grid_render;
pub mod grid_area;

fn main() {
    let suite = suite_common::SuiteApp::new("org.tunaos.tables");
    let shortcuts: &[(&str, &[(&str, &str)])] = &[
        ("Editing", &[
            ("Undo", "<Control>z"),
            ("Redo", "<Control><Shift>z"),
            ("Clear cell", "Delete"),
            ("Commit cell edit", "Enter"),
        ]),
        ("File", &[
            ("Save", "<Control>s"),
            ("Open", "<Control>o"),
            ("New spreadsheet", "<Control>n"),
        ]),
    ];
    let act_shortcuts = gtk4::gio::SimpleAction::new("show-shortcuts", None);
    let s = shortcuts;
    act_shortcuts.connect_activate(move |_, _| {
        suite_common::show_shortcuts_dialog(s);
    });
    suite.app.add_action(&act_shortcuts);
    suite.app.set_accels_for_action("app.show-shortcuts", &["<Primary>question"]);
    let act_prefs = gtk4::gio::SimpleAction::new("preferences", None);
    let parent_win = std::rc::Rc::new(std::cell::RefCell::new(None::<gtk4::Window>));
    let pw = parent_win.clone();
    act_prefs.connect_activate(move |_, _| {
        let settings = gio::Settings::new("org.tunaos.tables");
        let prefs_win = preferences::TablesPreferences::new(&settings);
        libadwaita::prelude::AdwDialogExt::present(&prefs_win.window, pw.borrow().as_ref());
    });
    suite.app.add_action(&act_prefs);
    suite.app.set_accels_for_action("app.preferences", &["<Control>comma"]);
    let win_store = std::rc::Rc::new(std::cell::RefCell::new(None::<window::TablesWindow>));
    let pw_store = parent_win.clone();
    let ws = win_store.clone();
    suite.app.connect_activate(move |app| {
        let mut store = ws.borrow_mut();
        if store.is_none() {
            let win = window::TablesWindow::new(app);
            win.recover_from_snapshot();
            *pw_store.borrow_mut() = Some(win.window.clone().upcast::<gtk4::Window>());
            *store = Some(win);
        }
        store.as_ref().unwrap().present();
    });

    // CLI / file-manager launches: `tables budget.xlsx` opens the file.
    let pw_store = parent_win.clone();
    let ws = win_store.clone();
    suite.app.connect_open(move |app, files, _hint| {
        {
            let mut store = ws.borrow_mut();
            if store.is_none() {
                let win = window::TablesWindow::new(app);
                *pw_store.borrow_mut() = Some(win.window.clone().upcast::<gtk4::Window>());
                *store = Some(win);
            }
        }
        let store = ws.borrow();
        let win = store.as_ref().unwrap();
        for file in files {
            if let Some(path) = file.path() {
                let path_str = path.to_string_lossy().to_string();
                if let Err(e) = win.open_path(&path_str) {
                    // stderr is not a user interface: launched from a file
                    // manager or a Flatpak, an unreadable file used to open
                    // an empty window with no explanation at all (#447).
                    let name = std::path::Path::new(&path_str)
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| path_str.clone());
                    suite_common::show_error_dialog(
                        Some(&win.window),
                        &suite_common::i18n("Could not open file"),
                        &format!("{name}

{e}"),
                    );
                }
            }
        }
        win.present();
    });
    suite.run();
}
