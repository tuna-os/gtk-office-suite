//! Settings → Smart Folders: the list of saved searches.

use adw::prelude::*;
use crate::relm::prelude::*;

use super::super::{AccountsInput, AccountsWindow};
use crate::i18n::i18n;

/// The page, and the widgets on it that the panel reaches later.
#[derive(Debug)]
pub(in crate::ui::accounts) struct SmartPage {
    pub(in crate::ui::accounts) page: adw::PreferencesPage,
    pub(in crate::ui::accounts) smart_list: gtk::ListBox,
}

impl SmartPage {
    pub(in crate::ui::accounts) fn build(sender: &ComponentSender<AccountsWindow>) -> Self {
        let page = adw::PreferencesPage::default();
        // Smart folders (#417): saved searches.
        let preferences_group_1 = adw::PreferencesGroup::default();
        preferences_group_1.set_title(&i18n("Smart Folders"));
        preferences_group_1.set_description(Some(
                        i18n("Lists the messages that match its conditions. Shown in the sidebar under the unified rows.").as_str()
                    ));
        let button_2 = gtk::Button::default();
        button_2.set_label(&i18n("Add Smart Folder…"));
        button_2.set_valign(gtk::Align::Start);
        button_2.set_margin_start(24);
        {
            let sender = sender.clone();
            button_2.connect_clicked(move |_| sender.input(AccountsInput::AddSmartFolder));
        }
        preferences_group_1.set_header_suffix(Some(&button_2));
        let smart_list = gtk::ListBox::default();
        smart_list.add_css_class("boxed-list");
        smart_list.set_selection_mode(gtk::SelectionMode::None);
        preferences_group_1.add(&smart_list);
        page.add(&preferences_group_1);
        Self { page, smart_list }
    }
}
