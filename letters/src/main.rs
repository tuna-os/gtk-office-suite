use libadwaita as adw;
use adw::prelude::AdwDialogExt;
use gtk4::prelude::*;
mod window;
mod engine;
mod export;
mod preferences;

fn main() {
    let suite = suite_common::SuiteApp::new("org.tunaos.letters-rust");

    // Restore dark mode from GSettings
    suite.restore_dark_mode();

    let app = &suite.app;

    // Register the "show-preferences" action
    let act_prefs = gtk4::gio::SimpleAction::new("show-preferences", None);
    act_prefs.connect_activate(|_, _| {
        let prefs_win = preferences::LettersPreferences::new();
        // AdwPreferencesDialog.present() takes a parent GtkWindow
        // AdwPreferencesDialog is an AdwDialog; use AdwDialogExt::present(parent)
        prefs_win.window.present(Option::<&gtk4::Window>::None);
    });
    app.add_action(&act_prefs);

    // Wire the keyboard shortcuts action
    let act_shortcuts = gtk4::gio::SimpleAction::new("show-shortcuts", None);
    act_shortcuts.connect_activate(move |_, _| {
        suite_common::show_shortcuts_dialog(
            &[
                ("General", &[
                    ("New Document", "<Primary>n"),
                    ("Open File", "<Primary>o"),
                    ("Save", "<Primary>s"),
                    ("Save As", "<Primary><Shift>s"),
                    ("Print", "<Primary>p"),
                    ("Export PDF", "<Primary><Shift>e"),
                    ("Preferences", "<Primary>comma"),
                    ("Keyboard Shortcuts", "<Primary>question"),
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
                    ("Align Left", "<Primary>l"),
                    ("Align Center", "<Primary>e"),
                    ("Align Right", "<Primary>r"),
                    ("Justify", "<Primary>j"),
                ]),
                ("Font", &[
                    ("Increase Font Size", "<Primary><Shift>greater"),
                    ("Decrease Font Size", "<Primary><Shift>less"),
                ]),
                ("Document", &[
                    ("New Tab", "<Primary>t"),
                    ("Close Tab", "<Primary>w"),
                    ("Find", "<Primary>f"),
                    ("Find and Replace", "<Primary>h"),
                    ("Undo", "<Primary>z"),
                    ("Redo", "<Primary>y"),
                ]),
            ],
        );
    });
    app.add_action(&act_shortcuts);

    // Wire the existing app.shortcuts action to show-shortcuts
    app.set_accels_for_action("app.show-shortcuts", &["<Primary>question"]);

    // Register save/save-as/print/export actions
    for name in &["save-file", "save-file-as", "export-pdf", "print"] {
        let act = gtk4::gio::SimpleAction::new(name, None);
        act.connect_activate(|_, _| {});
        app.add_action(&act);
    }

    suite.app.connect_activate(|app| {
        let w = window::LettersWindow::new(app);
        w.present();
    });
    suite.run();
}
