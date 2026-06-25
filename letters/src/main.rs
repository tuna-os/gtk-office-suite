use libadwaita as adw;
use adw::prelude::AdwDialogExt;
use gtk4::{gio, prelude::*};
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
    let suite = suite_common::SuiteApp::new("org.tunaos.letters-rust");

    // Store settings so we can pass to preferences
    let settings = gio::Settings::new("org.tunaos.letters-rust");

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

    // Register export-pdf action
    suite.app.add_action(&{
        let a = gtk4::gio::SimpleAction::new("export-pdf", None);
        a.connect_activate(|_, _| {});
        a
    });
    suite.app.set_accels_for_action("app.export-pdf", &["<Primary><Shift>e"]);

    let s = settings.clone();
    suite.app.connect_activate(move |gtk_app| {
        // Restore dark mode after GTK init
        if s.boolean("dark-mode") {
            let sm = adw::StyleManager::default();
            sm.set_color_scheme(adw::ColorScheme::ForceDark);
        }
        let w = window::LettersWindow::new(gtk_app, s.clone());
        w.present();
    });
    suite.run();
}
