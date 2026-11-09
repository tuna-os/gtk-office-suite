//! The text entry's signals: separators, Return, Backspace, the arrow keys, the
//! pointer sweep over pills and dropped pills.

use super::parse::{is_whole_address, on_insert, Insert};
use super::selection::{acting_on, is_in_pill, is_selected, set_selected};
use super::RecipientRow;
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gdk, glib};
use std::cell::RefCell;

impl RecipientRow {
    /// Connect the text and the row to their signals and controllers, in
    /// the order the handlers have always run in.
    pub(super) fn wire(&self) {
        self.wire_typing();
        self.wire_selection_keys();
        self.wire_sweep();
        self.wire_pill_drop();
    }

    /// Separators, Return and Backspace in the text.
    fn wire_typing(&self) {
        let text = &self.imp().text;

        // Keys are not enough: an input method or a paste inserts text
        // without them, so separators are caught as the text goes in.
        let weak = self.downgrade();
        text.connect_insert_text(move |text, new, pos| {
            let Some(row) = weak.upgrade() else { return };
            if row.imp().busy.get() {
                return;
            }
            let current = text.text().to_string();
            let at = current
                .char_indices()
                .nth((*pos).max(0) as usize)
                .map_or(current.len(), |(i, _)| i);
            let (before, after) = current.split_at(at);
            if let Some(action) = on_insert(before, new, after) {
                text.stop_signal_emission_by_name("insert-text");
                match action {
                    Insert::Swallow => {}
                    Insert::Commit { done, rest } => {
                        row.push(&done);
                        row.set_typed(&rest);
                        row.emit_changed();
                    }
                }
            }
        });

        let weak = self.downgrade();
        text.connect_changed(move |_| {
            let Some(row) = weak.upgrade() else { return };
            if !row.imp().busy.get() {
                row.emit_changed();
            }
        });

        // Return makes a pill of whatever was typed. The composer catches
        // Return first while its suggestion list is open, to pick one.
        let weak = self.downgrade();
        text.connect_activate(move |_| {
            let Some(row) = weak.upgrade() else { return };
            let typed = row.typed();
            if !typed.is_empty() {
                row.complete(&typed);
            }
        });

        // Backspace on an empty line: the last pill becomes text again.
        let weak = self.downgrade();
        text.connect_backspace(move |text| {
            let Some(row) = weak.upgrade() else { return };
            if !text.text().is_empty() {
                return;
            }
            let Some((pill, recipient)) = row.imp().pills.borrow_mut().pop() else {
                return;
            };
            text.stop_signal_emission_by_name("backspace");
            row.imp().flow.remove(&pill);
            row.set_typed(&recipient);
            row.emit_changed();
        });
    }

    /// Pills selected from the keyboard, and let go of on typing.
    fn wire_selection_keys(&self) {
        let text = &self.imp().text;
        // Left from the start of the text selects the last pill, to go
        // through them with the keyboard, and Ctrl+A on an empty line
        // selects them all.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.downgrade();
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(row) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let text = &row.imp().text;
            let Some(last) = row.pill_list().pop() else {
                return glib::Propagation::Proceed;
            };
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            if ctrl && matches!(key, gdk::Key::a | gdk::Key::A) && text.text().is_empty() {
                row.select_all();
                last.grab_focus();
                return glib::Propagation::Stop;
            }
            let towards = if row.direction() == gtk::TextDirection::Rtl {
                gdk::Key::Right
            } else {
                gdk::Key::Left
            };
            let other =
                state.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK);
            if key != towards || other || text.position() != 0 || text.selection_bounds().is_some()
            {
                return glib::Propagation::Proceed;
            }
            row.select_only(&last);
            last.grab_focus();
            glib::Propagation::Stop
        });
        text.add_controller(keys);

        // Going back to typing lets go of the selection.
        let focus = gtk::EventControllerFocus::new();
        let weak = self.downgrade();
        focus.connect_enter(move |_| {
            if let Some(row) = weak.upgrade() {
                row.clear_selection();
            }
        });
        text.add_controller(focus);
    }

    /// Pills selected by sweeping the pointer over them.
    fn wire_sweep(&self) {
        // Swept with the pointer from anywhere but a pill, the pills are
        // selected the way dragging over text selects it, on into the other
        // fields; Ctrl keeps those already selected. A press on the text
        // where there is some is left to it, to select text with; one
        // anywhere else is taken from the text at once, which would claim
        // it, and is a click into the text when it does not move. In the
        // capture phase, to see the press before the text does.
        let sweep = gtk::GestureDrag::new();
        sweep.set_propagation_phase(gtk::PropagationPhase::Capture);
        // From where among all the pills it started, the selection Ctrl
        // kept, and whether it has moved enough to be a sweep.
        type Sweep = (usize, Vec<gtk::Widget>, bool);
        let swept: std::rc::Rc<RefCell<Option<Sweep>>> = Default::default();
        let weak = self.downgrade();
        let s = swept.clone();
        sweep.connect_drag_begin(move |gesture, x, y| {
            let Some(row) = weak.upgrade() else { return };
            let text = &row.imp().text;
            let on = row.pick(x, y, gtk::PickFlags::DEFAULT);
            let on_text = on
                .as_ref()
                .is_some_and(|w| w.is_ancestor(text) || w == text.upcast_ref::<gtk::Widget>())
                && !text.text().is_empty();
            if on_text
                || on.is_none_or(|w| {
                    is_in_pill(&w) || w.ancestor(gtk::Button::static_type()).is_some()
                })
            {
                gesture.set_state(gtk::EventSequenceState::Denied);
                return;
            }
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let kept = if gesture
                .current_event_state()
                .contains(gdk::ModifierType::CONTROL_MASK)
            {
                row.all_pills().into_iter().filter(is_selected).collect()
            } else {
                Vec::new()
            };
            s.replace(Some((row.sweep_index(x, y), kept, false)));
        });
        let weak = self.downgrade();
        let s = swept.clone();
        sweep.connect_drag_update(move |gesture, dx, dy| {
            let (Some(row), Some((x, y))) = (weak.upgrade(), gesture.start_point()) else {
                return;
            };
            let mut swept = s.borrow_mut();
            let Some((from, kept, moved)) = swept.as_mut() else {
                return;
            };
            *moved = *moved || dx.hypot(dy) >= row.settings().gtk_dnd_drag_threshold() as f64;
            if !*moved {
                return;
            }
            let to = row.sweep_index(x + dx, y + dy);
            let range = (*from).min(to)..(*from).max(to);
            for (i, p) in row.all_pills().iter().enumerate() {
                set_selected(p, range.contains(&i) || kept.contains(p));
            }
        });
        let weak = self.downgrade();
        sweep.connect_drag_end(move |gesture, dx, dy| {
            let Some((from, kept, moved)) = swept.take() else {
                return;
            };
            let (Some(row), Some((x, y))) = (weak.upgrade(), gesture.start_point()) else {
                return;
            };
            let to = row.sweep_index(x + dx, y + dy);
            if !moved || (from == to && kept.is_empty()) {
                row.imp().text.grab_focus();
                return;
            }
            // Shift+click goes on from where the sweep started, and the
            // focus is on the pill it ended at.
            let pills = row.all_pills();
            let (start, end) = if to < from {
                (from - 1, to)
            } else {
                (from, to.max(1) - 1)
            };
            let end = if from == to {
                kept.last()
            } else {
                pills.get(end)
            };
            if let Some(start) = pills.get(start).filter(|_| from != to) {
                if let Some(field) = start
                    .ancestor(RecipientRow::static_type())
                    .and_downcast::<RecipientRow>()
                {
                    field.imp().anchor.replace(Some(start.clone()));
                }
            }
            if let Some(end) = end {
                end.grab_focus();
            }
        });
        self.add_controller(sweep);
    }

    /// Pills dropped on the field.
    fn wire_pill_drop(&self) {
        // A pill dragged here, from this field or another. In the capture
        // phase, ahead of the text's own target, which would take the pill
        // as typed text.
        let target = gtk::DropTarget::new(gtk::Widget::static_type(), gdk::DragAction::MOVE);
        target.set_propagation_phase(gtk::PropagationPhase::Capture);
        target.set_preload(true);
        let weak = self.downgrade();
        target.connect_accept(move |_, drop| {
            weak.upgrade().is_some()
                && drop.formats().contains_type(gtk::Widget::static_type())
                && drop.actions().contains(gdk::DragAction::MOVE)
        });
        let weak = self.downgrade();
        target.connect_motion(move |target, x, y| {
            let Some(row) = weak.upgrade() else {
                return gdk::DragAction::empty();
            };
            let dragged = target.value().and_then(|v| v.get::<gtk::Widget>().ok());
            let at = row.drop_index(x, y);
            // No marker where letting go leaves the pills as they are, nor
            // in a field with no pills, where its own highlight says enough.
            let moving = dragged.as_ref().map(acting_on).unwrap_or_default();
            let stays = row.would_stay(&moving, at);
            let empty = row.imp().pills.borrow().is_empty();
            row.imp().flow.set_marker((!stays && !empty).then_some(at));
            gdk::DragAction::MOVE
        });
        let weak = self.downgrade();
        target.connect_leave(move |_| {
            if let Some(row) = weak.upgrade() {
                row.imp().flow.set_marker(None);
            }
        });
        let weak = self.downgrade();
        target.connect_drop(move |_, value, x, y| {
            let Some(row) = weak.upgrade() else {
                return false;
            };
            row.imp().flow.set_marker(None);
            let Ok(pill) = value.get::<gtk::Widget>() else {
                return false;
            };
            row.take_pills(acting_on(&pill), row.drop_index(x, y));
            true
        });
        self.add_controller(target);
    }

    /// The field looks focused while its text or one of its pills has the
    /// focus, as an entry row does while its text has.
    pub(super) fn wire_focus(&self, area: &gtk::Box) {
        let focus = gtk::EventControllerFocus::new();
        let weak = self.downgrade();
        focus.connect_enter(move |_| {
            if let Some(row) = weak.upgrade() {
                row.imp().focused.set(true);
                row.update_state();
            }
        });
        let weak = self.downgrade();
        focus.connect_leave(move |_| {
            let Some(row) = weak.upgrade() else { return };
            row.imp().focused.set(false);
            row.update_state();
            // A whole address left behind becomes a pill like the rest, once
            // the focus has gone to another field (a pill of this one keeps
            // it here). Not when the window loses it: someone who switched
            // away to look the rest of an address up comes back to the text
            // as they left it.
            let weak = row.downgrade();
            glib::idle_add_local_once(move || {
                let Some(row) = weak.upgrade() else { return };
                // The selection goes when the focus goes somewhere else:
                // not to a pill of another field, nor to the pills' menu,
                // nor nowhere for a moment as that menu closes.
                let focus = row.root().and_then(|r| r.focus());
                let gone = focus.is_some_and(|w| {
                    !is_in_pill(&w) && w.ancestor(gtk::Popover::static_type()).is_none()
                });
                if gone {
                    row.clear_selection();
                }
                let active = row
                    .root()
                    .and_downcast::<gtk::Window>()
                    .is_some_and(|w| w.is_active());
                let typed = row.typed();
                if active && !row.imp().focused.get() && is_whole_address(&typed) {
                    row.complete(&typed);
                }
            });
        });
        area.add_controller(focus);
    }
}
