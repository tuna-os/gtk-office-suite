// formula_bar.rs — the formula editor on the fx entry.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// docs/DESIGN-UI.md, "Formula editor with range tokens" (Numbers) and
// "formula autocomplete with argument hints" (Sheets), in libadwaita form:
// - each reference in the formula is a coloured token, tinted like a chip,
//   in the colour of the outline the grid draws round its range;
// - typing a function name opens a popover under the entry listing the
//   functions it could be (Up/Down to choose, Tab to insert);
// - inside a call, the popover shows the signature with the current
//   argument in bold.
// The text analysis is tables_core::formula_edit; this only wires GTK.

use gtk4::{self as gtk, pango, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;
use tables_core::controller::WorkbookController;
use tables_core::formula_edit::{self as fe, FunctionInfo};

/// Most completions shown at once.
const MAX_ROWS: usize = 8;

/// Byte offset of the entry's cursor (GTK counts characters).
fn cursor_byte(entry: &gtk::Entry) -> usize {
    let text = entry.text();
    let pos = entry.position().max(0) as usize;
    text.char_indices().nth(pos).map_or(text.len(), |(b, _)| b)
}

/// Chip-like tint for each reference: its colour, over a light wash of it.
fn token_attributes(tokens: &[fe::RefToken]) -> pango::AttrList {
    let attrs = pango::AttrList::new();
    for t in tokens {
        let (r, g, b) = fe::REF_COLORS[t.color % fe::REF_COLORS.len()];
        let wide = |c: u8| c as u16 * 257;
        let parts: [pango::Attribute; 4] = [
            pango::AttrColor::new_foreground(wide(r), wide(g), wide(b)).upcast(),
            pango::AttrColor::new_background(wide(r), wide(g), wide(b)).upcast(),
            pango::AttrInt::new_background_alpha(u16::MAX / 6).upcast(),
            pango::AttrInt::new_weight(pango::Weight::Semibold).upcast(),
        ];
        for mut a in parts {
            a.set_start_index(t.span.0 as u32);
            a.set_end_index(t.span.1 as u32);
            attrs.insert(a);
        }
    }
    attrs
}

fn escape(s: &str) -> String {
    gtk::glib::markup_escape_text(s).to_string()
}

/// Wire the formula editor onto `fx`. `refs` is what the grid outlines.
pub fn attach(fx: &gtk::Entry, ctl: &Rc<RefCell<WorkbookController>>, refs: &crate::window::FormulaRefs, grid: &gtk::DrawingArea) {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("navigation-sidebar");
    list.update_property(&[gtk::accessible::Property::Label("Function suggestions")]);
    let hint = gtk::Label::new(None);
    hint.set_xalign(0.0);
    hint.set_single_line_mode(true);
    hint.add_css_class("monospace");
    hint.set_margin_start(6);
    hint.set_margin_end(6);
    hint.set_margin_top(6);
    hint.set_margin_bottom(6);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&list);
    content.append(&hint);
    let popover = gtk::Popover::builder()
        .child(&content)
        .autohide(false)
        .has_arrow(false)
        .position(gtk::PositionType::Bottom)
        .halign(gtk::Align::Start)
        .build();
    popover.set_parent(fx);
    popover.add_css_class("menu");
    {
        let popover = popover.clone();
        fx.connect_destroy(move |_| popover.unparent());
    }
    // The functions currently listed, in row order.
    let shown: Rc<RefCell<Vec<&'static FunctionInfo>>> = Rc::default();
    // Moving through the list shows each suggestion's signature.
    {
        let (hint, shown) = (hint.clone(), shown.clone());
        list.connect_row_selected(move |_, row| {
            let Some(f) = row.and_then(|r| shown.borrow().get(r.index().max(0) as usize).copied()) else { return };
            hint.set_text(&format!("{}({})", f.name, f.args.join(", ")));
        });
    }

    let refresh = {
        let (fx, list, hint, popover, shown) = (fx.clone(), list.clone(), hint.clone(), popover.clone(), shown.clone());
        let (ctl, refs, grid) = (ctl.clone(), refs.clone(), grid.clone());
        Rc::new(move || {
            let text = fx.text().to_string();
            // Tokens and the grid's outlines.
            let tokens = if text.starts_with('=') {
                let Ok(c) = ctl.try_borrow() else { return };
                let state = c.state.borrow();
                let sheet_name = state.sheet().name.clone();
                let names: Vec<String> = state.engine.model.workbook.defined_names.iter().map(|n| n.name.clone()).collect();
                fe::reference_tokens(&text, &sheet_name, &names)
            } else {
                Vec::new()
            };
            fx.set_attributes(&token_attributes(&tokens));
            *refs.borrow_mut() = fe::distinct_ranges(&tokens);
            grid.queue_draw();

            // Suggestions, else the argument hint, else nothing.
            let cursor = cursor_byte(&fx);
            while let Some(row) = list.row_at_index(0) {
                list.remove(&row);
            }
            let found = fe::completions(&text, cursor).map(|(_, f)| f).unwrap_or_default();
            *shown.borrow_mut() = found.iter().take(MAX_ROWS).copied().collect();
            for f in shown.borrow().iter() {
                let label = gtk::Label::new(None);
                label.set_markup(&format!("<b>{}</b>  <span alpha=\"60%\">{}</span>", f.name, escape(f.summary)));
                label.set_xalign(0.0);
                list.append(&label);
            }
            if let Some(first) = list.row_at_index(0) {
                list.select_row(Some(&first));
            }
            // While choosing, the selected suggestion's whole signature;
            // inside a call, its signature with the current argument bold.
            let signature = match (shown.borrow().first(), fe::argument_hint(&text, cursor)) {
                (Some(f), _) => Some((format!("{}({})", f.name, f.args.join(", ")), String::new(), String::new())),
                (None, Some((f, arg))) => Some(fe::signature_parts(f, arg)),
                (None, None) => None,
            };
            list.set_visible(!shown.borrow().is_empty());
            match signature {
                // The entry's own GtkText child holds the focus, not the entry.
                Some((before, current, after)) if fx.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN) => {
                    hint.set_markup(&format!("{}<b>{}</b>{}", escape(&before), escape(&current), escape(&after)));
                    // present() re-measures a popover already showing, so
                    // it shrinks when the list goes away.
                    if popover.is_visible() {
                        popover.present();
                    } else {
                        popover.popup();
                    }
                }
                _ => popover.popdown(),
            }
        })
    };

    {
        let r = refresh.clone();
        fx.connect_changed(move |_| r());
    }
    {
        let r = refresh.clone();
        fx.connect_notify_local(Some("cursor-position"), move |_, _| r());
    }
    {
        let popover = popover.clone();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(move |_| popover.popdown());
        fx.add_controller(focus);
    }

    // Choosing a suggestion: its name and an open parenthesis replace the
    // name being typed.
    let choose = {
        let (fx, shown) = (fx.clone(), shown.clone());
        Rc::new(move |index: usize| {
            let Some(f) = shown.borrow().get(index).copied() else { return };
            let (text, cursor) = fe::apply_completion(&fx.text(), cursor_byte(&fx), f);
            fx.set_text(&text);
            fx.set_position(text[..cursor].chars().count() as i32);
        })
    };
    {
        let choose = choose.clone();
        list.connect_row_activated(move |_, row| choose(row.index().max(0) as usize));
    }

    // Keys, ahead of the entry's own: Up/Down move through the list, Tab
    // takes the selected suggestion, Escape closes the popover (a second
    // Escape leaves the entry, as before).
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let (list, popover, shown) = (list.clone(), popover.clone(), shown.clone());
        keys.connect_key_pressed(move |_, key, _, _| {
            use gtk::gdk::Key;
            use gtk::glib::Propagation;
            if !popover.is_visible() {
                return Propagation::Proceed;
            }
            if key == Key::Escape {
                popover.popdown();
                return Propagation::Stop;
            }
            let n = shown.borrow().len() as i32;
            if n == 0 {
                return Propagation::Proceed;
            }
            let at = list.selected_row().map_or(0, |r| r.index());
            match key {
                Key::Down | Key::Up => {
                    let next = if key == Key::Down { (at + 1) % n } else { (at + n - 1) % n };
                    list.select_row(list.row_at_index(next).as_ref());
                    Propagation::Stop
                }
                Key::Tab | Key::ISO_Left_Tab => {
                    choose(at.max(0) as usize);
                    Propagation::Stop
                }
                _ => Propagation::Proceed,
            }
        });
    }
    fx.add_controller(keys);
}

/// With `TABLES_TRACE_INPUT` set, what the formula bar saw goes to stderr,
/// which a failed GUI journey keeps as its app.log: for the narrow window's
/// Go to Cell journey, which only the CI runner reproduces.
pub(crate) fn trace_input(what: impl FnOnce() -> String) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ON.get_or_init(|| std::env::var_os("TABLES_TRACE_INPUT").is_some_and(|v| !v.is_empty())) {
        eprintln!("tables input: {}", what());
    }
}

/// After a jump, typing replaces the landed cell's input, as in any
/// spreadsheet. Selecting the bar's text was not enough: on the narrow
/// window, after the Go to Cell dialog, the bar was found with the caret
/// before the old value and nothing selected, and "300" typed over "200"
/// became "300200" (release soak, narrow-400 and narrow-400-dark, 4 times
/// in 8 there even with the selection restored on every focus-in). So the
/// replacing does not depend on the selection. The returned closure arms
/// the bar, and the first text inserted while it is armed first clears
/// whatever the bar holds. A click in the bar, a caret key, or leaving it
/// for another widget of the active window (Escape to the grid) disarms
/// it, so placing the caret to edit the value still edits it.
pub(crate) fn replace_on_first_input(fx: &gtk::Entry) -> Rc<dyn Fn()> {
    let armed = Rc::new(std::cell::Cell::new(false));
    let disarm = {
        let armed = armed.clone();
        move |why: &str| {
            if armed.replace(false) {
                trace_input(|| format!("disarmed by {why}"));
            }
        }
    };
    if let Some(text) = fx.delegate() {
        let armed = armed.clone();
        text.connect_insert_text(move |ed, new, pos| {
            let old = ed.text();
            trace_input(|| {
                format!("insert {new:?} at {} into {old:?}, selection {:?}, armed {}", *pos, ed.selection_bounds(), armed.get())
            });
            if armed.replace(false) && !old.is_empty() {
                ed.delete_text(0, -1);
                *pos = 0;
                trace_input(|| "replaced the old input".to_string());
            }
        });
    }
    {
        let text = fx.clone();
        fx.connect_changed(move |_| trace_input(|| format!("changed to {:?}", text.text())));
    }
    {
        let focus = gtk::EventControllerFocus::new();
        let weak = fx.downgrade();
        focus.connect_enter(move |_| {
            if let Some(fx) = weak.upgrade() {
                trace_input(|| format!("focus in, text {:?}, selection {:?}", fx.text(), fx.selection_bounds()));
            }
        });
        let (disarm, weak) = (disarm.clone(), fx.downgrade());
        focus.connect_leave(move |_| {
            trace_input(|| "focus out".to_string());
            let (disarm, weak) = (disarm.clone(), weak.clone());
            gtk::glib::idle_add_local_full(gtk::glib::Priority::HIGH, move || {
                if let Some(fx) = weak.upgrade() {
                    let active = fx.root().and_downcast::<gtk::Window>().is_some_and(|w| w.is_active());
                    if active && !fx.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN) {
                        disarm("leaving the bar");
                    }
                }
                gtk::glib::ControlFlow::Break
            });
        });
        fx.add_controller(focus);
    }
    {
        // Capture, so the click disarms before it places the caret.
        let disarm = disarm.clone();
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(move |_, _, _, _| disarm("a click"));
        fx.add_controller(click);
    }
    {
        use gtk::gdk::Key;
        let disarm = disarm.clone();
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(move |_, key, _, _| {
            if matches!(key, Key::Left | Key::Right | Key::Home | Key::End | Key::KP_Left | Key::KP_Right | Key::KP_Home | Key::KP_End) {
                disarm("a caret key");
            }
            gtk::glib::Propagation::Proceed
        });
        fx.add_controller(keys);
    }
    let weak = fx.downgrade();
    Rc::new(move || {
        if let Some(fx) = weak.upgrade() {
            fx.select_region(0, -1);
            trace_input(|| format!("armed on {:?}, focused {}", fx.text(), fx.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN)));
        }
        armed.set(true);
    })
}

#[cfg(test)]
mod tests {
    use gtk4::{self as gtk, prelude::*};

    /// The first input after a jump replaces the bar's text even when the
    /// selection is gone and the caret sits before the old value — the
    /// state the narrow window's Go to Cell journey found the bar in.
    #[test]
    fn the_first_input_after_a_jump_replaces_without_a_selection() {
        suite_common::gtk_test::run(|| {
            let fx = gtk::Entry::new();
            fx.set_text("200");
            let arm = super::replace_on_first_input(&fx);
            arm();
            fx.select_region(0, 0);
            fx.set_position(0);
            let mut pos = 0;
            fx.insert_text("3", &mut pos);
            assert_eq!(fx.text(), "3");
            // Only the first: the rest of the value is typed after it.
            let mut pos = 1;
            fx.insert_text("00", &mut pos);
            assert_eq!(fx.text(), "300");
        });
    }

    /// Unarmed, input goes where the caret is, as in any entry.
    #[test]
    fn without_a_jump_input_inserts_at_the_caret() {
        suite_common::gtk_test::run(|| {
            let fx = gtk::Entry::new();
            fx.set_text("200");
            let _arm = super::replace_on_first_input(&fx);
            let mut pos = 3;
            fx.insert_text("1", &mut pos);
            assert_eq!(fx.text(), "2001");
        });
    }
}
