//! Settings → Conversations: threading and how conversations unfold.

use adw::prelude::*;

use super::super::{PrefInput, PrefOutput, Preferences, ViewSender};
use crate::i18n::i18n;

/// The page, and the rows on it that the window reaches later.
#[derive(Debug)]
pub(in crate::ui::preferences) struct ConversationsPage {
    pub(in crate::ui::preferences) page: adw::PreferencesPage,
    pub(in crate::ui::preferences) threading_row: adw::SwitchRow,
    pub(in crate::ui::preferences) thread_expansion_row: adw::SwitchRow,
    pub(in crate::ui::preferences) threads_expanded_row: adw::SwitchRow,
    pub(in crate::ui::preferences) thread_newest_first_row: adw::SwitchRow,
    pub(in crate::ui::preferences) fold_messages_row: adw::ComboRow,
    pub(in crate::ui::preferences) thread_row_newest_row: adw::SwitchRow,
    pub(in crate::ui::preferences) reply_position_row: adw::ComboRow,
    pub(in crate::ui::preferences) card_attachments_row: adw::SwitchRow,
    pub(in crate::ui::preferences) attachment_drawer_row: adw::SwitchRow,
    pub(in crate::ui::preferences) confirm_thread_delete_row: adw::SwitchRow,
}

impl ConversationsPage {
    pub(in crate::ui::preferences) fn build(model: &Preferences, sender: &ViewSender) -> Self {
        let page = adw::PreferencesPage::default();
        let preferences_group_1 = adw::PreferencesGroup::default();
        preferences_group_1.set_title(&i18n("Conversations"));
        let threading_row = adw::SwitchRow::default();
        threading_row.set_title(&i18n("Group messages by conversation"));
        {
            let sender = sender.clone();
            threading_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleThreading(row.is_active()));
            });
        }
        preferences_group_1.add(&threading_row);
        let thread_expansion_row = adw::SwitchRow::default();
        // Kept current by update_view.
        thread_expansion_row.set_sensitive(model.threading);
        thread_expansion_row.set_title(&i18n("Expandable conversations"));
        thread_expansion_row.set_subtitle(&i18n(
            "Off, a conversation's messages show only in the reading pane.",
        ));
        {
            let sender = sender.clone();
            thread_expansion_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleThreadExpansion(row.is_active()));
            });
        }
        preferences_group_1.add(&thread_expansion_row);
        let threads_expanded_row = adw::SwitchRow::default();
        // Kept current by update_view.
        threads_expanded_row.set_sensitive(model.threading && model.thread_expansion);
        threads_expanded_row.set_title(&i18n("Expand conversations by default"));
        {
            let sender = sender.clone();
            threads_expanded_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetThreadsExpanded(row.is_active()));
            });
        }
        preferences_group_1.add(&threads_expanded_row);
        let thread_newest_first_row = adw::SwitchRow::default();
        thread_newest_first_row.set_title(&i18n("Newest message first"));
        thread_newest_first_row.set_subtitle(&i18n("In the reading pane."));
        {
            let sender = sender.clone();
            thread_newest_first_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetThreadNewestFirst(row.is_active()));
            });
        }
        preferences_group_1.add(&thread_newest_first_row);
        let fold_messages_row = adw::ComboRow::default();
        fold_messages_row.set_title(&i18n("Fold earlier messages"));
        fold_messages_row.set_subtitle(&i18n("The newest always opens."));
        {
            let sender = sender.clone();
            fold_messages_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeFoldMessages(row.selected()));
            });
        }
        preferences_group_1.add(&fold_messages_row);
        let thread_row_newest_row = adw::SwitchRow::default();
        // Kept current by update_view.
        thread_row_newest_row.set_sensitive(model.threading);
        thread_row_newest_row.set_title(&i18n("Show your own replies in the message list"));
        thread_row_newest_row.set_subtitle(&i18n("Off shows the last message that arrived."));
        {
            let sender = sender.clone();
            thread_row_newest_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetThreadRowNewest(row.is_active()));
            });
        }
        preferences_group_1.add(&thread_row_newest_row);
        let reply_position_row = adw::ComboRow::default();
        reply_position_row.set_title(&i18n("Reply editor position"));
        reply_position_row.set_subtitle(&i18n(
            "Following the reading order puts it where the conversation ends.",
        ));
        {
            let sender = sender.clone();
            reply_position_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeReplyPosition(row.selected()));
            });
        }
        preferences_group_1.add(&reply_position_row);
        let card_attachments_row = adw::SwitchRow::default();
        card_attachments_row.set_title(&i18n("Attachments on each message"));
        {
            let sender = sender.clone();
            card_attachments_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetCardAttachments(row.is_active()));
            });
        }
        preferences_group_1.add(&card_attachments_row);
        let attachment_drawer_row = adw::SwitchRow::default();
        attachment_drawer_row.set_title(&i18n("Attachment drawer"));
        {
            let sender = sender.clone();
            attachment_drawer_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetAttachmentDrawer(row.is_active()));
            });
        }
        preferences_group_1.add(&attachment_drawer_row);
        let confirm_thread_delete_row = adw::SwitchRow::default();
        confirm_thread_delete_row.set_title(&i18n("Confirm conversation deletion"));
        {
            let sender = sender.clone();
            confirm_thread_delete_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetConfirmThreadDelete(row.is_active()));
            });
        }
        preferences_group_1.add(&confirm_thread_delete_row);
        page.add(&preferences_group_1);
        Self {
            page,
            threading_row,
            thread_expansion_row,
            threads_expanded_row,
            thread_newest_first_row,
            fold_messages_row,
            thread_row_newest_row,
            reply_position_row,
            card_attachments_row,
            attachment_drawer_row,
            confirm_thread_delete_row,
        }
    }

    /// The view's `#[watch]` setters for this page, in the order the
    /// view had them (see the module comment of `pages`).
    pub(in crate::ui::preferences) fn update_view(&self, model: &Preferences) {
        self.thread_expansion_row.set_sensitive(model.threading);
        self.threads_expanded_row
            .set_sensitive(model.threading && model.thread_expansion);
        self.thread_row_newest_row.set_sensitive(model.threading);
    }
}
