//! layout_picker.rs — the inspector's Layout row: which of its master's
//! layouts the current slide is on, and putting it on another.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Keynote's and Google Slides' Layout menu, as a combo row in the Slide
//! group. Choosing a layout calls `DecksController::apply_layout` (one
//! undo step): the slide's title and body boxes move to the layout's
//! places, and it gains an empty box for any place it had nothing for. A
//! master with no layouts offers the standard six, and gets them on the
//! first choice.

use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use std::cell::Cell;
use std::rc::Rc;

use decks_core::DecksController;

pub(crate) struct LayoutPicker {
    pub row: adw::ComboRow,
    /// Fill the row from the model (the current slide's master and layout).
    pub sync: Rc<dyn Fn()>,
}

/// The names the row offers for the current slide: its master's layouts,
/// or the standard ones.
fn names(ctl: &DecksController, slide: usize) -> Vec<String> {
    let mi = ctl.slides.borrow().get(slide).and_then(|s| s.master_idx).unwrap_or(0);
    let masters = ctl.masters.borrow();
    match masters.get(mi).filter(|m| !m.layouts.is_empty()) {
        Some(m) => m.layouts.iter().map(|l| l.name.clone()).collect(),
        None => decks_core::layouts::standard().into_iter().map(|l| l.name).collect(),
    }
}

pub(crate) fn build(ctl: &Rc<DecksController>, current_slide: &Rc<Cell<usize>>, changed: Rc<dyn Fn()>) -> LayoutPicker {
    let model = gtk::StringList::new(&[]);
    let row = adw::ComboRow::builder().title("Layout").model(&model).build();
    row.set_subtitle("Where the title and text go");
    let syncing = Rc::new(Cell::new(false));
    let sync: Rc<dyn Fn()> = {
        let (ctl, cs, row, model, syncing) = (ctl.clone(), current_slide.clone(), row.clone(), model.clone(), syncing.clone());
        Rc::new(move || {
            syncing.set(true);
            let wanted = names(&ctl, cs.get());
            let now: Vec<String> = (0..model.n_items()).filter_map(|i| model.string(i).map(|s| s.to_string())).collect();
            if now != wanted {
                let refs: Vec<&str> = wanted.iter().map(String::as_str).collect();
                model.splice(0, model.n_items(), &refs);
            }
            let layout = ctl.slides.borrow().get(cs.get()).and_then(|s| s.layout);
            row.set_selected(layout.map_or(gtk::INVALID_LIST_POSITION, |l| l as u32));
            syncing.set(false);
        })
    };
    {
        let (ctl, cs, syncing) = (ctl.clone(), current_slide.clone(), syncing.clone());
        row.connect_selected_notify(move |r| {
            if syncing.get() || r.selected() == gtk::INVALID_LIST_POSITION {
                return;
            }
            let already = ctl.slides.borrow().get(cs.get()).and_then(|s| s.layout) == Some(r.selected() as usize);
            if !already && ctl.apply_layout(cs.get(), r.selected() as usize) {
                changed();
            }
        });
    }
    LayoutPicker { row, sync }
}
