use libadwaita as adw;
use adw::prelude::AdwDialogExt;
use gtk4::{gio, prelude::*};
mod bridge;
mod window;
mod engine;
mod preferences;
mod page_container;
mod ruler;
mod spell;
mod docx_bridge;
mod styles;
mod layout;
mod print_preview;

fn main() {
    let suite = suite_common::SuiteApp::new("org.tunaos.letters");

    // Store settings so we can pass to preferences
    let settings = gio::Settings::new("org.tunaos.letters");

    // Register the "show-preferences" action
    let s = settings.clone();
    let app_weak = suite.app.downgrade();
    let act_prefs = gtk4::gio::SimpleAction::new("show-preferences", None);
    act_prefs.connect_activate(move |_, _| {
        let prefs_win = preferences::LettersPreferences::new(&s);
        if let Some(app) = app_weak.upgrade() {
            prefs_win.window.present(app.active_window().as_ref());
        } else {
            prefs_win.window.present(Option::<&gtk4::Window>::None);
        }
    });
    suite.app.add_action(&act_prefs);

    // Wire the keyboard shortcuts action
    let act_shortcuts = gtk4::gio::SimpleAction::new("show-shortcuts", None);
    act_shortcuts.connect_activate(move |_, _| {
        suite_common::show_shortcuts_dialog(
            &[
                ("General", &[
                    ("New document", "<Primary>n"),
                    ("Open file", "<Primary>o"),
                    ("Save", "<Primary>s"),
                    ("Save as", "<Primary><Shift>s"),
                    ("Print", "<Primary>p"),
                    ("Export PDF", "<Primary><Shift>e"),
                    ("Preferences", "<Primary>comma"),
                    ("Keyboard shortcuts", "<Primary>question"),
                    ("Quit", "<Primary>q"),
                ]),
                ("Formatting", &[
                    ("Bold", "<Primary>b"),
                    ("Italic", "<Primary>i"),
                    ("Underline", "<Primary>u"),
                    ("Strikethrough", "<Primary><Shift>s"),
                    ("Highlight", "<Primary><Shift>h"),
                ]),
                ("Alignment", &[
                    ("Align left", "<Primary>l"),
                    ("Align center", "<Primary>e"),
                    ("Align right", "<Primary>r"),
                    ("Justify", "<Primary>j"),
                ]),
                ("Font", &[
                    ("Increase font size", "<Primary><Shift>greater"),
                    ("Decrease font size", "<Primary><Shift>less"),
                ]),
                ("Document", &[
                    ("New tab", "<Primary>t"),
                    ("Close tab", "<Primary>w"),
                    ("Find", "<Primary>f"),
                    ("Find and replace", "<Primary>h"),
                    ("Undo", "<Primary>z"),
                    ("Redo", "<Primary>y"),
                ]),
            ],
        );
    });
    suite.app.add_action(&act_shortcuts);
    suite.app.set_accels_for_action("app.show-shortcuts", &["<Primary>question"]);

    let win_store: std::rc::Rc<std::cell::RefCell<Option<window::LettersWindow>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));

    let s = settings.clone();
    let recent = suite_common::RecentFiles::new(&s);
    let ws = win_store.clone();
    suite.app.connect_activate(move |gtk_app| {
        // Restore dark mode after GTK init
        if s.boolean("dark-mode") {
            let sm = adw::StyleManager::default();
            sm.set_color_scheme(adw::ColorScheme::ForceDark);
        }
        let mut store = ws.borrow_mut();
        if store.is_none() {
            let win = window::LettersWindow::new(gtk_app, s.clone());
            win.recover_from_snapshot();
            *store = Some(win);
        }
        store.as_ref().unwrap().present();
    });

    // CLI / file-manager launches: `letters doc.md` opens each file in a tab.
    let s = settings.clone();
    let ws = win_store.clone();
    suite.app.connect_open(move |gtk_app, files, _hint| {
        {
            let mut store = ws.borrow_mut();
            if store.is_none() {
                if s.boolean("dark-mode") {
                    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
                }
                *store = Some(window::LettersWindow::new(gtk_app, s.clone()));
            }
        }
        let store = ws.borrow();
        let win = store.as_ref().unwrap();
        for file in files {
            if let Some(path) = file.path() {
                recent.record(file);
                win.open_path(&path.to_string_lossy());
            }
        }
        win.present();
    });
    suite.run();
}

// ─── Dead-module drift guard (tuna-os/gtk-office-suite#196) ─────────────────
// letters/src/export.rs was never declared as a module, so its 14 #[cfg(test)]
// tests silently never compiled and provided zero coverage. This guard fails
// CI if any new src/*.rs file is added without a matching `mod <name>;`
// declaration in this file, so dead files can no longer accumulate silently.
#[cfg(test)]
mod dead_module_guard {
    use std::collections::HashSet;
    use std::path::Path;

    #[test]
    fn every_src_rs_file_is_declared_as_a_module() {
        // Modules known to be dead but not yet removed. export.rs duplicates
        // engine.rs's compiled Typst/PDF export (issue #196); it is
        // allowlisted pending the maintainer's wire-or-delete decision so this
        // guard still catches any NEW undeclared module.
        const KNOWN_UNDECLARED: &[&str] = &["export"];

        let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let declared: HashSet<String> = include_str!("main.rs")
            .lines()
            .filter_map(|line| line.trim().strip_prefix("mod "))
            .filter_map(|rest| rest.split_whitespace().next())
            .map(|name| name.trim_end_matches(';').to_string())
            .collect();

        let undeclared: Vec<String> = std::fs::read_dir(&src_dir)
            .expect("letters/src should be readable")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
            .filter_map(|path| {
                let stem = path.file_stem()?.to_string_lossy().into_owned();
                if stem == "main" || declared.contains(&stem) || KNOWN_UNDECLARED.contains(&stem.as_str()) {
                    None
                } else {
                    Some(stem)
                }
            })
            .collect();

        assert!(
            undeclared.is_empty(),
            "letters/src/*.rs files without a `mod <name>;` declaration in main.rs: {undeclared:?}. \
             Add the module declaration (wiring the code into the build) or delete the dead file."
        );
    }
}
