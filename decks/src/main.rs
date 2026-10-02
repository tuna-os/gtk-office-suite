use gtk4::prelude::*;
use gtk4::gio;
mod window;
mod persistence;
mod export;
mod export_ui;
mod loss_ui;
mod canvas;
mod text_render;
mod format_inspector;
mod chart_inspector;
mod insert_bar;
mod layout_picker;
mod master_view;
mod template_chooser;
mod notes_pane;
mod presenter_window;
mod canvas_area;
mod canvas_input;
mod canvas_keys;
mod sidebar;
mod slide_actions;
mod toolbar;
mod transition;
mod markdown;
mod preferences;

fn main() {
    let suite = suite_common::SuiteApp::new("org.tunaos.decks");

    // Headless `--export-pdf <out>` (docs/EXPORT-PARITY-SPEC.md item 1): a
    // real GApplication option, so `--help` documents it. Export implies the
    // test-only actions, the same ones the render lab uses; connect_open
    // schedules the export once the window settles.
    suite.app.add_main_option(
        suite_common::render_dump::EXPORT_PDF_FLAG,
        gtk4::glib::Char::from(0u8),
        gtk4::glib::OptionFlags::NONE,
        gtk4::glib::OptionArg::Filename,
        "Export the opened presentation to PDF with no dialogs and quit",
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
                eprintln!("decks: {e}");
                return std::ops::ControlFlow::Break(gtk4::glib::ExitCode::FAILURE);
            }
        }
        std::ops::ControlFlow::Continue(())
    });
    let shortcuts: &[(&str, &[(&str, &str)])] = &[
        ("Editing", &[
            ("Undo", "<Control>z"),
            ("Redo", "<Control><Shift>z"),
            ("Delete selected object", "Delete"),
        ]),
        ("Navigation", &[
            ("Exit fullscreen", "Escape"),
            ("Previous slide", "Left / Up"),
            ("Next slide", "Right / Down / Space"),
            ("First slide", "Home"),
            ("Last slide", "End"),
        ]),
        ("File", &[
            ("Save", "<Control>s"),
            ("Open", "<Control>o"),
            ("New presentation", "<Control>n"),
        ]),
        ("Speaker Notes", &[
            ("Go to speaker notes", "<Control><Alt><Shift>s"),
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
        let settings = gio::Settings::new("org.tunaos.decks");
        let prefs_win = preferences::DecksPreferences::new(&settings);
        libadwaita::prelude::AdwDialogExt::present(&prefs_win.window, pw.borrow().as_ref());
    });
    suite.app.add_action(&act_prefs);
    suite.app.set_accels_for_action("app.preferences", &["<Control>comma"]);
    // After window creation, store it for preferences
    let win_store = std::rc::Rc::new(std::cell::RefCell::new(None::<window::DecksWindow>));
    let pw_store = parent_win.clone();
    let ws = win_store.clone();
    suite.app.connect_activate(move |app| {
        let mut store = ws.borrow_mut();
        if store.is_none() {
            let w = window::DecksWindow::new(app);
            w.recover_from_snapshot();
            *pw_store.borrow_mut() = Some(w.window.clone().upcast::<gtk4::Window>());
            *store = Some(w);
        }
        store.as_ref().unwrap().present();
        if std::env::var_os(suite_common::render_dump::EXPORT_PDF_ENV).is_some() {
            eprintln!("decks: --export-pdf needs an input file to export");
            app.quit();
        }
    });

    // CLI / file-manager launches: `decks talk.pptx` opens the file.
    let pw_store = parent_win.clone();
    let ws = win_store.clone();
    suite.app.connect_open(move |app, files, _hint| {
        {
            let mut store = ws.borrow_mut();
            if store.is_none() {
                let w = window::DecksWindow::new(app);
                *pw_store.borrow_mut() = Some(w.window.clone().upcast::<gtk4::Window>());
                *store = Some(w);
            }
        }
        let store = ws.borrow();
        let win = store.as_ref().unwrap();
        // A remote location is staged to a local copy (RFC-0003).
        let paths: Vec<String> = files
            .iter()
            .filter_map(|file| match suite_common::locations::open_location(file) {
                Ok(path) => Some(path.to_string_lossy().to_string()),
                Err(e) => {
                    suite_common::show_error_dialog(Some(&win.window), &suite_common::i18n("Could not open file"), &e);
                    None
                }
            })
            .collect();
        // A file handed over by the file manager replaces the window's
        // deck, so unsaved changes are asked about first.
        let opener = ws.clone();
        suite_common::confirm_discarding(&win.window, win.is_dirty(), "presentation", move || {
            let store = opener.borrow();
            let Some(win) = store.as_ref() else { return };
            for path_str in paths {
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
        });
        win.present();
        suite_common::render_dump::schedule(app);
        if std::env::var_os(suite_common::render_dump::EXPORT_PDF_ENV).is_some() {
            suite_common::render_dump::schedule_export(app);
        }
    });
    suite.run();
}
