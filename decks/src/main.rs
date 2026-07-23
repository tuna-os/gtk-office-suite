use gtk4::prelude::*;
use gtk4::gio;
mod window;
mod export;
mod canvas;
mod canvas_area;
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
    let parent_win = std::rc::Rc::new(std::cell::RefCell::new(None::<gtk4::Window>));
    let pw = parent_win.clone();
    act_prefs.connect_activate(move |_, _| {
        let settings = gio::Settings::new("org.tunaos.decks-rust");
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
        for file in files {
            if let Some(path) = file.path() {
                if let Err(e) = win.open_path(&path.to_string_lossy()) {
                    eprintln!("open failed: {e}");
                }
            }
        }
        win.present();
    });
    suite.run();
}
