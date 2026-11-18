//! Settings → Backup: export and import of the settings.

use adw::prelude::*;

use super::super::{PrefInput, ViewSender};
use crate::i18n::i18n;

/// The page, and the rows on it that the window reaches later.
#[derive(Debug)]
pub(in crate::ui::preferences) struct BackupPage {
    pub(in crate::ui::preferences) page: adw::PreferencesPage,
}

impl BackupPage {
    pub(in crate::ui::preferences) fn build(sender: &ViewSender) -> Self {
        let page = adw::PreferencesPage::default();
        let preferences_group_1 = adw::PreferencesGroup::default();
        preferences_group_1.set_title(&i18n("Backup"));
        preferences_group_1.set_description(Some(
                        i18n("Accounts, preferences, filters, tags and layout, in one file. Passwords, OAuth client secrets and OpenPGP keys are not included.").as_str()
                    ));
        let action_row_2 = adw::ActionRow::default();
        action_row_2.set_title(&i18n("Export settings"));
        action_row_2.set_activatable(true);
        {
            let sender = sender.clone();
            action_row_2.connect_activated(move |_| sender.input(PrefInput::ExportSettings));
        }
        let image_3 = gtk::Image::default();
        image_3.set_icon_name(Some("go-next-symbolic"));
        action_row_2.add_suffix(&image_3);
        preferences_group_1.add(&action_row_2);
        let action_row_4 = adw::ActionRow::default();
        action_row_4.set_title(&i18n("Import settings"));
        action_row_4.set_subtitle(&i18n("Replaces everything in place. Don't remove accounts first: that deletes their passwords."));
        action_row_4.set_activatable(true);
        {
            let sender = sender.clone();
            action_row_4.connect_activated(move |_| sender.input(PrefInput::ImportSettings));
        }
        let image_5 = gtk::Image::default();
        image_5.set_icon_name(Some("go-next-symbolic"));
        action_row_4.add_suffix(&image_5);
        preferences_group_1.add(&action_row_4);
        page.add(&preferences_group_1);
        Self { page }
    }
}
