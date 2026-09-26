//! master_view.rs — Edit Master: the master of the current slide, edited
//! in the slides' place.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Keynote's Edit Master (Google Slides' Edit theme): the controller puts
//! the master where the slides were, as one slide whose objects are the
//! master's decorations (`DecksController::edit_master`), so the canvas,
//! the Insert buttons, the inspector and undo all work on it unchanged. A
//! banner says what is being edited and holds Done, which puts the slides
//! back and records the master edit as one undo step. Starting another
//! deck waits for Done: New, New from Template and Open are off meanwhile.

use adw::prelude::*;
use gtk4::{self as gtk, gio};
use libadwaita as adw;

use crate::canvas_keys::EditorHandles;
use crate::sidebar::rebuild_slide_list;

/// The actions that would replace the deck under the master view.
const REPLACES_DECK: [&str; 3] = ["new-document", "new-from-template", "open-file"];

fn set_deck_actions(app: &adw::Application, enabled: bool) {
    for name in REPLACES_DECK {
        if let Some(a) = app.lookup_action(name).and_then(|a| a.downcast::<gio::SimpleAction>().ok()) {
            a.set_enabled(enabled);
        }
    }
}

/// Add Edit Master and Done (`app.edit-master`, `app.finish-master`) and
/// the banner above the editor, into `add_bar`.
pub(crate) fn register(app: &adw::Application, add_bar: impl Fn(&adw::Banner), h: EditorHandles) {
    let banner = adw::Banner::builder().button_label("_Done").action_name("app.finish-master").build();
    add_bar(&banner);

    let redraw = {
        let (canvas, list, slides, masters, refresh) =
            (h.canvas.clone(), h.slide_list.clone(), h.slides.clone(), h.masters.clone(), h.refresh_hud.clone());
        move |at: usize| {
            let snapshot = slides.borrow().clone();
            rebuild_slide_list(&list, &snapshot, &masters.borrow(), at);
            canvas.queue_draw();
            refresh();
        }
    };

    {
        let (ctl, cs, so, masters, banner, app2) =
            (h.controller.clone(), h.current_slide.clone(), h.selected_object.clone(), h.masters.clone(), banner.clone(), app.clone());
        let redraw = redraw.clone();
        let act = gio::SimpleAction::new("edit-master", None);
        act.connect_activate(move |_, _| {
            let Some(index) = ctl.edit_master(cs.get()) else { return };
            let name = masters.borrow().get(index).map(|m| m.name.clone()).unwrap_or_default();
            banner.set_title(&format!("Editing the master \u{201c}{name}\u{201d}: every slide that uses it changes"));
            banner.set_revealed(true);
            set_deck_actions(&app2, false);
            cs.set(0);
            so.set(None);
            redraw(0);
        });
        app.add_action(&act);
    }
    // Put the current slide on layout n of its master (the inspector's
    // Layout row does the same; this is the keyboard's and automation's way).
    {
        let (ctl, cs, redraw) = (h.controller.clone(), h.current_slide.clone(), redraw.clone());
        let act = gio::SimpleAction::new("apply-layout", Some(gtk4::glib::VariantTy::UINT32));
        act.connect_activate(move |_, p| {
            let Some(n) = p.and_then(|p| p.get::<u32>()) else { return };
            if ctl.apply_layout(cs.get(), n as usize) {
                redraw(cs.get());
            }
        });
        app.add_action(&act);
    }
    {
        let (ctl, cs, so, banner, app2) =
            (h.controller.clone(), h.current_slide.clone(), h.selected_object.clone(), banner.clone(), app.clone());
        let act = gio::SimpleAction::new("finish-master", None);
        act.connect_activate(move |_, _| {
            let Some(from) = ctl.finish_master() else { return };
            banner.set_revealed(false);
            set_deck_actions(&app2, true);
            cs.set(from);
            so.set(None);
            redraw(from);
        });
        app.add_action(&act);
    }
}

/// The inspector's row that opens the master view.
pub(crate) fn inspector_row() -> adw::ActionRow {
    let button = gtk::Button::with_label("Edit Master");
    button.set_valign(gtk::Align::Center);
    button.set_action_name(Some("app.edit-master"));
    let row = adw::ActionRow::builder()
        .title("Master")
        .subtitle("The background and decorations every slide on it shares")
        .build();
    row.add_suffix(&button);
    row
}
