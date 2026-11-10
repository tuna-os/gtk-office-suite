//! Settings → General: checking for mail, notifications and their
//! sound, and the buttons a notification carries.

use adw::prelude::*;

use super::super::{PrefInput, PrefOutput, Preferences, ViewSender};
use crate::i18n::i18n;

/// The page, and the rows on it that the window reaches later.
#[derive(Debug)]
pub(in crate::ui::preferences) struct GeneralPage {
    pub(in crate::ui::preferences) page: adw::PreferencesPage,
    pub(in crate::ui::preferences) fetch_row: adw::ComboRow,
    pub(in crate::ui::preferences) push_row: adw::SwitchRow,
    pub(in crate::ui::preferences) metered_row: adw::SwitchRow,
    pub(in crate::ui::preferences) notifications_row: adw::SwitchRow,
    pub(in crate::ui::preferences) notification_content_row: adw::SwitchRow,
    pub(in crate::ui::preferences) notify_receipts_row: adw::SwitchRow,
    pub(in crate::ui::preferences) sound_row: adw::SwitchRow,
    pub(in crate::ui::preferences) sound_choice_row: adw::ComboRow,
    pub(in crate::ui::preferences) sound_file_row: adw::ActionRow,
    pub(in crate::ui::preferences) sound_focused_row: adw::SwitchRow,
    pub(in crate::ui::preferences) notify_mark_read_row: adw::SwitchRow,
    pub(in crate::ui::preferences) notify_archive_row: adw::SwitchRow,
    pub(in crate::ui::preferences) notify_delete_row: adw::SwitchRow,
    pub(in crate::ui::preferences) notify_reply_row: adw::SwitchRow,
    pub(in crate::ui::preferences) notify_forward_row: adw::SwitchRow,
    pub(in crate::ui::preferences) notify_spam_row: adw::SwitchRow,
}

impl GeneralPage {
    pub(in crate::ui::preferences) fn build(model: &Preferences, sender: &ViewSender) -> Self {
        let page = adw::PreferencesPage::default();
        let preferences_group_1 = adw::PreferencesGroup::default();
        preferences_group_1.set_title(&i18n("General"));
        let fetch_row = adw::ComboRow::default();
        fetch_row.set_title(&i18n("Check for new mail"));
        {
            let sender = sender.clone();
            fetch_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeFetchInterval(row.selected()));
            });
        }
        preferences_group_1.add(&fetch_row);
        let push_row = adw::SwitchRow::default();
        push_row.set_title(&i18n("Instant new mail (push)"));
        push_row.set_subtitle(&i18n("Microsoft 365 accounts check at the interval above."));
        {
            let sender = sender.clone();
            push_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetPush(row.is_active()));
            });
        }
        preferences_group_1.add(&push_row);
        let metered_row = adw::SwitchRow::default();
        metered_row.set_title(&i18n("Save data on metered connections"));
        metered_row.set_subtitle(&i18n(
            "New mail still arrives. Nothing else is fetched ahead.",
        ));
        {
            let sender = sender.clone();
            metered_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetMeteredSaveData(row.is_active()));
            });
        }
        preferences_group_1.add(&metered_row);
        page.add(&preferences_group_1);
        let preferences_group_2 = adw::PreferencesGroup::default();
        preferences_group_2.set_title(&i18n("Notifications"));
        let notifications_row = adw::SwitchRow::default();
        notifications_row.set_title(&i18n("Desktop notifications"));
        notifications_row.set_subtitle(&i18n(
            "For new mail and errors, while the window is in the background.",
        ));
        {
            let sender = sender.clone();
            notifications_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleNotifications(row.is_active()));
            });
        }
        preferences_group_2.add(&notifications_row);
        let notification_content_row = adw::SwitchRow::default();
        // Kept current by update_view.
        notification_content_row.set_sensitive(model.notifications);
        notification_content_row.set_title(&i18n("Show sender and subject"));
        {
            let sender = sender.clone();
            notification_content_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetNotificationContent(row.is_active()));
            });
        }
        preferences_group_2.add(&notification_content_row);
        // When a receipt comes back for mail sent (#520).
        let notify_receipts_row = adw::SwitchRow::default();
        // Kept current by update_view.
        notify_receipts_row.set_sensitive(model.notifications);
        notify_receipts_row.set_title(&i18n("Read receipts"));
        notify_receipts_row.set_subtitle(&i18n("When someone opens mail you asked a receipt for."));
        {
            let sender = sender.clone();
            notify_receipts_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetNotifyReceipts(row.is_active()));
            });
        }
        preferences_group_2.add(&notify_receipts_row);
        // A sound with each new-mail notification
        // (#292): a built-in one or a file of the
        // user's own.
        let sound_row = adw::SwitchRow::default();
        // Kept current by update_view.
        sound_row.set_sensitive(model.notifications);
        sound_row.set_title(&i18n("Sound for new mail"));
        {
            let sender = sender.clone();
            sound_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleSound(row.is_active()));
            });
        }
        preferences_group_2.add(&sound_row);
        let sound_choice_row = adw::ComboRow::default();
        // Kept current by update_view.
        sound_choice_row.set_sensitive(model.notifications && model.sound.enabled);
        sound_choice_row.set_title(&i18n("Sound"));
        let button_3 = gtk::Button::default();
        button_3.set_icon_name("media-playback-start-symbolic");
        button_3.set_tooltip_text(Some(i18n("Play").as_str()));
        button_3.set_valign(gtk::Align::Center);
        button_3.add_css_class("flat");
        {
            let sender = sender.clone();
            button_3.connect_clicked(move |_| {
                sender.input(PrefInput::PlaySound);
            });
        }
        sound_choice_row.add_suffix(&button_3);
        {
            let sender = sender.clone();
            sound_choice_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeSound(row.selected()));
            });
        }
        preferences_group_2.add(&sound_choice_row);
        let sound_file_row = adw::ActionRow::default();
        // Kept current by update_view.
        sound_file_row.set_visible(model.sound.sound == crate::config::CUSTOM_SOUND);
        // Kept current by update_view.
        sound_file_row.set_sensitive(model.notifications && model.sound.enabled);
        sound_file_row.set_title(&i18n("Sound file"));
        // Kept current by update_view.
        sound_file_row.set_subtitle(&custom_sound_name(model));
        let button_4 = gtk::Button::default();
        button_4.set_label(&i18n("Choose…"));
        button_4.set_valign(gtk::Align::Center);
        {
            let sender = sender.clone();
            button_4.connect_clicked(move |_| {
                sender.input(PrefInput::ChooseSound);
            });
        }
        sound_file_row.add_suffix(&button_4);
        preferences_group_2.add(&sound_file_row);
        // GNOME shows no notification for the app
        // in front, so by default there is no sound
        // either; this plays it anyway (#337).
        let sound_focused_row = adw::SwitchRow::default();
        // Kept current by update_view.
        sound_focused_row.set_sensitive(model.notifications && model.sound.enabled);
        sound_focused_row.set_title(&i18n("Also play while Hylki is in front"));
        {
            let sender = sender.clone();
            sound_focused_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleSoundFocused(row.is_active()));
            });
        }
        preferences_group_2.add(&sound_focused_row);
        page.add(&preferences_group_2);
        // The buttons a single-message notification
        // carries (#244); a summary of several has none.
        // Three at most: once three are on, the rest
        // grey out until one is switched off.
        let preferences_group_5 = adw::PreferencesGroup::default();
        preferences_group_5.set_title(&i18n("Notification Buttons"));
        preferences_group_5.set_description(Some(&i18n(
            "Up to three buttons on a notification about one message.",
        )));
        let notify_mark_read_row = adw::SwitchRow::default();
        // Kept current by update_view.
        notify_mark_read_row
            .set_sensitive(model.button_row_sensitive(crate::config::NotificationButton::MarkRead));
        notify_mark_read_row.set_title(&i18n("Mark as Read"));
        {
            let sender = sender.clone();
            notify_mark_read_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleNotificationButton(
                    crate::config::NotificationButton::MarkRead,
                    row.is_active(),
                ));
            });
        }
        preferences_group_5.add(&notify_mark_read_row);
        let notify_archive_row = adw::SwitchRow::default();
        // Kept current by update_view.
        notify_archive_row
            .set_sensitive(model.button_row_sensitive(crate::config::NotificationButton::Archive));
        notify_archive_row.set_title(&i18n("Archive"));
        {
            let sender = sender.clone();
            notify_archive_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleNotificationButton(
                    crate::config::NotificationButton::Archive,
                    row.is_active(),
                ));
            });
        }
        preferences_group_5.add(&notify_archive_row);
        let notify_delete_row = adw::SwitchRow::default();
        // Kept current by update_view.
        notify_delete_row
            .set_sensitive(model.button_row_sensitive(crate::config::NotificationButton::Delete));
        notify_delete_row.set_title(&i18n("Delete"));
        {
            let sender = sender.clone();
            notify_delete_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleNotificationButton(
                    crate::config::NotificationButton::Delete,
                    row.is_active(),
                ));
            });
        }
        preferences_group_5.add(&notify_delete_row);
        let notify_reply_row = adw::SwitchRow::default();
        // Kept current by update_view.
        notify_reply_row
            .set_sensitive(model.button_row_sensitive(crate::config::NotificationButton::Reply));
        notify_reply_row.set_title(&i18n("Reply"));
        {
            let sender = sender.clone();
            notify_reply_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleNotificationButton(
                    crate::config::NotificationButton::Reply,
                    row.is_active(),
                ));
            });
        }
        preferences_group_5.add(&notify_reply_row);
        let notify_forward_row = adw::SwitchRow::default();
        // Kept current by update_view.
        notify_forward_row
            .set_sensitive(model.button_row_sensitive(crate::config::NotificationButton::Forward));
        notify_forward_row.set_title(&i18n("Forward"));
        {
            let sender = sender.clone();
            notify_forward_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleNotificationButton(
                    crate::config::NotificationButton::Forward,
                    row.is_active(),
                ));
            });
        }
        preferences_group_5.add(&notify_forward_row);
        let notify_spam_row = adw::SwitchRow::default();
        // Kept current by update_view.
        notify_spam_row
            .set_sensitive(model.button_row_sensitive(crate::config::NotificationButton::Spam));
        notify_spam_row.set_title(&i18n("Mark as Spam"));
        {
            let sender = sender.clone();
            notify_spam_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleNotificationButton(
                    crate::config::NotificationButton::Spam,
                    row.is_active(),
                ));
            });
        }
        preferences_group_5.add(&notify_spam_row);
        page.add(&preferences_group_5);
        Self {
            page,
            fetch_row,
            push_row,
            metered_row,
            notifications_row,
            notification_content_row,
            notify_receipts_row,
            sound_row,
            sound_choice_row,
            sound_file_row,
            sound_focused_row,
            notify_mark_read_row,
            notify_archive_row,
            notify_delete_row,
            notify_reply_row,
            notify_forward_row,
            notify_spam_row,
        }
    }

    /// The view's `#[watch]` setters for this page, in the order the
    /// view had them (see the module comment of `pages`).
    pub(in crate::ui::preferences) fn update_view(&self, model: &Preferences) {
        self.notification_content_row
            .set_sensitive(model.notifications);
        self.notify_receipts_row.set_sensitive(model.notifications);
        self.sound_row.set_sensitive(model.notifications);
        self.sound_choice_row
            .set_sensitive(model.notifications && model.sound.enabled);
        self.sound_file_row
            .set_visible(model.sound.sound == crate::config::CUSTOM_SOUND);
        self.sound_file_row
            .set_sensitive(model.notifications && model.sound.enabled);
        self.sound_file_row.set_subtitle(&custom_sound_name(model));
        self.sound_focused_row
            .set_sensitive(model.notifications && model.sound.enabled);
        self.notify_mark_read_row
            .set_sensitive(model.button_row_sensitive(crate::config::NotificationButton::MarkRead));
        self.notify_archive_row
            .set_sensitive(model.button_row_sensitive(crate::config::NotificationButton::Archive));
        self.notify_delete_row
            .set_sensitive(model.button_row_sensitive(crate::config::NotificationButton::Delete));
        self.notify_reply_row
            .set_sensitive(model.button_row_sensitive(crate::config::NotificationButton::Reply));
        self.notify_forward_row
            .set_sensitive(model.button_row_sensitive(crate::config::NotificationButton::Forward));
        self.notify_spam_row
            .set_sensitive(model.button_row_sensitive(crate::config::NotificationButton::Spam));
    }
}

/// The Sound file row's subtitle: the custom sound's file name.
fn custom_sound_name(model: &Preferences) -> String {
    model
        .custom_sound
        .as_ref()
        .and_then(|p| p.file_name())
        .map_or_else(|| i18n("None"), |n| n.to_string_lossy().into_owned())
}
