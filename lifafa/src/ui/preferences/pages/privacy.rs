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
    pub(in crate::ui::preferences) clean_links_row: adw::SwitchRow,
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
        {
            let sender = sender.clone();
            auto_remote_content_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetAutoRemoteContent(row.is_active()));
            });
        }
        preferences_group_1.add(&auto_remote_content_row);
        let remote_ahead_row = adw::SwitchRow::default();
        remote_ahead_row.set_title(&i18n("Pre-load remote content for the next messages"));
        {
            let sender = sender.clone();
            remote_ahead_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetRemoteAhead(row.is_active()));
            });
        }
        preferences_group_1.add(&remote_ahead_row);
        let clean_links_row = adw::SwitchRow::default();
        clean_links_row.set_title(&i18n("Remove trackers from links"));
        {
            let sender = sender.clone();
            clean_links_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetCleanLinks(row.is_active()));
            });
        }
        preferences_group_1.add(&clean_links_row);
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
        {
            let sender = sender.clone();
            sender_logos_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetSenderLogos(row.is_active()));
            });
        }
        preferences_group_1.add(&sender_logos_row);
        let receipts_row = adw::ComboRow::default();
        receipts_row.set_title(&i18n("Send read receipts"));
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
        // Not "On This Computer": that names an account kind.
        let stored_mail_group = adw::PreferencesGroup::default();
        stored_mail_group.set_title(&i18n("Stored Mail"));
        let encrypt_row = adw::SwitchRow::default();
        encrypt_row.set_title(&i18n("Encrypt stored mail"));
        encrypt_row.set_active(crate::config::cache_encrypted());
        encrypt_row.connect_active_notify(|row| {
            if !row.is_active() {
                crate::config::save_cache_encrypted(false);
            } else if !crate::config::cache_encrypted() {
                confirm_encrypted(row);
            }
        });
        stored_mail_group.add(&encrypt_row);
        page.add(&stored_mail_group);
        Self {
            page,
            auto_remote_content_row,
            remote_ahead_row,
            clean_links_row,
            show_remote_banner_row,
            show_spoof_banner_row,
            gravatar_row,
            sender_logos_row,
            receipts_row,
        }
    }
}

/// Settings → Privacy → Encrypt stored mail, switched on. Asked about
/// first: the first start after it converts the whole cache, and an earlier
/// version, one a beta tester goes back to included, cannot open what it
/// leaves. The cache itself is converted at the next start, when nothing
/// has it open.
fn confirm_encrypted(row: &adw::SwitchRow) {
    let dialog = adw::AlertDialog::new(
        Some(&i18n("Encrypt Stored Mail?")),
        Some(&i18n("Mail stored on this computer will be encrypted at the next start which can take a few minutes. Earlier versions of the app cannot open it. Turn this setting off and restart before going back to an older version.")),
    );
    dialog.add_response("cancel", &i18n("Cancel"));
    dialog.add_response("encrypt", &i18n("Encrypt"));
    dialog.set_response_appearance("encrypt", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let switch = row.clone();
    dialog.connect_response(None, move |_, response| {
        if response == "encrypt" {
            crate::config::save_cache_encrypted(true);
        } else {
            switch.set_active(false);
        }
    });
    dialog.present(Some(row));
}
