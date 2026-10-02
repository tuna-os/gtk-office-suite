//! slide_actions.rs — the slide list's commands as app actions.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Duplicate Slide, Move Slide Up and Move Slide Down
//! (`app.duplicate-slide`, `app.move-slide-up`, `app.move-slide-down`):
//! the sidebar's buttons bind to them, so the shortcuts, the command
//! palette and automation reach the same commands. Each is one undo step
//! on the controller (`DecksController::duplicate_slide`,
//! `move_slide_up`, `move_slide_down`), and each selects the slide it
//! moved or made.

use gtk4::{gio, prelude::*};
use libadwaita as adw;

use crate::canvas_keys::EditorHandles;
use crate::sidebar::rebuild_slide_list;

/// The actions, their labels and their shortcuts.
const ACTIONS: [(&str, &str, &str); 3] = [
    ("duplicate-slide", "Duplicate Slide", "<Primary><Shift>d"),
    ("move-slide-up", "Move Slide Up", "<Primary><Shift>Page_Up"),
    ("move-slide-down", "Move Slide Down", "<Primary><Shift>Page_Down"),
];

pub(crate) fn register(app: &adw::Application, h: EditorHandles) {
    let labels: Vec<(String, &str)> = ACTIONS.iter().map(|(n, l, _)| (format!("app.{n}"), *l)).collect();
    suite_common::actions::register_labels(&labels.iter().map(|(n, l)| (n.as_str(), *l)).collect::<Vec<_>>());
    for (name, _, accel) in ACTIONS {
        let (ctl, cs, so, canvas, list, slides, masters, refresh) = (
            h.controller.clone(),
            h.current_slide.clone(),
            h.selected_object.clone(),
            h.canvas.clone(),
            h.slide_list.clone(),
            h.slides.clone(),
            h.masters.clone(),
            h.refresh_hud.clone(),
        );
        let act = gio::SimpleAction::new(name, None);
        act.connect_activate(move |_, _| {
            let at = cs.get();
            let moved = match name {
                "duplicate-slide" => ctl.duplicate_slide(at),
                "move-slide-up" => ctl.move_slide_up(at),
                _ => ctl.move_slide_down(at),
            };
            let Some(to) = moved else { return };
            cs.set(to);
            so.set(None);
            rebuild_slide_list(&list, &slides.borrow().clone(), &masters.borrow(), to);
            canvas.queue_draw();
            refresh();
        });
        app.add_action(&act);
        app.set_accels_for_action(&format!("app.{name}"), &[accel]);
    }
}
