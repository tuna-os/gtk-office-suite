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
