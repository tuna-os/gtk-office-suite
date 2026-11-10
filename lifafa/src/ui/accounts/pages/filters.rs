//! Settings → Filters: the search box, the rules and Apply Now.

use adw::prelude::*;
use relm4::prelude::*;

use super::super::{AccountsInput, AccountsWindow};
use crate::i18n::i18n;

/// The page, and the widgets on it that the panel reaches later.
#[derive(Debug)]
pub(in crate::ui::accounts) struct FiltersPage {
    pub(in crate::ui::accounts) page: adw::PreferencesPage,
    pub(in crate::ui::accounts) apply_filters_spinner: gtk::Spinner,
    pub(in crate::ui::accounts) apply_filters_label: gtk::Label,
    pub(in crate::ui::accounts) filters_list: gtk::ListBox,
}

impl FiltersPage {
    pub(in crate::ui::accounts) fn build(sender: &ComponentSender<AccountsWindow>) -> Self {
        let page = adw::PreferencesPage::default();
        // A search box over the long lists (#141): rows
        // that do not match the text are hidden.
        let preferences_group_1 = adw::PreferencesGroup::default();
        let filters_search = gtk::SearchEntry::default();
        filters_search.set_placeholder_text(Some(i18n("Search filters").as_str()));
        {
            let sender = sender.clone();
            filters_search.connect_search_changed(move |entry| {
                sender
                    .input_sender()
                    .emit(AccountsInput::SearchFilters(entry.text().to_string()));
            });
        }
        preferences_group_1.add(&filters_search);
        page.add(&preferences_group_1);
        // Mail hygiene (moved from Settings): filters,
        // the remote-content allow list, the blocklist.
        let preferences_group_2 = adw::PreferencesGroup::default();
        preferences_group_2.set_title(&i18n("Filters"));
        preferences_group_2.set_description(Some(
                        i18n("File or tag incoming mail automatically. Filters run on each Inbox as it syncs; Apply Now runs them over the mail already there.").as_str()
                    ));
        // At the header's end, like the other
        // panels' buttons.
        let box_3 = gtk::Box::default();
        box_3.set_orientation(gtk::Orientation::Vertical);
        box_3.set_spacing(6);
        box_3.set_valign(gtk::Align::Start);
        box_3.set_halign(gtk::Align::End);
        box_3.set_margin_start(24);
        let button_4 = gtk::Button::default();
        button_4.set_label(&i18n("Add Filter…"));
        button_4.set_size_request(130, -1);
        {
            let sender = sender.clone();
            button_4.connect_clicked(move |_| sender.input(AccountsInput::AddFilter));
        }
        box_3.append(&button_4);
        // Rules normally meet mail as it
        // arrives; this holds them up against
        // what is already in the Inbox (#198).
        let apply_filters_btn = gtk::Button::default();
        apply_filters_btn.set_size_request(130, -1);
        apply_filters_btn.set_tooltip_text(Some(
            i18n("Run every rule over the mail already in your inboxes").as_str(),
        ));
        {
            let sender = sender.clone();
            apply_filters_btn.connect_clicked(move |_| sender.input(AccountsInput::ApplyFilters));
        }
        // A spinner inside the button
        // while the run is under way, as
        // the tag finder has.
        let box_5 = gtk::Box::default();
        box_5.set_spacing(6);
        box_5.set_halign(gtk::Align::Center);
        let apply_filters_spinner = gtk::Spinner::default();
        apply_filters_spinner.set_visible(false);
        box_5.append(&apply_filters_spinner);
        let apply_filters_label = gtk::Label::default();
        apply_filters_label.set_label(&i18n("Apply Now"));
        box_5.append(&apply_filters_label);
        apply_filters_btn.set_child(&box_5);
        box_3.append(&apply_filters_btn);
        preferences_group_2.set_header_suffix(Some(&box_3));
        let filters_list = gtk::ListBox::default();
        filters_list.add_css_class("boxed-list");
        filters_list.set_selection_mode(gtk::SelectionMode::None);
        preferences_group_2.add(&filters_list);
        page.add(&preferences_group_2);
        Self {
            page,
            apply_filters_spinner,
            apply_filters_label,
            filters_list,
        }
    }
}
