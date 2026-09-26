//! export_ui.rs — Export as PDF, Export Handouts and Export Slide as PNG:
//! the actions, their save dialogs, and their place in the primary menu.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Keynote's File ▸ Export To and Google Slides' Download: an "Export"
//! section in the primary menu. Handouts offer 2, 4 or 6 slides to a page
//! as a submenu (the action's parameter). What is drawn is export.rs; the
//! deck exported is the one Save would write (`DecksController::deck`).

use adw::prelude::*;
use gtk4::{self as gtk, gio, glib};
use libadwaita as adw;
use std::cell::Cell;
use std::rc::Rc;

use decks_core::DecksController;

/// The primary menu the suite's header bar opens (its "open-menu"
/// button), to add the Export section to.
fn primary_menu(widget: &gtk::Widget) -> Option<gio::Menu> {
    if let Some(b) = widget.downcast_ref::<gtk::MenuButton>() {
        if b.icon_name().as_deref() == Some("open-menu-symbolic") {
            return b.menu_model().and_then(|m| m.downcast::<gio::Menu>().ok());
        }
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        if let Some(m) = primary_menu(&c) {
            return Some(m);
        }
        child = c.next_sibling();
    }
    None
}

fn menu_section() -> gio::Menu {
    let section = gio::Menu::new();
    section.append(Some("Export as _PDF…"), Some("app.export-pdf"));
    let handouts = gio::Menu::new();
    for n in [2u32, 4, 6] {
        let item = gio::MenuItem::new(Some(&format!("{n} Slides per Page…")), None);
        item.set_action_and_target_value(Some("app.export-handouts"), Some(&n.to_variant()));
        handouts.append_item(&item);
    }
    section.append_submenu(Some("Export _Handouts"), &handouts);
    section.append(Some("Export Slide as P_NG…"), Some("app.export-png"));
    section
}

/// Ask where to save a `kind` export named `name`, then run `write` on the
/// local path (staged for a remote location, and uploaded after).
fn save_as(window: &adw::ApplicationWindow, title: &str, name: &str, pattern: &str, filter_name: &str, write: impl Fn(&std::path::Path) -> Result<(), String> + 'static) {
    let dialog = gtk::FileDialog::builder().title(title).initial_name(name).modal(true).build();
    let filter = gtk::FileFilter::new();
    filter.add_pattern(pattern);
    filter.set_name(Some(filter_name));
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));
    let win = window.clone();
    dialog.save(Some(window), None::<&gio::Cancellable>, move |result| {
        let Ok(file) = result else { return };
        let outcome = suite_common::locations::save_location(&file)
            .and_then(|path| write(&path).and_then(|_| suite_common::locations::commit_save(&path)));
        if let Err(e) = outcome {
            suite_common::show_error_dialog(Some(&win), &suite_common::i18n("Export Failed"), &e);
        }
    });
}

/// The document's name without its extension, for the dialogs.
fn stem(ctl: &DecksController) -> String {
    ctl.file_path
        .borrow()
        .as_deref()
        .and_then(|p| std::path::Path::new(p).file_stem().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_else(|| "Presentation".into())
}

pub(crate) fn register(app: &adw::Application, window: &adw::ApplicationWindow, header: &adw::HeaderBar, ctl: &Rc<DecksController>, current_slide: &Rc<Cell<usize>>) {
    if let Some(menu) = primary_menu(header.upcast_ref()) {
        // After File, before Edit and Help.
        menu.insert_section(1, Some("Export"), &menu_section());
    }
    {
        let (ctl, win) = (ctl.clone(), window.clone());
        let act = gio::SimpleAction::new("export-pdf", None);
        act.connect_activate(move |_, _| {
            let deck = ctl.deck();
            save_as(&win, "Export as PDF", &format!("{}.pdf", stem(&ctl)), "*.pdf", "PDF Documents", move |p| {
                crate::export::export_pdf(&deck, p, None)
            });
        });
        app.add_action(&act);
    }
    {
        let (ctl, win) = (ctl.clone(), window.clone());
        let act = gio::SimpleAction::new("export-handouts", Some(glib::VariantTy::UINT32));
        act.connect_activate(move |_, p| {
            let n = p.and_then(|p| p.get::<u32>()).filter(|n| [2, 4, 6].contains(n)).unwrap_or(6) as usize;
            let deck = ctl.deck();
            save_as(&win, "Export Handouts", &format!("{} Handouts.pdf", stem(&ctl)), "*.pdf", "PDF Documents", move |p| {
                crate::export::export_pdf(&deck, p, Some(n))
            });
        });
        app.add_action(&act);
    }
    {
        let (ctl, win, cs) = (ctl.clone(), window.clone(), current_slide.clone());
        let act = gio::SimpleAction::new("export-png", None);
        act.connect_activate(move |_, _| {
            let deck = ctl.deck();
            let index = cs.get();
            let name = format!("{} Slide {}.png", stem(&ctl), index + 1);
            save_as(&win, "Export Slide as PNG", &name, "*.png", "PNG Images", move |p| {
                crate::export::export_png(&deck, index, 1920, p)
            });
        });
        app.add_action(&act);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action_of(m: &gio::MenuModel, i: i32) -> Option<(String, Option<u32>)> {
        let action = m.item_attribute_value(i, "action", Some(glib::VariantTy::STRING))?.get::<String>()?;
        let target = m.item_attribute_value(i, "target", None).and_then(|v| v.get::<u32>());
        Some((action, target))
    }

    #[test]
    fn the_export_section_offers_pdf_handouts_and_png() {
        let section = menu_section();
        let m: &gio::MenuModel = section.upcast_ref();
        assert_eq!(m.n_items(), 3);
        assert_eq!(action_of(m, 0), Some(("app.export-pdf".into(), None)));
        assert_eq!(action_of(m, 2), Some(("app.export-png".into(), None)));
        let handouts = m.item_link(1, "submenu").expect("a handouts submenu");
        let targets: Vec<Option<(String, Option<u32>)>> = (0..handouts.n_items()).map(|i| action_of(&handouts, i)).collect();
        assert_eq!(
            targets,
            [2, 4, 6].iter().map(|n| Some(("app.export-handouts".to_string(), Some(*n)))).collect::<Vec<_>>()
        );
    }
}
