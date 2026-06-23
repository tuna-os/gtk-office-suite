use libadwaita as adw;
use adw::prelude::AdwDialogExt;
use gtk4::{gio, prelude::*};
mod window;
mod engine;
mod preferences;

fn main() {
    let suite = suite_common::SuiteApp::new("org.tunaos.letters-rust");

    // Register the "show-preferences" action
    let act_prefs = gtk4::gio::SimpleAction::new("show-preferences", None);
    act_prefs.connect_activate(|_, _| {
        let prefs_win = preferences::LettersPreferences::new();
        prefs_win.window.present(Option::<&gtk4::Window>::None);
    });
    suite.app.add_action(&act_prefs);

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
    suite.app.add_action(&act_shortcuts);
    suite.app.set_accels_for_action("app.show-shortcuts", &["<Primary>question"]);

    // Register export-pdf / print actions
    suite.app.add_action(&{
        let a = gtk4::gio::SimpleAction::new("export-pdf", None);
        a.connect_activate(|_, _| {});
        a
    });
    suite.app.set_accels_for_action("app.export-pdf", &["<Primary><Shift>e"]);
    suite.app.add_action(&{
        let a = gtk4::gio::SimpleAction::new("print", None);
        a.connect_activate(|_, _| {});
        a
    });

    suite.app.connect_activate(move |gtk_app| {
        // Restore dark mode after GTK init
        let settings = gio::Settings::new("org.tunaos.letters-rust");
        if settings.boolean("dark-mode") {
            let sm = adw::StyleManager::default();
            sm.set_color_scheme(adw::ColorScheme::ForceDark);
        }
        let w = window::LettersWindow::new(gtk_app);
        w.present();
    });
    suite.run();
}
