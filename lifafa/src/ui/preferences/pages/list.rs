//! Settings → Message List: avatars, layout, preview lines, swipes and
//! the single-line columns.

use adw::prelude::*;

use super::super::{PrefInput, PrefOutput, Preferences, ViewSender};
use crate::i18n::i18n;

/// The page, and the rows on it that the window reaches later.
#[derive(Debug)]
pub(in crate::ui::preferences) struct ListPage {
    pub(in crate::ui::preferences) page: adw::PreferencesPage,
    pub(in crate::ui::preferences) avatars_row: adw::SwitchRow,
    pub(in crate::ui::preferences) own_mailbox_face_row: adw::SwitchRow,
    pub(in crate::ui::preferences) list_layout_row: adw::ComboRow,
    pub(in crate::ui::preferences) list_follows_window_row: adw::SwitchRow,
    pub(in crate::ui::preferences) preview_lines_row: adw::ComboRow,
    pub(in crate::ui::preferences) group_by_date_row: adw::SwitchRow,
    pub(in crate::ui::preferences) pills_below_row: adw::SwitchRow,
    pub(in crate::ui::preferences) list_search_open_row: adw::SwitchRow,
    pub(in crate::ui::preferences) list_search_place_row: adw::ComboRow,
    pub(in crate::ui::preferences) list_bulk_place_row: adw::ComboRow,
    pub(in crate::ui::preferences) tag_row_tint_row: adw::SwitchRow,
    pub(in crate::ui::preferences) swipe_enabled_row: adw::SwitchRow,
    pub(in crate::ui::preferences) swipe_reversed_row: adw::SwitchRow,
    pub(in crate::ui::preferences) swipe_sensitivity_row: adw::SpinRow,
    pub(in crate::ui::preferences) columns_group: adw::PreferencesGroup,
    pub(in crate::ui::preferences) list_headings_row: adw::SwitchRow,
    pub(in crate::ui::preferences) list_fit_row: adw::SwitchRow,
    pub(in crate::ui::preferences) column_editor_box: gtk::Box,
}

impl ListPage {
    pub(in crate::ui::preferences) fn build(model: &Preferences, sender: &ViewSender) -> Self {
        let page = adw::PreferencesPage::default();
        let preferences_group_1 = adw::PreferencesGroup::default();
        preferences_group_1.set_title(&i18n("Message List"));
        let avatars_row = adw::SwitchRow::default();
        avatars_row.set_title(&i18n("Show sender avatars"));
        {
            let sender = sender.clone();
            avatars_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleAvatars(row.is_active()));
            });
        }
        preferences_group_1.add(&avatars_row);
        // Your own mail (#189): its mailbox's
        // face, or the circle any sender gets.
        let own_mailbox_face_row = adw::SwitchRow::default();
        // Kept current by update_view.
        own_mailbox_face_row.set_sensitive(model.avatars);
        own_mailbox_face_row.set_title(&i18n("Sent mail uses your account circle"));
        own_mailbox_face_row
            .set_subtitle(&i18n("The account's picture or emoji, as in the sidebar."));
        {
            let sender = sender.clone();
            own_mailbox_face_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetOwnMailboxFace(row.is_active()));
            });
        }
        preferences_group_1.add(&own_mailbox_face_row);
        let list_layout_row = adw::ComboRow::default();
        list_layout_row.set_title(&i18n("Layout"));
        list_layout_row.set_subtitle(&i18n("Automatic uses single lines in wide windows."));
        {
            let sender = sender.clone();
            list_layout_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeListLayout(row.selected()));
            });
        }
        preferences_group_1.add(&list_layout_row);
        // The list's width, or its share of the window, as the window
        // resizes (upstream #648).
        let list_follows_window_row = adw::SwitchRow::default();
        list_follows_window_row.set_title(&i18n("Resize the list with the window"));
        list_follows_window_row.set_subtitle(&i18n("Off, it keeps its width and the reader resizes."));
        {
            let sender = sender.clone();
            list_follows_window_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetListFollowsWindow(row.is_active()));
            });
        }
        preferences_group_1.add(&list_follows_window_row);
        let preview_lines_row = adw::ComboRow::default();
        preview_lines_row.set_title(&i18n("Preview lines"));
        preview_lines_row.set_subtitle(&i18n("Off also stops downloading previews."));
        {
            let sender = sender.clone();
            preview_lines_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangePreviewLines(row.selected()));
            });
        }
        preferences_group_1.add(&preview_lines_row);
        let group_by_date_row = adw::SwitchRow::default();
        group_by_date_row.set_title(&i18n("Group by date"));
        {
            let sender = sender.clone();
            group_by_date_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetGroupByDate(row.is_active()));
            });
        }
        preferences_group_1.add(&group_by_date_row);
        let pills_below_row = adw::SwitchRow::default();
        pills_below_row.set_title(&i18n("Tags and labels under the text"));
        pills_below_row.set_subtitle(&i18n("Gives the subject its full width."));
        {
            let sender = sender.clone();
            pills_below_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetPillsBelow(row.is_active()));
            });
        }
        preferences_group_1.add(&pills_below_row);
        let list_search_open_row = adw::SwitchRow::default();
        list_search_open_row.set_title(&i18n("Keep the search field open"));
        {
            let sender = sender.clone();
            list_search_open_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetListSearchOpen(row.is_active()));
            });
        }
        preferences_group_1.add(&list_search_open_row);
        let list_search_place_row = adw::ComboRow::default();
        list_search_place_row.set_title(&i18n("Search field position"));
        {
            let sender = sender.clone();
            list_search_place_row.connect_selected_notify(move |row| {
                let _ = sender.output(PrefOutput::SetListSearchBottom(row.selected() == 1));
            });
        }
        preferences_group_1.add(&list_search_place_row);
        let list_bulk_place_row = adw::ComboRow::default();
        list_bulk_place_row.set_title(&i18n("Selection bar position"));
        {
            let sender = sender.clone();
            list_bulk_place_row.connect_selected_notify(move |row| {
                let _ = sender.output(PrefOutput::SetListBulkBarBottom(row.selected() == 1));
            });
        }
        preferences_group_1.add(&list_bulk_place_row);
        let tag_row_tint_row = adw::SwitchRow::default();
        tag_row_tint_row.set_title(&i18n("Tint tagged messages"));
        tag_row_tint_row.set_subtitle(&i18n("In the color of its first tag."));
        {
            let sender = sender.clone();
            tag_row_tint_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetTagRowTint(row.is_active()));
            });
        }
        preferences_group_1.add(&tag_row_tint_row);
        let swipe_enabled_row = adw::SwitchRow::default();
        swipe_enabled_row.set_title(&i18n("Swipe actions"));
        swipe_enabled_row.set_subtitle(&i18n(
            "Left deletes, right archives. Drag with the mouse or swipe with two fingers.",
        ));
        {
            let sender = sender.clone();
            swipe_enabled_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleSwipeEnabled(row.is_active()));
            });
        }
        preferences_group_1.add(&swipe_enabled_row);
        let swipe_reversed_row = adw::SwitchRow::default();
        // Kept current by update_view.
        swipe_reversed_row.set_sensitive(model.swipe_enabled);
        swipe_reversed_row.set_title(&i18n("Reverse swipe directions"));
        {
            let sender = sender.clone();
            swipe_reversed_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetSwipeReversed(row.is_active()));
            });
        }
        preferences_group_1.add(&swipe_reversed_row);
        let swipe_sensitivity_row = adw::SpinRow::default();
        // Kept current by update_view.
        swipe_sensitivity_row.set_sensitive(model.swipe_enabled);
        swipe_sensitivity_row.set_title(&i18n("Trackpad swipe sensitivity"));
        swipe_sensitivity_row.set_subtitle(&i18n(
            "Raise it if swipes stop short. Mouse drags are unaffected.",
        ));
        {
            let sender = sender.clone();
            swipe_sensitivity_row.connect_value_notify(move |row| {
                let _ = sender.output(PrefOutput::SetSwipeSensitivity(row.value()));
            });
        }
        preferences_group_1.add(&swipe_sensitivity_row);
        page.add(&preferences_group_1);
        // The single-line list's columns (#334): two
        // drop zones of draggable chips, filled in init.
        let columns_group = adw::PreferencesGroup::default();
        columns_group.set_title(&i18n("Columns"));
        columns_group.set_description(Some(
                        &i18n("For the single-line layout. Drag columns to reorder them or out to remove them. Due Date shows only for Microsoft 365 mail."),
                    ));
        // Kept current by update_view.
        columns_group.set_sensitive(model.list_single_possible);
        let list_headings_row = adw::SwitchRow::default();
        list_headings_row.set_title(&i18n("Show column headings"));
        {
            let sender = sender.clone();
            list_headings_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetListHeadings(row.is_active()));
            });
        }
        columns_group.add(&list_headings_row);
        let list_fit_row = adw::SwitchRow::default();
        list_fit_row.set_title(&i18n("Fit columns to the list"));
        list_fit_row.set_subtitle(&i18n("Off, a narrow list scrolls sideways."));
        {
            let sender = sender.clone();
            list_fit_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetListColumnScroll(!row.is_active()));
            });
        }
        columns_group.add(&list_fit_row);
        let column_editor_box = gtk::Box::default();
        column_editor_box.set_orientation(gtk::Orientation::Vertical);
        column_editor_box.set_spacing(12);
        // Clear of the switch's card above.
        column_editor_box.set_margin_top(12);
        let button_2 = gtk::Button::default();
        button_2.set_label(&i18n("Restore Defaults"));
        button_2.set_halign(gtk::Align::End);
        button_2.set_valign(gtk::Align::Center);
        {
            let sender = sender.clone();
            button_2.connect_clicked(move |_| sender.input(PrefInput::ColumnRestore));
        }
        column_editor_box.append(&button_2);
        columns_group.add(&column_editor_box);
        page.add(&columns_group);
        Self {
            page,
            avatars_row,
            own_mailbox_face_row,
            list_layout_row,
            list_follows_window_row,
            preview_lines_row,
            group_by_date_row,
            pills_below_row,
            list_search_open_row,
            list_search_place_row,
            list_bulk_place_row,
            tag_row_tint_row,
            swipe_enabled_row,
            swipe_reversed_row,
            swipe_sensitivity_row,
            columns_group,
            list_headings_row,
            list_fit_row,
            column_editor_box,
        }
    }

    /// The view's `#[watch]` setters for this page, in the order the
    /// view had them (see the module comment of `pages`).
    pub(in crate::ui::preferences) fn update_view(&self, model: &Preferences) {
        self.own_mailbox_face_row.set_sensitive(model.avatars);
        self.swipe_reversed_row.set_sensitive(model.swipe_enabled);
        self.swipe_sensitivity_row
            .set_sensitive(model.swipe_enabled);
        self.columns_group.set_sensitive(model.list_single_possible);
    }
}
