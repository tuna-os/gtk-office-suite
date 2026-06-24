use gtk4::prelude::*;
use gtk4::gio;
mod window;
mod engine;
mod export;
mod undo;
mod canvas;
mod sidebar;
mod toolbar;
mod transition;
mod markdown;
mod preferences;

fn main() {
    let suite = suite_common::SuiteApp::new("org.tunaos.decks-rust");
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
    ];
    let act_shortcuts = gtk4::gio::SimpleAction::new("show-shortcuts", None);
    let s = shortcuts;
    act_shortcuts.connect_activate(move |_, _| {
        suite_common::show_shortcuts_dialog(s);
    });
    suite.app.add_action(&act_shortcuts);
    suite.app.set_accels_for_action("app.show-shortcuts", &["<Primary>question"]);
    let act_prefs = gtk4::gio::SimpleAction::new("preferences", None);
    act_prefs.connect_activate(|_, _| {
        let settings = gio::Settings::new("org.tunaos.decks-rust");
        let _p = preferences::DecksPreferences::new(&settings);
    });
    suite.app.add_action(&act_prefs);
    suite.app.set_accels_for_action("app.preferences", &["<Control>comma"]);
    suite.app.connect_activate(|app| {
        let w = window::DecksWindow::new(app);
        w.present();
    });
    suite.run();
}
