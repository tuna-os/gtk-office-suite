//! Settings → Senders: the allowed and the blocked senders. Their lists are
//! the factories' own boxes, handed in.

use adw::prelude::*;
use relm4::prelude::*;

use super::super::{AccountsInput, AccountsWindow};
use crate::i18n::i18n;

/// The page: nothing on it is reached later, the lists being the
/// factories' own.
#[derive(Debug)]
pub(in crate::ui::accounts) struct SendersPage {
    pub(in crate::ui::accounts) page: adw::PreferencesPage,
}

impl SendersPage {
    pub(in crate::ui::accounts) fn build(
        sender: &ComponentSender<AccountsWindow>,
        senders_box: &gtk::ListBox,
        blacklist_box: &gtk::ListBox,
    ) -> Self {
        let page = adw::PreferencesPage::default();
        // A search box over the long lists (#141): rows
        // that do not match the text are hidden.
        let preferences_group_1 = adw::PreferencesGroup::default();
        let senders_search = gtk::SearchEntry::default();
        senders_search.set_placeholder_text(Some(i18n("Search senders").as_str()));
        {
            let sender = sender.clone();
            senders_search.connect_search_changed(move |entry| {
                sender
                    .input_sender()
                    .emit(AccountsInput::SearchSenders(entry.text().to_string()));
            });
        }
        preferences_group_1.add(&senders_search);
        page.add(&preferences_group_1);
        let preferences_group_2 = adw::PreferencesGroup::default();
        preferences_group_2.set_title(&i18n("Allowed Senders"));
        preferences_group_2.set_description(Some(
            i18n(
                "Messages from these senders load remote content \
                         automatically.",
            )
            .as_str(),
        ));
        let add_sender_row = adw::EntryRow::default();
        add_sender_row.set_title(&i18n("Email address"));
        add_sender_row.set_input_purpose(gtk::InputPurpose::Email);
        add_sender_row.set_show_apply_button(false);
        {
            let sender = sender.clone();
            add_sender_row.connect_entry_activated(move |row| {
                sender
                    .input_sender()
                    .emit(AccountsInput::AddSenderText(row.text().to_string()));
                row.set_text("");
            });
        }
        let button_3 = gtk::Button::default();
        button_3.set_icon_name("list-add-symbolic");
        button_3.set_tooltip_text(Some(i18n("Allow this sender").as_str()));
        button_3.set_valign(gtk::Align::Center);
        button_3.add_css_class("flat");
        {
            let sender = sender.clone();
            let add_sender_row = add_sender_row.clone();
            button_3.connect_clicked(move |_| {
                sender.input_sender().emit(AccountsInput::AddSenderText(
                    add_sender_row.text().to_string(),
                ));
                add_sender_row.set_text("");
            });
        }
        add_sender_row.add_suffix(&button_3);
        preferences_group_2.add(&add_sender_row);
        senders_box.add_css_class("boxed-list");
        senders_box.add_css_class("sender-list");
        senders_box.set_selection_mode(gtk::SelectionMode::None);
        preferences_group_2.add(&senders_box);
        page.add(&preferences_group_2);
        let preferences_group_4 = adw::PreferencesGroup::default();
        preferences_group_4.set_title(&i18n("Blocked Senders"));
        preferences_group_4.set_description(Some(
                        i18n("Their mail is moved to Trash on arrival. Enter an address, or a domain like \"example.com\" to apply to all senders from that domain.").as_str()
                    ));
        let add_blacklist_row = adw::EntryRow::default();
        add_blacklist_row.set_title(&i18n("Address or domain"));
        add_blacklist_row.set_show_apply_button(false);
        {
            let sender = sender.clone();
            add_blacklist_row.connect_entry_activated(move |row| {
                sender
                    .input_sender()
                    .emit(AccountsInput::AddBlacklistText(row.text().to_string()));
                row.set_text("");
            });
        }
        let button_5 = gtk::Button::default();
        button_5.set_icon_name("list-add-symbolic");
        button_5.set_tooltip_text(Some(i18n("Block this sender").as_str()));
        button_5.set_valign(gtk::Align::Center);
        button_5.add_css_class("flat");
        {
            let sender = sender.clone();
            let add_blacklist_row = add_blacklist_row.clone();
            button_5.connect_clicked(move |_| {
                sender.input_sender().emit(AccountsInput::AddBlacklistText(
                    add_blacklist_row.text().to_string(),
                ));
                add_blacklist_row.set_text("");
            });
        }
        add_blacklist_row.add_suffix(&button_5);
        preferences_group_4.add(&add_blacklist_row);
        blacklist_box.add_css_class("boxed-list");
        blacklist_box.add_css_class("sender-list");
        blacklist_box.set_selection_mode(gtk::SelectionMode::None);
        preferences_group_4.add(&blacklist_box);
        page.add(&preferences_group_4);
        Self { page }
    }
}
