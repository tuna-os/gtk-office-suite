use gtk4::prelude::*;
use gtk4::gio;
use std::cell::RefCell;
use std::rc::Rc;
mod charts;
mod chart_dialog;
mod window;
mod format_inspector;
mod number_format_editor;
mod formula_bar;
mod column_menu;
mod notes;
mod validation_list;
mod sheet_bar;
mod persistence;
mod window_dialogs;
mod sheet_actions;
mod loss_ui;
mod preferences;
pub mod grid_render;
pub mod grid_area;

fn main() {
    let suite = suite_common::SuiteApp::new("org.tunaos.tables");

    // Headless `--export-pdf <out>` (docs/TABLES-EXPORT-PARITY.md): a real
    // GApplication option, so `--help` documents it. Export implies the
    // test-only actions, the same ones the render lab uses; connect_open
    // schedules the export once the window settles.
    suite.app.add_main_option(
        suite_common::render_dump::EXPORT_PDF_FLAG,
        gtk4::glib::Char::from(0u8),
        gtk4::glib::OptionFlags::NONE,
        gtk4::glib::OptionArg::Filename,
        "Export the opened spreadsheet to PDF with no dialogs and quit",
        Some("OUT"),
    );
    suite.app.connect_handle_local_options(|_, options| {
        match suite_common::render_dump::export_pdf_path(options) {
            Ok(Some(out)) => {
                std::env::set_var("GTK_OFFICE_TEST_MODE", "1");
                std::env::set_var(suite_common::render_dump::EXPORT_PDF_ENV, &out);
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("tables: {e}");
                return std::ops::ControlFlow::Break(gtk4::glib::ExitCode::FAILURE);
            }
        }
        std::ops::ControlFlow::Continue(())
    });
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
    let app_for_prefs = suite.app.clone();
    act_prefs.connect_activate(move |_, _| {
        let settings = gio::Settings::new("org.tunaos.tables");
        let prefs_win = preferences::TablesPreferences::new(&settings);
        libadwaita::prelude::AdwDialogExt::present(&prefs_win.window, app_for_prefs.active_window().as_ref());
    });
    suite.app.add_action(&act_prefs);
    suite.app.set_accels_for_action("app.preferences", &["<Control>comma"]);

    // One window per workbook (#1422): a crash with two workbooks open
    // brings both back, and a file opened from the file manager no longer
    // replaces the one already open.
    let windows: Windows = Rc::new(RefCell::new(Vec::new()));
    let ws = windows.clone();
    suite.app.connect_activate(move |app| {
        if let Some(active) = app.active_window() {
            active.present();
        } else {
            // Every orphan comes back in a window of its own. The bound
            // stops a snapshot that recovers but cannot be cleared from
            // being offered again and again.
            let orphans = suite_common::autosave::find_orphaned_snapshots(&persistence::autosave_state_dir()).len();
            let mut recovered = 0;
            for _ in 0..orphans {
                let win = new_window(app, &ws);
                if !win.recover_from_snapshot() {
                    win.window.destroy();
                    break;
                }
                win.present();
                recovered += 1;
            }
            if recovered == 0 {
                new_window(app, &ws).present();
            }
        }
        if std::env::var_os(suite_common::render_dump::EXPORT_PDF_ENV).is_some() {
            eprintln!("tables: --export-pdf needs an input file to export");
            app.quit();
        }
    });

    // CLI / file-manager launches: `tables budget.xlsx` opens the file, in
    // the window in front if that one is an untouched new workbook and in a
    // new window otherwise.
    let ws = windows.clone();
    suite.app.connect_open(move |app, files, _hint| {
        let mut reuse = app.active_window().and_then(|active| {
            ws.borrow().iter().find(|w| w.window.upcast_ref::<gtk4::Window>() == &active && w.is_pristine()).cloned()
        });
        for file in files {
            let win = reuse.take().unwrap_or_else(|| new_window(app, &ws));
            let Some(path) = persistence::local_path(file, false, Some(&win.window)) else {
                continue;
            };
            let path_str = path.to_string_lossy().to_string();
            if let Err(e) = win.open_path(&path_str) {
                // stderr is not a user interface: launched from a file
                // manager or a Flatpak, an unreadable file used to open an
                // empty window with no explanation at all (#447).
                let name = path.file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| path_str.clone());
                suite_common::show_error_dialog(
                    Some(&win.window),
                    &suite_common::i18n("Could not open file"),
                    &format!("{name}\n\n{e}"),
                );
            }
            win.present();
        }
        suite_common::render_dump::schedule(app);
        if std::env::var_os(suite_common::render_dump::EXPORT_PDF_ENV).is_some() {
            suite_common::render_dump::schedule_export(app);
        }
    });
    suite.run();
}

type Windows = Rc<RefCell<Vec<Rc<window::TablesWindow>>>>;

/// A new workbook window. The application's document actions (`app.save-file`
/// and the rest) act on whichever window is active, and the window is
/// forgotten once it closes.
fn new_window(app: &libadwaita::Application, windows: &Windows) -> Rc<window::TablesWindow> {
    let registration = suite_common::window_actions::begin(app);
    let win = Rc::new(window::TablesWindow::new(app));
    registration.finish(app, &win.window);
    let weak = Rc::downgrade(windows);
    let closing = win.window.clone();
    win.window.connect_destroy(move |_| {
        if let Some(windows) = weak.upgrade() {
            windows.borrow_mut().retain(|w| w.window != closing);
        }
    });
    windows.borrow_mut().push(win.clone());
    win
}
