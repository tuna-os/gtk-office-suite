//! canvas_keys.rs — the editor's keyboard and clipboard, and editing a
//! text box in place: split out of window.rs (which keeps the widgets and
//! the file actions) to give it room for masters and layouts.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Pointer input on the canvas is canvas_input.rs.

use gtk4::{self as gtk, glib, prelude::*};
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::canvas::{canvas_to_slide, hit_test_object};
use crate::sidebar::rebuild_slide_list;
use crate::transition::{TransitionState, TransitionType};
use decks_core::engine::{MasterSlide, Slide, SlideObject};
use decks_core::DecksController;

/// The editor state the handlers here share with the window.
pub(crate) struct EditorHandles<'a> {
    pub window: &'a adw::ApplicationWindow,
    pub canvas: &'a gtk::DrawingArea,
    pub slide_list: &'a gtk::ListBox,
    pub slides: &'a Rc<RefCell<Vec<Slide>>>,
    pub masters: &'a Rc<RefCell<Vec<MasterSlide>>>,
    pub current_slide: &'a Rc<Cell<usize>>,
    pub selected_object: &'a Rc<Cell<Option<usize>>>,
    pub controller: &'a Rc<DecksController>,
    pub refresh_hud: &'a Rc<dyn Fn()>,
    pub transition: &'a Rc<RefCell<TransitionState>>,
}

/// Cross-app copy and paste (DESIGN-UI), in place text editing on a
/// double-click, and the editor's keys: navigation, delete, undo and redo.
pub(crate) fn register(h: EditorHandles) {
    let EditorHandles { window, canvas, slide_list, slides, masters, current_slide, selected_object, controller, refresh_hud, transition } = h;
    // ── Cross-app clipboard (DESIGN-UI): Ctrl+C copies the selected
    // text box as a styled fragment; Ctrl+V pastes a fragment as a
    // new text box. Window-level capture, skipped while an entry or
    // the notes view has focus.
    {
        let ss = slides.clone();
        let cs_ref = current_slide.clone();
        let so = selected_object.clone();
        let cs = canvas.clone();
        let controller2 = controller.clone();
        let refresh = refresh_hud.clone();
        let win = window.clone();
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        key.connect_key_pressed(move |_, keyval, _code, mods| {
            let ctrl = mods.contains(gtk::gdk::ModifierType::CONTROL_MASK);
            if !ctrl || (keyval != gtk::gdk::Key::c && keyval != gtk::gdk::Key::v) {
                return glib::Propagation::Proceed;
            }
            // Text widgets keep their own clipboard behavior.
            if gtk::prelude::GtkWindowExt::focus(&win)
                .map(|w| w.is::<gtk::Text>() || w.is::<gtk::TextView>() || w.is::<gtk::Entry>())
                .unwrap_or(false)
            {
                return glib::Propagation::Proceed;
            }
            if keyval == gtk::gdk::Key::c {
                let idx = cs_ref.get();
                let slides = ss.borrow();
                let frag = so
                    .get()
                    .and_then(|oi| slides.get(idx).and_then(|s| s.objects.get(oi)))
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
            let cs_ref2 = cs_ref.clone();
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
                        let idx = cs_ref2.get();
                        controller3.add_object(idx, obj);
                        cs2.queue_draw();
                        refresh2();
                    }
                },
            );
            glib::Propagation::Stop
        });
        window.add_controller(key);
    }

    // ── Double-click: inline text edit on TextBox ───────────────────
    {
        let ss = slides.clone();
        let cs = canvas.clone();
        let cs_ref = current_slide.clone();
        let _so = selected_object.clone();
        let controller = controller.clone();
        let dbl = gtk::GestureClick::new();
        dbl.set_button(1);
        let cs2 = cs.clone();
        dbl.connect_pressed(move |_g, n, x, y| {
            if n < 2 { return; }
            let idx = cs_ref.get();
            let slides = ss.borrow();
            if idx >= slides.len() { return; }
            let (hx, hy) =
                canvas_to_slide(x, y, cs2.width() as f64, cs2.height() as f64);
            if let Some(oi) = hit_test_object(&slides[idx].objects, hx, hy) {
                let obj = slides[idx].objects[oi].clone();
                if let SlideObject::TextBox { text, x: ox, y: oy, w: ow, h: oh, .. } = obj {
                    let old_text = text.clone();
                    drop(slides);
                    let text_view = gtk::TextView::new();
                    text_view.buffer().set_text(&old_text);
                    text_view.set_wrap_mode(gtk::WrapMode::Word);
                    text_view.set_size_request((ow / 960.0 * 800.0) as i32, (oh / 540.0 * 450.0) as i32);
                    let overlay = gtk::Fixed::new();
                    let (cvx, cvy) = crate::canvas::slide_to_canvas(ox, oy, 800.0, 450.0);
                    overlay.put(&text_view, cvx, cvy);
                    // Add overlay to window via a stack or popover — put on Fixed overlay
                    // For now, add as child of the canvas parent scrolled window area
                    if let Some(p) = cs2.parent() {
                        if let Ok(fixed) = p.downcast::<gtk::Fixed>() {
                            fixed.put(&overlay, 0.0, 0.0);
                        }
                    }
                    text_view.grab_focus();
                    // Commit on Enter via EventControllerKey
                    let key_ctrl = gtk::EventControllerKey::new();
                    let _ss2 = ss.clone();
                    let cs3 = cs.clone();
                    let controller2 = controller.clone();
                    let tv2 = text_view.clone();
                    let ov2 = overlay.clone();
                    let cs_ref2 = cs_ref.clone();
                    key_ctrl.connect_key_pressed(move |_, key, _code, _mod| {
                        if key == gtk::gdk::Key::Return || key == gtk::gdk::Key::KP_Enter {
                            let buf = tv2.buffer();
                            let new_text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
                            if new_text != old_text {
                                controller2.change_text(cs_ref2.get(), oi, old_text.clone(), new_text);
                            }
                            ov2.unparent();
                            cs3.queue_draw();
                            glib::Propagation::Stop
                        } else {
                            glib::Propagation::Proceed
                        }
                    });
                    text_view.add_controller(key_ctrl);
                    // Commit on focus loss
                    let fc = gtk::EventControllerFocus::new();
                    let tv3 = text_view.clone();
                    let _ov3 = overlay.clone();
                    fc.connect_leave(move |_| {
                        if let Some(p) = tv3.parent() { p.unparent(); }
                    });
                    text_view.add_controller(fc);
                }
            }
        });
        canvas.add_controller(dbl);
    }

    // ── Keyboard: navigation, delete, undo/redo ─────────────────────
    {
        let w = window.clone();
        let sl = slide_list.clone();
        let ss = slides.clone();
        let cs = canvas.clone();
        let cs_ref = current_slide.clone();
        let so = selected_object.clone();
        let controller = controller.clone();
        let ts = transition.clone();
        let m = masters.clone();
        let key = gtk::EventControllerKey::new();
        key.connect_key_pressed(move |_, keyval, _code, mods| {
            // Ctrl+Z: undo
            if mods.contains(gtk::gdk::ModifierType::CONTROL_MASK) && keyval == gtk::gdk::Key::z {
                if controller.undo() {
                    cs.queue_draw();
                    // Snapshot instead of holding ss.borrow() across the
                    // call -- rebuild_slide_list()'s select_row() fires
                    // row-selected synchronously, which reaches ss.borrow_mut().
                    let slides = ss.borrow().clone();
                    rebuild_slide_list(&sl, &slides, &m.borrow(), cs_ref.get());
                }
                return glib::Propagation::Stop;
            }
            // Ctrl+Shift+Z: redo
            if mods.contains(gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SHIFT_MASK) && keyval == gtk::gdk::Key::z {
                if controller.redo() {
                    cs.queue_draw();
                    let slides = ss.borrow().clone();
                    rebuild_slide_list(&sl, &slides, &m.borrow(), cs_ref.get());
                }
                return glib::Propagation::Stop;
            }
            match keyval {
                gtk::gdk::Key::Escape => {
                    w.unfullscreen();
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Left | gtk::gdk::Key::Up => {
                    let idx = cs_ref.get();
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
                        cs_ref.set(idx - 1);
                        rebuild_slide_list(&sl, &snap, &m.borrow(), idx - 1);
                        cs.queue_draw();
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Right | gtk::gdk::Key::Down | gtk::gdk::Key::space => {
                    let idx = cs_ref.get();
                    let slides = ss.borrow();
                    if idx + 1 < slides.len() {
                        TransitionState::start(&ts, TransitionType::of(slides[idx + 1].transition),
                            &slides[idx], &slides[idx + 1], &m.borrow(), &cs);
                        let snap = slides.clone();
                        drop(slides);
                        cs_ref.set(idx + 1);
                        rebuild_slide_list(&sl, &snap, &m.borrow(), idx + 1);
                        cs.queue_draw();
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Home => {
                    cs_ref.set(0);
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
                        cs_ref.set(last);
                        rebuild_slide_list(&sl, &snap, &m.borrow(), last);
                        cs.queue_draw();
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Delete | gtk::gdk::Key::BackSpace => {
                    let idx = cs_ref.get();
                    let slides = ss.borrow();
                    if idx < slides.len() {
                        if let Some(oi) = so.get() {
                            if oi < slides[idx].objects.len() {
                                let obj = slides[idx].objects[oi].clone();
                                drop(slides);
                                controller.delete_object(idx, oi, obj);
                                so.set(None);
                                cs.queue_draw();
                            }
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
