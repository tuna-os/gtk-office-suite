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
fn names(ctl: &DecksController) -> Vec<String> {
    let slides = ctl.slides.borrow();
    let slide_id = ctl.selection().slide_id;
    let slide = slide_id.and_then(|id| slides.iter().find(|s| s.ids.slide == id));
    let masters = ctl.masters.borrow();
    let master = slide.and_then(|s| s.master_id).and_then(|id| masters.iter().find(|m| m.id == id)).or_else(|| masters.first());
    match master.filter(|m| !m.layouts.is_empty()) {
        Some(m) => m.layouts.iter().map(|l| l.name.clone()).collect(),
        None => decks_core::layouts::standard().into_iter().map(|l| l.name).collect(),
    }
}

pub(crate) fn build(ctl: &Rc<DecksController>, changed: Rc<dyn Fn()>) -> LayoutPicker {
    let model = gtk::StringList::new(&[]);
    let row = adw::ComboRow::builder().title("Layout").model(&model).build();
    row.set_subtitle("Where the title and text go");
    let syncing = Rc::new(Cell::new(false));
    let sync: Rc<dyn Fn()> = {
        let (ctl, row, model, syncing) = (ctl.clone(), row.clone(), model.clone(), syncing.clone());
        Rc::new(move || {
            syncing.set(true);
            let wanted = names(&ctl);
            let now: Vec<String> = (0..model.n_items()).filter_map(|i| model.string(i).map(|s| s.to_string())).collect();
            if now != wanted {
                let refs: Vec<&str> = wanted.iter().map(String::as_str).collect();
                model.splice(0, model.n_items(), &refs);
            }
            let slides = ctl.slides.borrow();
            let slide = ctl.selection().slide_id.and_then(|id| slides.iter().find(|s| s.ids.slide == id));
            let masters = ctl.masters.borrow();
            let selected = slide.and_then(|s| {
                let master = s.master_id.and_then(|id| masters.iter().find(|m| m.id == id))?;
                s.layout_id.and_then(|id| master.layout_index(id))
            });
            row.set_selected(selected.map_or(gtk::INVALID_LIST_POSITION, |l| l as u32));
            syncing.set(false);
        })
    };
    {
        let (ctl, syncing) = (ctl.clone(), syncing.clone());
        row.connect_selected_notify(move |r| {
            if syncing.get() || r.selected() == gtk::INVALID_LIST_POSITION {
                return;
            }
            let row_idx = r.selected() as usize;
            let Some(slide_id) = ctl.selection().slide_id else { return };
            let apply = {
                let slides = ctl.slides.borrow();
                let slide = slides.iter().find(|s| s.ids.slide == slide_id);
                let masters = ctl.masters.borrow();
                let master = slide.and_then(|s| s.master_id).and_then(|id| masters.iter().find(|m| m.id == id));
                match master.filter(|m| !m.layouts.is_empty()) {
                    Some(m) => m.layouts.get(row_idx).map(|l| (false, l.id)),
                    None => Some((true, row_idx as u64)),
                }
            };
            let Some((standard, id)) = apply else { return };
            let changed_slide = if standard {
                ctl.apply_standard_layout(slide_id, id as usize)
            } else {
                ctl.apply_layout(slide_id, id)
            };
            if changed_slide {
                changed();
            }
        });
    }
    LayoutPicker { row, sync }
}
