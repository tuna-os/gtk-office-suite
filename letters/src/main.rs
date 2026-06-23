use libadwaita as adw;
use adw::prelude::AdwDialogExt;
use gtk4::prelude::*;
mod window;
mod engine;
mod export;
mod preferences;

fn main() {
    let suite = suite_common::SuiteApp::new("org.tunaos.letters-rust");
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
