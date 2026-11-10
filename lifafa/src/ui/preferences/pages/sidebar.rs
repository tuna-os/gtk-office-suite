//! Settings → Sidebar: the unified rows, the sections and the icon
//! rail.

use adw::prelude::*;

use super::super::{PrefInput, PrefOutput, Preferences, ViewSender};
use crate::i18n::i18n;

/// The page, and the rows on it that the window reaches later.
#[derive(Debug)]
pub(in crate::ui::preferences) struct SidebarPage {
    pub(in crate::ui::preferences) page: adw::PreferencesPage,
    pub(in crate::ui::preferences) start_view_row: adw::ComboRow,
    pub(in crate::ui::preferences) show_accounts_row: adw::SwitchRow,
    pub(in crate::ui::preferences) folder_sort_row: adw::ComboRow,
    pub(in crate::ui::preferences) chevron_side_row: adw::ComboRow,
    pub(in crate::ui::preferences) account_click_row: adw::SwitchRow,
    pub(in crate::ui::preferences) show_attachments_row: adw::SwitchRow,
    pub(in crate::ui::preferences) sidebar_hover_expand_row: adw::SwitchRow,
    pub(in crate::ui::preferences) remember_sidebar_row: adw::SwitchRow,
    pub(in crate::ui::preferences) remember_rail_row: adw::SwitchRow,
    pub(in crate::ui::preferences) seen_counts_row: adw::SwitchRow,
    pub(in crate::ui::preferences) show_unified_row: adw::SwitchRow,
    pub(in crate::ui::preferences) unified_starred_row: adw::SwitchRow,
    pub(in crate::ui::preferences) unified_sent_row: adw::SwitchRow,
    pub(in crate::ui::preferences) unified_drafts_row: adw::SwitchRow,
    pub(in crate::ui::preferences) unified_archive_row: adw::SwitchRow,
    pub(in crate::ui::preferences) unified_junk_row: adw::SwitchRow,
    pub(in crate::ui::preferences) unified_trash_row: adw::SwitchRow,
    pub(in crate::ui::preferences) unified_snoozed_row: adw::SwitchRow,
    pub(in crate::ui::preferences) unified_chips_row: adw::ExpanderRow,
    pub(in crate::ui::preferences) unified_chip_all_inboxes_row: adw::SwitchRow,
    pub(in crate::ui::preferences) unified_chip_starred_row: adw::SwitchRow,
    pub(in crate::ui::preferences) unified_chip_drafts_row: adw::SwitchRow,
    pub(in crate::ui::preferences) unified_chip_archive_row: adw::SwitchRow,
    pub(in crate::ui::preferences) unified_chip_junk_row: adw::SwitchRow,
    pub(in crate::ui::preferences) unified_chip_trash_row: adw::SwitchRow,
    pub(in crate::ui::preferences) unified_chip_snoozed_row: adw::SwitchRow,
    pub(in crate::ui::preferences) section_style_row: adw::ComboRow,
    pub(in crate::ui::preferences) filtered_placement_row: adw::ComboRow,
    pub(in crate::ui::preferences) tags_placement_row: adw::ComboRow,
    pub(in crate::ui::preferences) favorites_placement_row: adw::ComboRow,
    pub(in crate::ui::preferences) contacts_placement_row: adw::ComboRow,
    pub(in crate::ui::preferences) rail_dots_row: adw::SwitchRow,
    pub(in crate::ui::preferences) rail_fold_row: adw::ExpanderRow,
    pub(in crate::ui::preferences) rail_fold_accounts_row: adw::SwitchRow,
    pub(in crate::ui::preferences) rail_fold_all_inboxes_row: adw::SwitchRow,
    pub(in crate::ui::preferences) rail_fold_starred_row: adw::SwitchRow,
    pub(in crate::ui::preferences) rail_fold_sent_row: adw::SwitchRow,
    pub(in crate::ui::preferences) rail_fold_drafts_row: adw::SwitchRow,
    pub(in crate::ui::preferences) rail_fold_archive_row: adw::SwitchRow,
    pub(in crate::ui::preferences) rail_fold_junk_row: adw::SwitchRow,
    pub(in crate::ui::preferences) rail_fold_trash_row: adw::SwitchRow,
    pub(in crate::ui::preferences) rail_fold_snoozed_row: adw::SwitchRow,
    pub(in crate::ui::preferences) rail_fold_filtered_row: adw::SwitchRow,
    pub(in crate::ui::preferences) rail_fold_tags_row: adw::SwitchRow,
}

impl SidebarPage {
    pub(in crate::ui::preferences) fn build(model: &Preferences, sender: &ViewSender) -> Self {
        let page = adw::PreferencesPage::default();
        let preferences_group_1 = adw::PreferencesGroup::default();
        preferences_group_1.set_title(&i18n("Sidebar"));
        let start_view_row = adw::ComboRow::default();
        start_view_row.set_title(&i18n("Open at startup"));
        {
            let sender = sender.clone();
            start_view_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeStartView(row.selected()));
            });
        }
        preferences_group_1.add(&start_view_row);
        let show_accounts_row = adw::SwitchRow::default();
        // Kept current by update_view.
        show_accounts_row.set_active(model.show_accounts);
        show_accounts_row.set_title(&i18n("Accounts in the sidebar"));
        show_accounts_row.set_subtitle(&i18n(
            "Off leaves only the unified section. Toggle with Ctrl+Shift+A or the main menu.",
        ));
        {
            let sender = sender.clone();
            show_accounts_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleShowAccounts(row.is_active()));
            });
        }
        preferences_group_1.add(&show_accounts_row);
        let folder_sort_row = adw::ComboRow::default();
        folder_sort_row.set_title(&i18n("Folder order"));
        folder_sort_row.set_subtitle(&i18n("Dragging a folder switches to Custom Order."));
        {
            let sender = sender.clone();
            folder_sort_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeFolderSort(row.selected()));
            });
        }
        preferences_group_1.add(&folder_sort_row);
        let chevron_side_row = adw::ComboRow::default();
        chevron_side_row.set_title(&i18n("Chevron placement"));
        {
            let sender = sender.clone();
            chevron_side_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeChevronSide(row.selected()));
            });
        }
        preferences_group_1.add(&chevron_side_row);
        let account_click_row = adw::SwitchRow::default();
        account_click_row.set_title(&i18n("Fold accounts with one click"));
        account_click_row.set_subtitle(&i18n(
            "When off, only its chevron or a double click activates the fold.",
        ));
        {
            let sender = sender.clone();
            account_click_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetAccountClickFolds(row.is_active()));
            });
        }
        preferences_group_1.add(&account_click_row);
        let show_attachments_row = adw::SwitchRow::default();
        show_attachments_row.set_title(&i18n("Show Attachments Browser"));
        show_attachments_row
            .set_subtitle(&i18n("Browse every account's attachments in one place."));
        {
            let sender = sender.clone();
            show_attachments_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetAttachmentsRow(row.is_active()));
            });
        }
        preferences_group_1.add(&show_attachments_row);
        let sidebar_hover_expand_row = adw::SwitchRow::default();
        sidebar_hover_expand_row.set_title(&i18n("Expand the sidebar on hover"));
        sidebar_hover_expand_row
            .set_subtitle(&i18n("Hovering the icon rail floats the full sidebar out."));
        {
            let sender = sender.clone();
            sidebar_hover_expand_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetSidebarHoverExpand(row.is_active()));
            });
        }
        preferences_group_1.add(&sidebar_hover_expand_row);
        let remember_sidebar_row = adw::SwitchRow::default();
        remember_sidebar_row.set_title(&i18n("Remember the sidebar layout"));
        remember_sidebar_row.set_subtitle(&i18n("Off starts every launch folded up."));
        {
            let sender = sender.clone();
            remember_sidebar_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetRememberSidebar(row.is_active()));
            });
        }
        preferences_group_1.add(&remember_sidebar_row);
        let remember_rail_row = adw::SwitchRow::default();
        remember_rail_row.set_title(&i18n("Remember icon rail state"));
        {
            let sender = sender.clone();
            remember_rail_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetRememberRail(row.is_active()));
            });
        }
        preferences_group_1.add(&remember_rail_row);
        let seen_counts_row = adw::SwitchRow::default();
        seen_counts_row.set_title(&i18n("Highlight only new unread mail"));
        seen_counts_row.set_subtitle(&i18n(
            "A count turns grey once you have looked at the folder.",
        ));
        {
            let sender = sender.clone();
            seen_counts_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetSeenCounts(row.is_active()));
            });
        }
        preferences_group_1.add(&seen_counts_row);
        page.add(&preferences_group_1);
        let preferences_group_2 = adw::PreferencesGroup::default();
        preferences_group_2.set_title(&i18n("Unified"));
        preferences_group_2.set_description(Some(&i18n("Shown only with more than one account.")));
        let show_unified_row = adw::SwitchRow::default();
        show_unified_row.set_title(&i18n("Inboxes"));
        {
            let sender = sender.clone();
            show_unified_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleShowUnified(row.is_active()));
            });
        }
        preferences_group_2.add(&show_unified_row);
        let unified_starred_row = adw::SwitchRow::default();
        unified_starred_row.set_title(&i18n("Starred"));
        {
            let sender = sender.clone();
            unified_starred_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleUnifiedStarred(row.is_active()));
            });
        }
        preferences_group_2.add(&unified_starred_row);
        let unified_sent_row = adw::SwitchRow::default();
        unified_sent_row.set_title(&i18n("Sent"));
        {
            let sender = sender.clone();
            unified_sent_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleUnifiedSent(row.is_active()));
            });
        }
        preferences_group_2.add(&unified_sent_row);
        let unified_drafts_row = adw::SwitchRow::default();
        unified_drafts_row.set_title(&i18n("Drafts"));
        {
            let sender = sender.clone();
            unified_drafts_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleUnifiedDrafts(row.is_active()));
            });
        }
        preferences_group_2.add(&unified_drafts_row);
        let unified_archive_row = adw::SwitchRow::default();
        unified_archive_row.set_title(&i18n("Archive"));
        {
            let sender = sender.clone();
            unified_archive_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleUnifiedArchive(row.is_active()));
            });
        }
        preferences_group_2.add(&unified_archive_row);
        let unified_junk_row = adw::SwitchRow::default();
        unified_junk_row.set_title(&i18n("Junk"));
        {
            let sender = sender.clone();
            unified_junk_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleUnifiedJunk(row.is_active()));
            });
        }
        preferences_group_2.add(&unified_junk_row);
        let unified_trash_row = adw::SwitchRow::default();
        unified_trash_row.set_title(&i18n("Trash"));
        {
            let sender = sender.clone();
            unified_trash_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleUnifiedTrash(row.is_active()));
            });
        }
        preferences_group_2.add(&unified_trash_row);
        let unified_snoozed_row = adw::SwitchRow::default();
        unified_snoozed_row.set_title(&i18n("Snoozed"));
        unified_snoozed_row.set_subtitle(&i18n("Shown only while mail is snoozed."));
        {
            let sender = sender.clone();
            unified_snoozed_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleUnifiedSnoozed(row.is_active()));
            });
        }
        preferences_group_2.add(&unified_snoozed_row);
        let unified_chips_row = adw::ExpanderRow::default();
        unified_chips_row.set_title(&i18n("Unread counts"));
        unified_chips_row
            .set_subtitle(&i18n("Shown on the unified rows while they are folded up."));
        unified_chips_row.set_expanded(false);
        let unified_chip_all_inboxes_row = adw::SwitchRow::default();
        unified_chip_all_inboxes_row.set_title(&i18n("Inboxes"));
        {
            let sender = sender.clone();
            unified_chip_all_inboxes_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleUnifiedChipAllInboxes(row.is_active()));
            });
        }
        unified_chips_row.add_row(&unified_chip_all_inboxes_row);
        let unified_chip_starred_row = adw::SwitchRow::default();
        unified_chip_starred_row.set_title(&i18n("Starred"));
        {
            let sender = sender.clone();
            unified_chip_starred_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleUnifiedChipStarred(row.is_active()));
            });
        }
        unified_chips_row.add_row(&unified_chip_starred_row);
        let unified_chip_drafts_row = adw::SwitchRow::default();
        unified_chip_drafts_row.set_title(&i18n("Drafts"));
        {
            let sender = sender.clone();
            unified_chip_drafts_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleUnifiedChipDrafts(row.is_active()));
            });
        }
        unified_chips_row.add_row(&unified_chip_drafts_row);
        let unified_chip_archive_row = adw::SwitchRow::default();
        unified_chip_archive_row.set_title(&i18n("Archive"));
        {
            let sender = sender.clone();
            unified_chip_archive_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleUnifiedChipArchive(row.is_active()));
            });
        }
        unified_chips_row.add_row(&unified_chip_archive_row);
        let unified_chip_junk_row = adw::SwitchRow::default();
        unified_chip_junk_row.set_title(&i18n("Junk"));
        {
            let sender = sender.clone();
            unified_chip_junk_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleUnifiedChipJunk(row.is_active()));
            });
        }
        unified_chips_row.add_row(&unified_chip_junk_row);
        let unified_chip_trash_row = adw::SwitchRow::default();
        unified_chip_trash_row.set_title(&i18n("Trash"));
        {
            let sender = sender.clone();
            unified_chip_trash_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleUnifiedChipTrash(row.is_active()));
            });
        }
        unified_chips_row.add_row(&unified_chip_trash_row);
        let unified_chip_snoozed_row = adw::SwitchRow::default();
        unified_chip_snoozed_row.set_title(&i18n("Snoozed"));
        {
            let sender = sender.clone();
            unified_chip_snoozed_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleUnifiedChipSnoozed(row.is_active()));
            });
        }
        unified_chips_row.add_row(&unified_chip_snoozed_row);
        preferences_group_2.add(&unified_chips_row);
        page.add(&preferences_group_2);
        let preferences_group_3 = adw::PreferencesGroup::default();
        preferences_group_3.set_title(&i18n("Sections"));
        let section_style_row = adw::ComboRow::default();
        section_style_row.set_title(&i18n("Section style"));
        {
            let sender = sender.clone();
            section_style_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeSectionStyle(row.selected()));
            });
        }
        preferences_group_3.add(&section_style_row);
        let filtered_placement_row = adw::ComboRow::default();
        filtered_placement_row.set_title(&i18n("Filters placement"));
        {
            let sender = sender.clone();
            filtered_placement_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeFilteredPlacement(row.selected()));
            });
        }
        preferences_group_3.add(&filtered_placement_row);
        let tags_placement_row = adw::ComboRow::default();
        tags_placement_row.set_title(&i18n("Tags placement"));
        {
            let sender = sender.clone();
            tags_placement_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeTagsPlacement(row.selected()));
            });
        }
        preferences_group_3.add(&tags_placement_row);
        let favorites_placement_row = adw::ComboRow::default();
        favorites_placement_row.set_title(&i18n("Favorites placement"));
        {
            let sender = sender.clone();
            favorites_placement_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeFavoritesPlacement(row.selected()));
            });
        }
        preferences_group_3.add(&favorites_placement_row);
        let contacts_placement_row = adw::ComboRow::default();
        contacts_placement_row.set_title(&i18n("Contacts placement"));
        contacts_placement_row.set_subtitle(&i18n("'In the footer' disables expand/collapse."));
        {
            let sender = sender.clone();
            contacts_placement_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeContactsPlacement(row.selected()));
            });
        }
        preferences_group_3.add(&contacts_placement_row);
        page.add(&preferences_group_3);
        let preferences_group_4 = adw::PreferencesGroup::default();
        preferences_group_4.set_title(&i18n("Icon rail"));
        let rail_dots_row = adw::SwitchRow::default();
        rail_dots_row.set_title(&i18n("Unread dots instead of counts"));
        rail_dots_row.set_subtitle(&i18n("The count stays in the tooltip."));
        {
            let sender = sender.clone();
            rail_dots_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetRailDots(row.is_active()));
            });
        }
        preferences_group_4.add(&rail_dots_row);
        let rail_fold_row = adw::ExpanderRow::default();
        rail_fold_row.set_title(&i18n("Fold up expanded items"));
        rail_fold_row.set_subtitle(&i18n(
            "The items below start folded up in the rail. Long-press to open one.",
        ));
        rail_fold_row.set_show_enable_switch(true);
        rail_fold_row.set_expanded(false);
        {
            let sender = sender.clone();
            rail_fold_row.connect_enable_expansion_notify(move |row| {
                sender.input(PrefInput::ToggleRailFoldEnabled(row.enables_expansion()));
            });
        }
        let rail_fold_accounts_row = adw::SwitchRow::default();
        rail_fold_accounts_row.set_title(&i18n("All accounts"));
        {
            let sender = sender.clone();
            rail_fold_accounts_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleRailFoldAccounts(row.is_active()));
            });
        }
        rail_fold_row.add_row(&rail_fold_accounts_row);
        let rail_fold_all_inboxes_row = adw::SwitchRow::default();
        rail_fold_all_inboxes_row.set_title(&i18n("Inboxes"));
        {
            let sender = sender.clone();
            rail_fold_all_inboxes_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleRailFoldAllInboxes(row.is_active()));
            });
        }
        rail_fold_row.add_row(&rail_fold_all_inboxes_row);
        let rail_fold_starred_row = adw::SwitchRow::default();
        rail_fold_starred_row.set_title(&i18n("Starred"));
        {
            let sender = sender.clone();
            rail_fold_starred_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleRailFoldStarred(row.is_active()));
            });
        }
        rail_fold_row.add_row(&rail_fold_starred_row);
        let rail_fold_sent_row = adw::SwitchRow::default();
        rail_fold_sent_row.set_title(&i18n("Sent"));
        {
            let sender = sender.clone();
            rail_fold_sent_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleRailFoldSent(row.is_active()));
            });
        }
        rail_fold_row.add_row(&rail_fold_sent_row);
        let rail_fold_drafts_row = adw::SwitchRow::default();
        rail_fold_drafts_row.set_title(&i18n("Drafts"));
        {
            let sender = sender.clone();
            rail_fold_drafts_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleRailFoldDrafts(row.is_active()));
            });
        }
        rail_fold_row.add_row(&rail_fold_drafts_row);
        let rail_fold_archive_row = adw::SwitchRow::default();
        rail_fold_archive_row.set_title(&i18n("Archive"));
        {
            let sender = sender.clone();
            rail_fold_archive_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleRailFoldArchive(row.is_active()));
            });
        }
        rail_fold_row.add_row(&rail_fold_archive_row);
        let rail_fold_junk_row = adw::SwitchRow::default();
        rail_fold_junk_row.set_title(&i18n("Junk"));
        {
            let sender = sender.clone();
            rail_fold_junk_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleRailFoldJunk(row.is_active()));
            });
        }
        rail_fold_row.add_row(&rail_fold_junk_row);
        let rail_fold_trash_row = adw::SwitchRow::default();
        rail_fold_trash_row.set_title(&i18n("Trash"));
        {
            let sender = sender.clone();
            rail_fold_trash_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleRailFoldTrash(row.is_active()));
            });
        }
        rail_fold_row.add_row(&rail_fold_trash_row);
        let rail_fold_snoozed_row = adw::SwitchRow::default();
        rail_fold_snoozed_row.set_title(&i18n("Snoozed"));
        {
            let sender = sender.clone();
            rail_fold_snoozed_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleRailFoldSnoozed(row.is_active()));
            });
        }
        rail_fold_row.add_row(&rail_fold_snoozed_row);
        let rail_fold_filtered_row = adw::SwitchRow::default();
        rail_fold_filtered_row.set_title(&i18n("Filters"));
        {
            let sender = sender.clone();
            rail_fold_filtered_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleRailFoldFiltered(row.is_active()));
            });
        }
        rail_fold_row.add_row(&rail_fold_filtered_row);
        let rail_fold_tags_row = adw::SwitchRow::default();
        rail_fold_tags_row.set_title(&i18n("Tags"));
        {
            let sender = sender.clone();
            rail_fold_tags_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleRailFoldTags(row.is_active()));
            });
        }
        rail_fold_row.add_row(&rail_fold_tags_row);
        preferences_group_4.add(&rail_fold_row);
        page.add(&preferences_group_4);
        Self {
            page,
            start_view_row,
            show_accounts_row,
            folder_sort_row,
            chevron_side_row,
            account_click_row,
            show_attachments_row,
            sidebar_hover_expand_row,
            remember_sidebar_row,
            remember_rail_row,
            seen_counts_row,
            show_unified_row,
            unified_starred_row,
            unified_sent_row,
            unified_drafts_row,
            unified_archive_row,
            unified_junk_row,
            unified_trash_row,
            unified_snoozed_row,
            unified_chips_row,
            unified_chip_all_inboxes_row,
            unified_chip_starred_row,
            unified_chip_drafts_row,
            unified_chip_archive_row,
            unified_chip_junk_row,
            unified_chip_trash_row,
            unified_chip_snoozed_row,
            section_style_row,
            filtered_placement_row,
            tags_placement_row,
            favorites_placement_row,
            contacts_placement_row,
            rail_dots_row,
            rail_fold_row,
            rail_fold_accounts_row,
            rail_fold_all_inboxes_row,
            rail_fold_starred_row,
            rail_fold_sent_row,
            rail_fold_drafts_row,
            rail_fold_archive_row,
            rail_fold_junk_row,
            rail_fold_trash_row,
            rail_fold_snoozed_row,
            rail_fold_filtered_row,
            rail_fold_tags_row,
        }
    }

    /// The view's `#[watch]` setters for this page, in the order the
    /// view had them (see the module comment of `pages`).
    pub(in crate::ui::preferences) fn update_view(&self, model: &Preferences) {
        self.show_accounts_row.set_active(model.show_accounts);
    }
}
