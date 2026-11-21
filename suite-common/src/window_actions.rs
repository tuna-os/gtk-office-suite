//! Document actions that follow the window the user is in (#1422).
//!
//! Tables and Decks register their document actions (`save-file`, `undo`,
//! `insert-row`, …) on the application, and every accelerator, menu item
//! and `gapplication action` call names them as `app.<name>`. That was
//! sound while each process held one window. With a window per document, a
//! second window's constructor re-registers every name, so the application
//! would act on whichever window was built last.
//!
//! Rather than move some sixty actions and their callers to `win.` names,
//! each window keeps the action objects its constructor registered, and
//! they are put back on the application whenever that window becomes the
//! application's active window. `app.save-file` then means "save the
//! document in front of me", from a shortcut, a menu or the test harness.
//!
//! Actions that existed before the constructor ran and were left alone
//! (About, Quit, Preferences) are not a window's and are never touched.

use gtk4::{self as gtk, gio, glib, prelude::*};
use std::cell::RefCell;

thread_local! {
    static WINDOWS: RefCell<Vec<(glib::WeakRef<gtk::Window>, Vec<gio::Action>)>> = const { RefCell::new(Vec::new()) };
    static WATCHED: RefCell<Vec<glib::WeakRef<gtk::Application>>> = const { RefCell::new(Vec::new()) };
}

/// The application's actions as they were before a window's constructor.
pub struct Registration {
    before: Vec<gio::Action>,
}

/// Call before building a window that registers document actions.
pub fn begin(app: &impl IsA<gtk::Application>) -> Registration {
    let app = app.as_ref();
    let before = app.list_actions().iter().filter_map(|name| app.lookup_action(name)).collect();
    Registration { before }
}

impl Registration {
    /// Call once `window`'s constructor has registered its actions: the ones
    /// it added or replaced are now `window`'s, reinstalled whenever it is
    /// the active window.
    pub fn finish(self, app: &impl IsA<gtk::Application>, window: &impl IsA<gtk::Window>) {
        let app = app.as_ref();
        let owned: Vec<gio::Action> = app
            .list_actions()
            .iter()
            .filter_map(|name| app.lookup_action(name))
            .filter(|action| !self.before.contains(action))
            .collect();
        WINDOWS.with(|windows| {
            let mut windows = windows.borrow_mut();
            windows.retain(|(w, _)| w.upgrade().is_some());
            windows.push((window.as_ref().downgrade(), owned));
        });
        watch(app);
    }
}

/// Put `window`'s document actions back on the application.
pub fn install_for(app: &impl IsA<gtk::Application>, window: &impl IsA<gtk::Window>) {
    let app = app.as_ref();
    let window = window.as_ref();
    let actions = WINDOWS.with(|windows| {
        windows
            .borrow()
            .iter()
            .find(|(w, _)| w.upgrade().as_ref() == Some(window))
            .map(|(_, actions)| actions.clone())
    });
    for action in actions.unwrap_or_default() {
        app.add_action(&action);
    }
}

fn watch(app: &gtk::Application) {
    let first = WATCHED.with(|watched| {
        let mut watched = watched.borrow_mut();
        watched.retain(|a| a.upgrade().is_some());
        if watched.iter().any(|a| a.upgrade().as_ref() == Some(app)) {
            return false;
        }
        watched.push(app.downgrade());
        true
    });
    if first {
        app.connect_active_window_notify(|app| {
            if let Some(window) = app.active_window() {
                install_for(app, &window);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> gio::SimpleAction {
        gio::SimpleAction::new(name, None)
    }

    #[test]
    fn each_window_gets_its_own_actions_back_and_shared_ones_are_left_alone() {
        crate::gtk_test::run(|| {
            let app = gtk::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
            // Listing an application's actions needs it registered, as the
            // apps are by the time they build a window.
            app.register(None::<&gio::Cancellable>).expect("register");
            let quit = named("quit");
            app.add_action(&quit);

            let first = gtk::Window::new();
            let registration = begin(&app);
            let (save_1, undo_1) = (named("save-file"), named("undo"));
            app.add_action(&save_1);
            app.add_action(&undo_1);
            registration.finish(&app, &first);

            let second = gtk::Window::new();
            let registration = begin(&app);
            let (save_2, undo_2) = (named("save-file"), named("undo"));
            app.add_action(&save_2);
            app.add_action(&undo_2);
            registration.finish(&app, &second);

            let is_current = |action: &gio::SimpleAction| {
                app.lookup_action(&action.name()).as_ref() == Some(action.upcast_ref::<gio::Action>())
            };
            assert!(is_current(&save_2), "the newest window's, until another is active");

            install_for(&app, &first);
            assert!(is_current(&save_1) && is_current(&undo_1));
            install_for(&app, &second);
            assert!(is_current(&save_2) && is_current(&undo_2));
            assert!(is_current(&quit), "an action no window registered stays as it was");

            first.destroy();
            second.destroy();
        });
    }
}
