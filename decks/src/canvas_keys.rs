//! canvas_keys.rs — the editor's keyboard and clipboard: split out of
//! window.rs (which keeps the widgets and the file actions) to give it
//! room for masters and layouts. Editing a text box in place is
//! text_editor.rs.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Pointer input on the canvas is canvas_input.rs.

use gtk4::{self as gtk, glib, prelude::*};
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;

use crate::sidebar::rebuild_slide_list;
use crate::transition::{TransitionState, TransitionType};
use decks_core::engine::{MasterSlide, Slide};
use decks_core::DecksController;

/// The editor state the handlers here share with the window.
#[derive(Clone, Copy)]
pub(crate) struct EditorHandles<'a> {
    pub window: &'a adw::ApplicationWindow,
    pub canvas: &'a gtk::DrawingArea,
    pub slide_list: &'a gtk::ListBox,
    pub slides: &'a Rc<RefCell<Vec<Slide>>>,
    pub masters: &'a Rc<RefCell<Vec<MasterSlide>>>,
    pub controller: &'a Rc<DecksController>,
    pub refresh_hud: &'a Rc<dyn Fn()>,
    pub transition: &'a Rc<RefCell<TransitionState>>,
}

/// Cross-app copy and paste (DESIGN-UI), in place text editing on a
/// double-click, and the editor's keys: navigation, delete, undo and redo.
pub(crate) fn register(h: EditorHandles, editor_host: &gtk::Overlay) {
    let EditorHandles { window, canvas, slide_list, slides, masters, controller, refresh_hud, transition } = h;
    // In-place rich text editing (text_editor.rs): while it is open the
    // controller holds a text session, and the canvas's keys stand aside.
    crate::text_editor::register(h, editor_host);
    // ── Cross-app clipboard (DESIGN-UI): Ctrl+C copies the selected
    // text box as a styled fragment; Ctrl+V pastes a fragment as a
    // new text box. Window-level capture, skipped while an entry or
    // the notes view has focus.
    {
        let ss = slides.clone();
        let cs = canvas.clone();
        let controller2 = controller.clone();
        let refresh = refresh_hud.clone();
        let win = window.clone();
        let text_editing = controller.clone();
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        key.connect_key_pressed(move |_, keyval, _code, mods| {
            let ctrl = mods.contains(gtk::gdk::ModifierType::CONTROL_MASK);
            if !ctrl || (keyval != gtk::gdk::Key::c && keyval != gtk::gdk::Key::v) {
                return glib::Propagation::Proceed;
            }
            // Text widgets keep their own clipboard behavior.
            if text_editing.has_text_session() || gtk::prelude::GtkWindowExt::focus(&win)
                .map(|w| w.is::<gtk::Text>() || w.is::<gtk::TextView>() || w.is::<gtk::Entry>())
                .unwrap_or(false)
            {
                return glib::Propagation::Proceed;
            }
            if keyval == gtk::gdk::Key::c {
                let slides = ss.borrow();
                let frag = controller2
                    .primary_object_index()
                    .zip(controller2.selected_slide_index())
                    .and_then(|(oi, idx)| slides.get(idx).and_then(|s| s.objects.get(oi)))
                    .and_then(decks_core::fragment::copy_object);
                if let Some(frag) = frag {
                    let provider = suite_common::clipboard::provider(
                        decks_core::fragment::MIME,
                        &frag.to_json(),
                        &frag.to_html(),
                        &frag.to_plain(),
                    );
                    let _ = cs.clipboard().set_content(Some(&provider));
                    return glib::Propagation::Stop;
                }
                return glib::Propagation::Proceed;
            }
            // Paste
            let clipboard = cs.clipboard();
            if !suite_common::clipboard::offers(&clipboard, decks_core::fragment::MIME) {
                return glib::Propagation::Proceed;
            }
            let cs2 = cs.clone();
            let controller3 = controller2.clone();
            let refresh2 = refresh.clone();
            suite_common::clipboard::read_string(
                &clipboard,
                decks_core::fragment::MIME,
                move |json| {
                    if let Some(frag) = json
                        .as_deref()
                        .and_then(decks_core::fragment::Fragment::from_json)
                    {
                        let obj = decks_core::fragment::paste_as_text_box(&frag, 240.0, 200.0);
                        if let Some(slide_id) = controller3.selection().slide_id {
                            controller3.add_object(slide_id, obj);
                        }
                        cs2.queue_draw();
                        refresh2();
                    }
                },
            );
            glib::Propagation::Stop
        });
        window.add_controller(key);
    }


    // ── Keyboard: navigation, delete, undo/redo ─────────────────────
    {
        let w = window.clone();
        let sl = slide_list.clone();
        let ss = slides.clone();
        let cs = canvas.clone();
        let controller = controller.clone();
        let ts = transition.clone();
        let m = masters.clone();
        let refresh = refresh_hud.clone();
        let key = gtk::EventControllerKey::new();
        let text_editing = controller.clone();
        key.connect_key_pressed(move |_, keyval, _code, mods| {
            // Canvas commands stay suspended throughout the text session,
            // including while focus moves to the editor's buttons.
            if text_editing.has_text_session() {
                return glib::Propagation::Proceed;
            }
            // Ctrl+Z: undo
            if mods.contains(gtk::gdk::ModifierType::CONTROL_MASK) && keyval == gtk::gdk::Key::z {
                if controller.undo() {
                    controller.repair_selection();
                    cs.queue_draw();
                    // Snapshot instead of holding ss.borrow() across the
                    // call -- rebuild_slide_list()'s select_row() fires
                    // row-selected synchronously, which reaches ss.borrow_mut().
                    let slides = ss.borrow().clone();
                    rebuild_slide_list(&sl, &slides, &m.borrow(), controller.selected_slide_index().unwrap_or(0));
                }
                return glib::Propagation::Stop;
            }
            // Ctrl+Shift+Z: redo
            if mods.contains(gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SHIFT_MASK) && keyval == gtk::gdk::Key::z {
                if controller.redo() {
                    controller.repair_selection();
                    cs.queue_draw();
                    let slides = ss.borrow().clone();
                    rebuild_slide_list(&sl, &slides, &m.borrow(), controller.selected_slide_index().unwrap_or(0));
                }
                return glib::Propagation::Stop;
            }
            match keyval {
                gtk::gdk::Key::Escape => {
                    w.unfullscreen();
                    glib::Propagation::Stop
                }
                // Tab and Shift+Tab select the slide's objects in turn, so
                // the keyboard reaches what the pointer does: the inspector
                // follows the selection (PowerPoint and Keynote do the same).
                gtk::gdk::Key::Tab | gtk::gdk::Key::ISO_Left_Tab => {
                    let sel = controller.selection();
                    let Some(slide_id) = sel.slide_id else {
                        return glib::Propagation::Proceed;
                    };
                    let Some(idx) = controller.slide_index(slide_id) else {
                        return glib::Propagation::Proceed;
                    };
                    let count = ss.borrow().get(idx).map_or(0, |s| s.objects.len());
                    if count == 0 {
                        return glib::Propagation::Proceed;
                    }
                    let back = keyval == gtk::gdk::Key::ISO_Left_Tab || mods.contains(gtk::gdk::ModifierType::SHIFT_MASK);
                    let next = next_object(controller.primary_object_index(), count, back);
                    if let Some(object_id) = controller.object_id_at(idx, next) {
                        controller.select_objects(slide_id, &[object_id], Some(object_id), decks_core::controller::SelectionMode::Object);
                    }
                    refresh();
                    cs.queue_draw();
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Left | gtk::gdk::Key::Up => {
                    if let Some(idx) = controller.selected_slide_index() {
                        if idx > 0 {
                            let sls = ss.borrow();
                            if idx < sls.len() && idx > 0 {
                                // Backwards, the slide being left plays
                                // its own transition in reverse.
                                TransitionState::start(&ts, TransitionType::of(sls[idx].transition),
                                    &sls[idx], &sls[idx - 1], &m.borrow(), &cs);
                            }
                            // Snapshot + drop before rebuild_slide_list(),
                            // whose select_row() reaches ss.borrow_mut().
                            let snap = sls.clone();
                            drop(sls);
                            if let Some(slide_id) = controller.slide_id_at(idx - 1) {
                                controller.select_slide(slide_id);
                            }
                            rebuild_slide_list(&sl, &snap, &m.borrow(), idx - 1);
                            cs.queue_draw();
                        }
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Right | gtk::gdk::Key::Down | gtk::gdk::Key::space => {
                    if let Some(idx) = controller.selected_slide_index() {
                        let slides = ss.borrow();
                        if idx + 1 < slides.len() {
                            TransitionState::start(&ts, TransitionType::of(slides[idx + 1].transition),
                                &slides[idx], &slides[idx + 1], &m.borrow(), &cs);
                            let snap = slides.clone();
                            drop(slides);
                            if let Some(slide_id) = controller.slide_id_at(idx + 1) {
                                controller.select_slide(slide_id);
                            }
                            rebuild_slide_list(&sl, &snap, &m.borrow(), idx + 1);
                            cs.queue_draw();
                        }
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Home => {
                    if let Some(slide_id) = controller.slide_id_at(0) {
                        controller.select_slide(slide_id);
                    }
                    let snap = ss.borrow().clone();
                    rebuild_slide_list(&sl, &snap, &m.borrow(), 0);
                    cs.queue_draw();
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::End => {
                    let slides = ss.borrow();
                    if !slides.is_empty() {
                        let snap = slides.clone();
                        let last = snap.len() - 1;
                        drop(slides);
                        if let Some(slide_id) = controller.slide_id_at(last) {
                            controller.select_slide(slide_id);
                        }
                        rebuild_slide_list(&sl, &snap, &m.borrow(), last);
                        cs.queue_draw();
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Delete | gtk::gdk::Key::BackSpace => {
                    let sel = controller.selection();
                    if let (Some(slide_id), Some(object_id)) = (sel.slide_id, sel.primary_object_id) {
                        if controller.delete_object(slide_id, object_id) {
                            cs.queue_draw();
                        }
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        canvas.add_controller(key);
    }
}

/// The object Tab (or, `back`, Shift+Tab) selects next among `count`,
/// from `current`: in order, wrapping round, starting at the first (or the
/// last) when nothing is selected.
fn next_object(current: Option<usize>, count: usize, back: bool) -> usize {
    match (current, back) {
        (None, false) => 0,
        (None, true) => count - 1,
        (Some(i), false) => (i + 1) % count,
        (Some(i), true) => (i + count - 1) % count,
    }
}

#[cfg(test)]
mod tests {
    use super::next_object;

    #[test]
    fn tab_walks_the_objects_round_in_both_directions() {
        assert_eq!(next_object(None, 3, false), 0);
        assert_eq!(next_object(Some(2), 3, false), 0, "Tab wraps to the first");
        assert_eq!(next_object(None, 3, true), 2);
        assert_eq!(next_object(Some(0), 3, true), 2, "Shift+Tab wraps to the last");
        assert_eq!(next_object(Some(1), 3, true), 0);
    }
}
