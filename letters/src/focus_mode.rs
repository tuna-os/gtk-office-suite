// SPDX-License-Identifier: GPL-3.0-or-later
//
// focus_mode.rs — distraction-free typing (DESIGN-UI, Letters from Pages:
// "The chrome fades while typing and comes back on pointer motion").
//
// When it is on, typing text slides the window's bars away (header bar,
// toolbar and status bar: AdwToolbarView's reveal properties) and moving
// the pointer brings them back. Shortcuts (Ctrl, Alt, Super) never hide
// them, so a command is always reachable. Off by default; the stateful
// action `app.distraction-free` (Ctrl+Alt+D, the command palette) turns it
// on and the choice is remembered.

use gtk4::{self as gtk, gdk, gio, glib, prelude::*};
use libadwaita as adw;
use std::cell::Cell;
use std::rc::Rc;

/// Pointer travel, in pixels, that counts as "moving the pointer" (a
/// resting hand's jitter, or a bar sliding under it, does not).
const WAKE_PX: f64 = 8.0;

/// Whether `key` with `state` types into the document: text, Enter,
/// Backspace, Delete or Tab, with no command modifier held.
pub fn is_typing(key: gdk::Key, state: gdk::ModifierType) -> bool {
    let command = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK | gdk::ModifierType::SUPER_MASK;
    if state.intersects(command) {
        return false;
    }
    let editing = [gdk::Key::Return, gdk::Key::KP_Enter, gdk::Key::BackSpace, gdk::Key::Delete, gdk::Key::Tab];
    editing.contains(&key) || key.to_unicode().is_some_and(|c| !c.is_control())
}

/// Wire distraction-free typing into `window`, whose bars `bars` holds.
pub fn attach(app: &adw::Application, window: &adw::ApplicationWindow, bars: &adw::ToolbarView, settings: &gio::Settings) {
    let on = Rc::new(Cell::new(settings.boolean("distraction-free")));
    let reveal = {
        let bars = bars.clone();
        move |shown: bool| {
            bars.set_reveal_top_bars(shown);
            bars.set_reveal_bottom_bars(shown);
        }
    };

    let action = gio::SimpleAction::new_stateful("distraction-free", None, &on.get().to_variant());
    {
        let (on, s, reveal) = (on.clone(), settings.clone(), reveal.clone());
        action.connect_change_state(move |a, state| {
            let Some(value) = state.and_then(|v| v.get::<bool>()) else { return };
            a.set_state(&value.to_variant());
            on.set(value);
            let _ = s.set_boolean("distraction-free", value);
            if !value {
                reveal(true);
            }
        });
    }
    app.add_action(&action);
    app.set_accels_for_action("app.distraction-free", &["<Primary><Alt>d"]);
    suite_common::actions::register_labels(&[("app.distraction-free", &suite_common::i18n("Distraction-Free Typing"))]);

    // Where the pointer was when the bars went away.
    let anchor: Rc<Cell<Option<(f64, f64)>>> = Rc::default();
    let pointer: Rc<Cell<(f64, f64)>> = Rc::default();

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let (on, anchor, pointer, reveal) = (on.clone(), anchor.clone(), pointer.clone(), reveal.clone());
        keys.connect_key_pressed(move |_, key, _, state| {
            if on.get() && anchor.get().is_none() && is_typing(key, state) {
                anchor.set(Some(pointer.get()));
                reveal(false);
            }
            glib::Propagation::Proceed
        });
    }
    window.add_controller(keys);

    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(move |_, x, y| {
        pointer.set((x, y));
        if let Some((ax, ay)) = anchor.get() {
            if (x - ax).hypot(y - ay) > WAKE_PX {
                anchor.set(None);
                reveal(true);
            }
        }
    });
    window.add_controller(motion);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_hides_the_chrome_and_shortcuts_do_not() {
        let none = gdk::ModifierType::empty();
        assert!(is_typing(gdk::Key::a, none));
        assert!(is_typing(gdk::Key::A, gdk::ModifierType::SHIFT_MASK));
        assert!(is_typing(gdk::Key::space, none));
        assert!(is_typing(gdk::Key::Return, none));
        assert!(is_typing(gdk::Key::BackSpace, none));
        assert!(!is_typing(gdk::Key::s, gdk::ModifierType::CONTROL_MASK), "Ctrl+S is a command");
        assert!(!is_typing(gdk::Key::d, gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK));
        assert!(!is_typing(gdk::Key::Escape, none));
        assert!(!is_typing(gdk::Key::Up, none), "moving the caret is not typing");
        assert!(!is_typing(gdk::Key::F10, none));
    }
}
