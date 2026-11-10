//! Settings → Privacy.

use adw::prelude::*;

use super::super::{PrefOutput, ViewSender};
use crate::i18n::i18n;

/// The page, and the rows on it that the window reaches later.
#[derive(Debug)]
pub(in crate::ui::preferences) struct PrivacyPage {
    pub(in crate::ui::preferences) page: adw::PreferencesPage,
    pub(in crate::ui::preferences) auto_remote_content_row: adw::SwitchRow,
    pub(in crate::ui::preferences) remote_ahead_row: adw::SwitchRow,
    pub(in crate::ui::preferences) show_remote_banner_row: adw::SwitchRow,
    pub(in crate::ui::preferences) show_spoof_banner_row: adw::SwitchRow,
    pub(in crate::ui::preferences) gravatar_row: adw::SwitchRow,
    pub(in crate::ui::preferences) sender_logos_row: adw::SwitchRow,
    pub(in crate::ui::preferences) receipts_row: adw::ComboRow,
}

impl PrivacyPage {
    pub(in crate::ui::preferences) fn build(sender: &ViewSender) -> Self {
        let page = adw::PreferencesPage::default();
        let preferences_group_1 = adw::PreferencesGroup::default();
        preferences_group_1.set_title(&i18n("Privacy"));
        preferences_group_1.set_description(Some(
            i18n("Hylki collects no telemetry and sends no analytics.").as_str(),
        ));
        let auto_remote_content_row = adw::SwitchRow::default();
        auto_remote_content_row.set_title(&i18n("Always load remote content"));
        auto_remote_content_row.set_subtitle(&i18n(
            "Remote content can tell a sender when and where you read.",
        ));
        {
            let sender = sender.clone();
            auto_remote_content_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetAutoRemoteContent(row.is_active()));
            });
        }
        preferences_group_1.add(&auto_remote_content_row);
        let remote_ahead_row = adw::SwitchRow::default();
        remote_ahead_row.set_title(&i18n("Pre-load remote content for the next messages"));
        remote_ahead_row.set_subtitle(&i18n("Their senders can tell they were opened."));
        {
            let sender = sender.clone();
            remote_ahead_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetRemoteAhead(row.is_active()));
            });
        }
        preferences_group_1.add(&remote_ahead_row);
        let show_remote_banner_row = adw::SwitchRow::default();
        show_remote_banner_row.set_title(&i18n("Warn when remote content is blocked"));
        show_remote_banner_row
            .set_subtitle(&i18n("Off hides the banner. Remote content stays blocked."));
        {
            let sender = sender.clone();
            show_remote_banner_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetShowRemoteBanner(row.is_active()));
            });
        }
        preferences_group_1.add(&show_remote_banner_row);
        let show_spoof_banner_row = adw::SwitchRow::default();
        show_spoof_banner_row.set_title(&i18n("Warn when the addressing doesn't match"));
        show_spoof_banner_row.set_subtitle(&i18n(
            "The badge beside the sender stays. A possible forgery always shows the banner.",
        ));
        {
            let sender = sender.clone();
            show_spoof_banner_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetShowSpoofBanner(row.is_active()));
            });
        }
        preferences_group_1.add(&show_spoof_banner_row);
        let gravatar_row = adw::SwitchRow::default();
        gravatar_row.set_title(&i18n("Use Gravatar when a contact has no photo"));
        gravatar_row.set_subtitle(&i18n("Sends a hash of the sender's address to Gravatar."));
        {
            let sender = sender.clone();
            gravatar_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetGravatar(row.is_active()));
            });
        }
        preferences_group_1.add(&gravatar_row);
        let sender_logos_row = adw::SwitchRow::default();
        sender_logos_row.set_title(&i18n("Show sender logos"));
        sender_logos_row.set_subtitle(&i18n(
            "Most are fetched from the sender's domain, which learns your IP address.",
        ));
        {
            let sender = sender.clone();
            sender_logos_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetSenderLogos(row.is_active()));
            });
        }
        preferences_group_1.add(&sender_logos_row);
        let receipts_row = adw::ComboRow::default();
        receipts_row.set_title(&i18n("Send read receipts"));
        receipts_row.set_subtitle(&i18n(
            "A receipt tells the sender when you opened the message.",
        ));
        receipts_row.set_model(Some(&gtk::StringList::new(&[
            &i18n("Ask each time"),
            &i18n("Always"),
            &i18n("Never"),
        ])));
        {
            let sender = sender.clone();
            receipts_row.connect_selected_notify(move |row| {
                let _ = sender.output(PrefOutput::SetReceipts(
                    crate::ui::preferences::choices::receipts_at(row.selected()),
                ));
            });
        }
        preferences_group_1.add(&receipts_row);
        page.add(&preferences_group_1);
        Self {
            page,
            auto_remote_content_row,
            remote_ahead_row,
            show_remote_banner_row,
            show_spoof_banner_row,
            gravatar_row,
            sender_logos_row,
            receipts_row,
        }
    }
}
