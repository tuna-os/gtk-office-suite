// SPDX-License-Identifier: GPL-3.0-or-later
//
// dialogs.rs — Modal dialogs for Letters: Page Setup, Header/Footer, Close Confirmation.

use gtk4::{self as gtk, gio, prelude::*};
use libadwaita as adw;
use adw::prelude::*;

pub fn load_page_setup_from_settings(settings: &gio::Settings) -> Option<gtk::PageSetup> {
    let name = settings.string("page-paper-name");
    if name.is_empty() { return None; }
    let ps = gtk::PageSetup::new();
    let pp = gtk::PaperSize::new(Some(&name));
    ps.set_paper_size(&pp);
    let orient = match settings.string("page-orientation").as_str() {
        "landscape" => gtk::PageOrientation::Landscape,
        _ => gtk::PageOrientation::Portrait,
    };
    ps.set_orientation(orient);
    let top = settings.double("page-margin-top");
    let bot = settings.double("page-margin-bottom");
    let left = settings.double("page-margin-left");
    let right = settings.double("page-margin-right");
    if top > 0.0 { ps.set_top_margin(top, gtk::Unit::Points); }
    if bot > 0.0 { ps.set_bottom_margin(bot, gtk::Unit::Points); }
    if left > 0.0 { ps.set_left_margin(left, gtk::Unit::Points); }
    if right > 0.0 { ps.set_right_margin(right, gtk::Unit::Points); }
    Some(ps)
}

pub fn save_page_setup_to_settings(settings: &gio::Settings, ps: &gtk::PageSetup) {
    let _ = settings.set_string("page-paper-name", ps.paper_size().name().as_str());
    let orient = match ps.orientation() {
        gtk::PageOrientation::Landscape => "landscape",
        _ => "portrait",
    };
    let _ = settings.set_string("page-orientation", orient);
    let _ = settings.set_double("page-margin-top", ps.top_margin(gtk::Unit::Points));
    let _ = settings.set_double("page-margin-bottom", ps.bottom_margin(gtk::Unit::Points));
    let _ = settings.set_double("page-margin-left", ps.left_margin(gtk::Unit::Points));
    let _ = settings.set_double("page-margin-right", ps.right_margin(gtk::Unit::Points));
}

pub fn show_header_footer_dialog(pc: &crate::page_container::PageContainer) {
    let dialog = adw::PreferencesDialog::new();
    dialog.set_title(&suite_common::i18n("Header & Footer"));

    let page = adw::PreferencesPage::new();

    let header_grp = adw::PreferencesGroup::new();
    header_grp.set_title(&suite_common::i18n("Header"));
    let header_entry = adw::EntryRow::new();
    header_entry.set_title(&suite_common::i18n("Header text"));
    header_entry.set_text(&pc.header_text());
    header_grp.add(&header_entry);

    let footer_grp = adw::PreferencesGroup::new();
    footer_grp.set_title(&suite_common::i18n("Footer"));
    let footer_entry = adw::EntryRow::new();
    footer_entry.set_title(&suite_common::i18n("Footer text"));
    footer_entry.set_text(&pc.footer_text());
    footer_grp.add(&footer_entry);

    let pageno_row = adw::SwitchRow::new();
    pageno_row.set_title(&suite_common::i18n("Show page numbers in footer"));
    pageno_row.set_active(pc.show_page_numbers());
    footer_grp.add(&pageno_row);

    let pc2 = pc.clone();
    header_entry.connect_changed(move |row| {
        pc2.set_header_text(&row.text());
    });
    let pc3 = pc.clone();
    footer_entry.connect_changed(move |row| {
        pc3.set_footer_text(&row.text());
    });
    let pc4 = pc.clone();
    pageno_row.connect_active_notify(move |row| {
        pc4.set_show_page_numbers(row.is_active());
    });

    page.add(&header_grp);
    page.add(&footer_grp);
    dialog.add(&page);
    dialog.present(Option::<&gtk::Widget>::None);
}

pub fn close_all_dirty_pages(
    tv: &adw::TabView,
    stack: &gtk4::Stack,
    on_all_closed: impl FnOnce() + 'static,
) {
    let dirty_pages: Vec<adw::TabPage> = (0..tv.n_pages())
        .map(|i| tv.nth_page(i))
        .filter(|p| {
            crate::formatting::get_textview(&p.child())
                .map(|t| t.buffer().is_modified())
                .unwrap_or(false)
        })
        .collect();

    if dirty_pages.is_empty() {
        for i in (0..tv.n_pages()).rev() {
            let p = tv.nth_page(i);
            tv.close_page_finish(&p, true);
        }
        stack.set_visible_child_name("empty");
        on_all_closed();
        return;
    }

    let n = dirty_pages.len();
    let body = if n == 1 {
        let title = dirty_pages[0].title();
        format!("\"{}\" has unsaved changes. Save before closing?", title)
    } else {
        format!("{n} documents have unsaved changes. Save before closing?")
    };

    let alert = adw::AlertDialog::builder()
        .heading(suite_common::i18n("Save Changes?"))
        .body(&body)
        .build();
    alert.add_response("cancel", &suite_common::i18n("Cancel"));
    alert.add_response("discard", &suite_common::i18n("Discard All"));
    alert.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    alert.add_response("save", &suite_common::i18n("Save All"));
    alert.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    alert.set_default_response(Some("save"));
    alert.set_close_response("cancel");

    let tv_c = tv.clone();
    let stack_c = stack.clone();
    let cb = std::cell::RefCell::new(Some(on_all_closed));
    alert.choose(Option::<&gtk::Widget>::None, None::<&gio::Cancellable>, move |resp| {
        match resp.as_str() {
            "save" => {
                for p in &dirty_pages {
                    crate::window::save_page(p);
                }
                for i in (0..tv_c.n_pages()).rev() {
                    let p = tv_c.nth_page(i);
                    tv_c.close_page_finish(&p, true);
                }
                stack_c.set_visible_child_name("empty");
                if let Some(cb) = cb.borrow_mut().take() { cb(); }
            }
            "discard" => {
                for i in (0..tv_c.n_pages()).rev() {
                    let p = tv_c.nth_page(i);
                    crate::window::clear_tab_autosave(&p.child());
                    tv_c.close_page_finish(&p, true);
                }
                stack_c.set_visible_child_name("empty");
                if let Some(cb) = cb.borrow_mut().take() { cb(); }
            }
            _ => {}
        }
    });
}
