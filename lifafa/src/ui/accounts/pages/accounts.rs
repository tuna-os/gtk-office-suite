//! Settings → Accounts: the mail accounts, and the GNOME Online Accounts
//! ones that can be switched on.

use adw::prelude::*;
use crate::relm::prelude::*;

use super::super::{AccountsInput, AccountsWindow};
use crate::i18n::i18n;

/// The page, and the widgets on it that the panel reaches later.
#[derive(Debug)]
pub(in crate::ui::accounts) struct AccountsPage {
    pub(in crate::ui::accounts) page: adw::PreferencesPage,
    pub(in crate::ui::accounts) goa_refresh_btn: gtk::Button,
    pub(in crate::ui::accounts) accounts_list: gtk::ListBox,
    pub(in crate::ui::accounts) goa_group: adw::PreferencesGroup,
    pub(in crate::ui::accounts) local_group: adw::PreferencesGroup,
    pub(in crate::ui::accounts) goa_list: gtk::ListBox,
}

impl AccountsPage {
    pub(in crate::ui::accounts) fn build(sender: &ComponentSender<AccountsWindow>) -> Self {
        let page = adw::PreferencesPage::default();
        let preferences_group_1 = adw::PreferencesGroup::default();
        preferences_group_1.set_title(&i18n("Mail Accounts"));
        preferences_group_1
            .set_description(Some(i18n("Drag to reorder them in the sidebar.").as_str()));
        // At the header's end, as the Tags and
        // Cloud Storage panels place theirs.
        let box_2 = gtk::Box::default();
        box_2.set_orientation(gtk::Orientation::Horizontal);
        box_2.set_spacing(6);
        box_2.set_valign(gtk::Align::Start);
        box_2.set_halign(gtk::Align::End);
        box_2.set_margin_start(24);
        // An account just added in GNOME
        // Settings, without waiting for GOA
        // to say so.
        let goa_refresh_btn = gtk::Button::default();
        goa_refresh_btn.set_icon_name("view-refresh-symbolic");
        goa_refresh_btn
            .set_tooltip_text(Some(&i18n("Check GNOME Online Accounts for new accounts")));
        {
            let sender = sender.clone();
            goa_refresh_btn.connect_clicked(move |_| sender.input(AccountsInput::RefreshGoa));
        }
        box_2.append(&goa_refresh_btn);
        let button_3 = gtk::Button::default();
        button_3.set_label(&i18n("Add Account…"));
        button_3.set_size_request(130, -1);
        {
            let sender = sender.clone();
            button_3.connect_clicked(move |_| sender.input(AccountsInput::AddAccount));
        }
        box_2.append(&button_3);
        preferences_group_1.set_header_suffix(Some(&box_2));
        let accounts_list = gtk::ListBox::default();
        accounts_list.add_css_class("boxed-list");
        accounts_list.set_selection_mode(gtk::SelectionMode::None);
        {
            let sender = sender.clone();
            accounts_list.connect_row_activated(move |_, row| {
                sender
                    .input_sender()
                    .emit(AccountsInput::EditAccount(row.index() as usize));
            });
        }
        preferences_group_1.add(&accounts_list);
        page.add(&preferences_group_1);
        // On This Computer (#552): folders on this computer
        // rather than a server, offered until there is one.
        let local_group = adw::PreferencesGroup::default();
        local_group.set_title(&i18n("On This Computer"));
        local_group.set_description(Some(
                        i18n("Store mail in folders that live on this computer. Move or copy mail from any account to here.").as_str()
                    ));
        local_group.set_visible(false);
        let button_4 = gtk::Button::default();
        button_4.set_label(&i18n("Add"));
        button_4.set_valign(gtk::Align::Start);
        {
            let sender = sender.clone();
            button_4.connect_clicked(move |_| sender.input(AccountsInput::AddLocal));
        }
        local_group.set_header_suffix(Some(&button_4));
        page.add(&local_group);
        let goa_group = adw::PreferencesGroup::default();
        goa_group.set_title(&i18n("GNOME Online Accounts"));
        goa_group.set_description(Some(
            i18n("Mail accounts from GNOME Settings. Switch one on to use it.").as_str(),
        ));
        goa_group.set_visible(false);
        let goa_list = gtk::ListBox::default();
        goa_list.add_css_class("boxed-list");
        goa_list.set_selection_mode(gtk::SelectionMode::None);
        goa_group.add(&goa_list);
        page.add(&goa_group);
        Self {
            page,
            goa_refresh_btn,
            accounts_list,
            goa_group,
            local_group,
            goa_list,
        }
    }
}
