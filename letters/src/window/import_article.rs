// SPDX-License-Identifier: GPL-3.0-or-later
// Import a saved web article (#1787): an explicit, local-only import into a
// NEW untitled tab. The parsing lives in letters_core::article; nothing is
// fetched or run, and a failure leaves every open tab alone.

use super::{
    apply_page_setup_from_buffer, connect_style_readout, connect_word_count, make_doc_widget,
    tab_data_set, TabData,
};
use gtk4::{self as gtk, gio, glib, prelude::*};
use libadwaita as adw;

pub(super) fn register(
    app: &adw::Application,
    tv: &adw::TabView,
    st: &gtk::Stack,
    wc: &gtk::Label,
    sl: &gtk::Label,
    win: &adw::ApplicationWindow,
    settings: &gio::Settings,
) {
    let (tv, st, w, s, wc, sl) = (tv.clone(), st.clone(), win.clone(), settings.clone(), wc.clone(), sl.clone());
    let a = gtk::gio::SimpleAction::new("import-web-article", None);
    a.connect_activate(move |_, _| {
        let tv = tv.clone(); let st = st.clone(); let w = w.clone(); let s = s.clone(); let wc = wc.clone();
        let sl = sl.clone();
        let filter = gtk::FileFilter::new();
        for ext in ["html", "htm", "xhtml"] {
            filter.add_suffix(ext);
        }
        filter.set_name(Some(&suite_common::i18n("Saved web pages (.html, .htm)")));
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dlg = gtk::FileDialog::new();
        dlg.set_title(&suite_common::i18n("Import Saved Web Article"));
        dlg.set_filters(Some(&filters));
        let w_dlg = w.clone();
        dlg.open(Some(&w_dlg), None::<&gio::Cancellable>, move |result: Result<gio::File, glib::Error>| {
            let Ok(file) = result else { return };
            let heading = suite_common::i18n("Could Not Import Article");
            let path = match suite_common::locations::open_location(&file) {
                Ok(path) => path,
                Err(e) => {
                    suite_common::show_error_dialog(Some(&w), &heading, &e);
                    return;
                }
            };
            glib::spawn_future_local(async move {
                let target = path.clone();
                let imported = gio::spawn_blocking(move || letters_core::article::import_file(&target))
                    .await
                    .unwrap_or_else(|_| Err("Article import stopped unexpectedly".to_string()));
                let imp = match imported {
                    Ok(imp) => imp,
                    Err(e) => {
                        suite_common::show_error_dialog(Some(&w), &heading, &e);
                        return;
                    }
                };
                // Only now is a tab built: the import cannot have touched one.
                let (container, buf) = make_doc_widget(Some(&s));
                crate::bridge::load_document(&imp.document, &buf);
                apply_page_setup_from_buffer(&container, &buf);
                let page = tv.append(&container);
                let title = imp.title.clone().unwrap_or_else(|| suite_common::i18n("Imported Article"));
                page.set_title(&title);
                page.set_tooltip(&title);
                st.set_visible_child_name("editor");
                tab_data_set(&container, TabData::new());
                let p = tv.page(&container);
                buf.connect_modified_changed(move |b| { p.set_needs_attention(b.is_modified()); });
                connect_word_count(&buf, &wc);
                connect_style_readout(&buf, &sl);
                // Unsaved: closing offers Save As, which picks the format.
                buf.set_modified(true);
                page.set_needs_attention(true);
                tv.set_selected_page(&page);
                if !imp.notes.is_empty() {
                    suite_common::dialogs::show_message(
                        Some(&w),
                        &suite_common::i18n("Article Imported"),
                        &imp.notes.join("\n"),
                    );
                }
            });
        });
    });
    app.add_action(&a);
}
