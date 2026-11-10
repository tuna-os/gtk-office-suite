//! Settings → Appearance: style, text size, the theme and app-icon
//! galleries, the reader toolbar and Focus Mode.

use adw::prelude::*;

use super::super::{PrefInput, PrefOutput, Preferences, ViewSender};
use crate::i18n::i18n;

/// The page, and the rows on it that the window reaches later.
#[derive(Debug)]
pub(in crate::ui::preferences) struct AppearancePage {
    pub(in crate::ui::preferences) page: adw::PreferencesPage,
    pub(in crate::ui::preferences) app_theme_row: adw::ComboRow,
    pub(in crate::ui::preferences) text_scale_row: adw::ComboRow,
    pub(in crate::ui::preferences) theme_row: adw::PreferencesRow,
    pub(in crate::ui::preferences) app_icon_row: adw::PreferencesRow,
    pub(in crate::ui::preferences) desktop_icons_row: adw::SwitchRow,
    pub(in crate::ui::preferences) settings_open_row: adw::ComboRow,
    pub(in crate::ui::preferences) toolbar_editor_box: gtk::Box,
    pub(in crate::ui::preferences) toolbar_fold_list: gtk::ListBox,
    pub(in crate::ui::preferences) toolbar_fold_row: adw::SwitchRow,
    pub(in crate::ui::preferences) focus_row: adw::SwitchRow,
    pub(in crate::ui::preferences) focus_start_row: adw::SwitchRow,
    pub(in crate::ui::preferences) focus_toolbar_row: adw::SwitchRow,
    pub(in crate::ui::preferences) focus_list_header_row: adw::SwitchRow,
    pub(in crate::ui::preferences) focus_accounts_row: adw::SwitchRow,
    pub(in crate::ui::preferences) focus_unified_row: adw::SwitchRow,
    pub(in crate::ui::preferences) focus_avatars_row: adw::SwitchRow,
    pub(in crate::ui::preferences) focus_hide_preview_row: adw::SwitchRow,
    pub(in crate::ui::preferences) focus_preview_row: adw::SwitchRow,
    pub(in crate::ui::preferences) focus_subject_row: adw::SwitchRow,
    pub(in crate::ui::preferences) focus_rail_row: adw::SwitchRow,
    pub(in crate::ui::preferences) focus_reader_row: adw::SwitchRow,
}

impl AppearancePage {
    pub(in crate::ui::preferences) fn build(model: &Preferences, sender: &ViewSender) -> Self {
        let page = adw::PreferencesPage::default();
        let preferences_group_1 = adw::PreferencesGroup::default();
        // Rendered as Pango markup — a bare "&" breaks it.
        preferences_group_1.set_title(&i18n("Appearance"));
        let app_theme_row = adw::ComboRow::default();
        app_theme_row.set_title(&i18n("Style"));
        app_theme_row.set_subtitle(&i18n("Messages have their own setting under Reading."));
        {
            let sender = sender.clone();
            app_theme_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeAppTheme(row.selected()));
            });
        }
        preferences_group_1.add(&app_theme_row);
        let text_scale_row = adw::ComboRow::default();
        text_scale_row.set_title(&i18n("Text size"));
        text_scale_row.set_subtitle(&i18n("Adjusts the interface and message text size."));
        {
            let sender = sender.clone();
            text_scale_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeTextScale(row.selected()));
            });
        }
        preferences_group_1.add(&text_scale_row);
        // The theme gallery; its content is built
        // in init (a grid the view! macro can't
        // declare), like the app-icon strip below.
        let theme_row = adw::PreferencesRow::default();
        theme_row.set_title(&i18n("Theme"));
        theme_row.set_activatable(false);
        theme_row.set_focusable(false);
        preferences_group_1.add(&theme_row);
        // The app-icon gallery; its content is built in init
        // (a grid of textures the view! macro can't declare).
        let app_icon_row = adw::PreferencesRow::default();
        app_icon_row.set_title(&i18n("App icon"));
        app_icon_row.set_activatable(false);
        app_icon_row.set_focusable(false);
        preferences_group_1.add(&app_icon_row);
        let desktop_icons_row = adw::SwitchRow::default();
        desktop_icons_row.set_title(&i18n("Use your desktop environment's icon set"));
        {
            let sender = sender.clone();
            desktop_icons_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetDesktopIcons(row.is_active()));
            });
        }
        preferences_group_1.add(&desktop_icons_row);
        let settings_open_row = adw::ComboRow::default();
        settings_open_row.set_title(&i18n("This window opens to"));
        {
            let sender = sender.clone();
            settings_open_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeSettingsOpen(row.selected()));
            });
        }
        preferences_group_1.add(&settings_open_row);
        page.add(&preferences_group_1);
        // The reader toolbar's buttons: three drop zones
        // (left group, right group, not shown) of
        // draggable chips, filled in init from the saved
        // layout. Every drop is applied and saved at once.
        let preferences_group_2 = adw::PreferencesGroup::default();
        preferences_group_2.set_title(&i18n("Toolbar"));
        preferences_group_2.set_description(Some(
                        &i18n("Drag buttons between the groups. The right group folds into ⋯ when the pane is narrow."),
                    ));
        let toolbar_editor_box = gtk::Box::default();
        toolbar_editor_box.set_orientation(gtk::Orientation::Vertical);
        toolbar_editor_box.set_spacing(12);
        let button_3 = gtk::Button::default();
        button_3.set_label(&i18n("Restore Defaults"));
        button_3.set_halign(gtk::Align::End);
        button_3.set_valign(gtk::Align::Center);
        {
            let sender = sender.clone();
            button_3.connect_clicked(move |_| sender.input(PrefInput::ToolbarRestore));
        }
        toolbar_editor_box.append(&button_3);
        // Moved under the zones once they are built.
        let toolbar_fold_list = gtk::ListBox::default();
        toolbar_fold_list.add_css_class("boxed-list");
        toolbar_fold_list.set_selection_mode(gtk::SelectionMode::None);
        let toolbar_fold_row = adw::SwitchRow::default();
        toolbar_fold_row.set_title(&i18n("Always fold the right group into ⋯"));
        {
            let sender = sender.clone();
            toolbar_fold_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetToolbarFoldRight(row.is_active()));
            });
        }
        toolbar_fold_list.append(&toolbar_fold_row);
        toolbar_editor_box.append(&toolbar_fold_list);
        preferences_group_2.add(&toolbar_editor_box);
        page.add(&preferences_group_2);
        // Focus Mode: the master switch and its parts.
        // Every row applies at once, animated.
        let preferences_group_4 = adw::PreferencesGroup::default();
        preferences_group_4.set_title(&i18n("Focus Mode"));
        preferences_group_4.set_description(Some(
                        &i18n("The parts switched on below hide while Focus Mode is on. Activated by Ctrl+Shift+F or the main menu."),
                    ));
        let focus_row = adw::SwitchRow::default();
        // Kept current by update_view.
        focus_row.set_active(model.focus.enabled);
        focus_row.set_title(&i18n("Focus Mode"));
        {
            let sender = sender.clone();
            focus_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleFocus(
                    crate::config::FocusPart::Enabled,
                    row.is_active(),
                ));
            });
        }
        preferences_group_4.add(&focus_row);
        let focus_start_row = adw::SwitchRow::default();
        // Kept current by update_view.
        focus_start_row.set_active(model.focus.start_focused);
        focus_start_row.set_title(&i18n("Start in Focus Mode"));
        {
            let sender = sender.clone();
            focus_start_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleFocus(
                    crate::config::FocusPart::StartFocused,
                    row.is_active(),
                ));
            });
        }
        preferences_group_4.add(&focus_start_row);
        let focus_toolbar_row = adw::SwitchRow::default();
        // Kept current by update_view.
        focus_toolbar_row.set_active(model.focus.reader_toolbar);
        focus_toolbar_row.set_title(&i18n("Fold the reading pane's toolbar"));
        focus_toolbar_row.set_subtitle(&i18n("Every button goes into its ⋯ menu."));
        {
            let sender = sender.clone();
            focus_toolbar_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleFocus(
                    crate::config::FocusPart::ReaderToolbar,
                    row.is_active(),
                ));
            });
        }
        preferences_group_4.add(&focus_toolbar_row);
        let focus_list_header_row = adw::SwitchRow::default();
        // Kept current by update_view.
        focus_list_header_row.set_active(model.focus.list_header);
        focus_list_header_row.set_title(&i18n("Fold the message list's header"));
        focus_list_header_row.set_subtitle(&i18n("Everything but the sidebar toggle goes into ⋯."));
        {
            let sender = sender.clone();
            focus_list_header_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleFocus(
                    crate::config::FocusPart::ListHeader,
                    row.is_active(),
                ));
            });
        }
        preferences_group_4.add(&focus_list_header_row);
        let focus_accounts_row = adw::SwitchRow::default();
        // Kept current by update_view.
        focus_accounts_row.set_active(model.focus.hide_accounts);
        focus_accounts_row.set_title(&i18n("Hide the accounts in the sidebar"));
        focus_accounts_row.set_subtitle(&i18n("Only the unified section stays."));
        {
            let sender = sender.clone();
            focus_accounts_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleFocus(
                    crate::config::FocusPart::HideAccounts,
                    row.is_active(),
                ));
            });
        }
        preferences_group_4.add(&focus_accounts_row);
        let focus_unified_row = adw::SwitchRow::default();
        // Kept current by update_view.
        focus_unified_row.set_active(model.focus.fold_unified);
        focus_unified_row.set_title(&i18n("Fold up the unified rows"));
        {
            let sender = sender.clone();
            focus_unified_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleFocus(
                    crate::config::FocusPart::FoldUnified,
                    row.is_active(),
                ));
            });
        }
        preferences_group_4.add(&focus_unified_row);
        let focus_avatars_row = adw::SwitchRow::default();
        // Kept current by update_view.
        focus_avatars_row.set_active(model.focus.hide_avatars);
        focus_avatars_row.set_title(&i18n("Hide sender avatars"));
        {
            let sender = sender.clone();
            focus_avatars_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleFocus(
                    crate::config::FocusPart::HideAvatars,
                    row.is_active(),
                ));
            });
        }
        preferences_group_4.add(&focus_avatars_row);
        let focus_hide_preview_row = adw::SwitchRow::default();
        // Kept current by update_view.
        focus_hide_preview_row.set_active(model.focus.hide_preview);
        focus_hide_preview_row.set_title(&i18n("Hide the preview text"));
        {
            let sender = sender.clone();
            focus_hide_preview_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleFocus(
                    crate::config::FocusPart::HidePreview,
                    row.is_active(),
                ));
            });
        }
        preferences_group_4.add(&focus_hide_preview_row);
        let focus_preview_row = adw::SwitchRow::default();
        // Kept current by update_view.
        focus_preview_row.set_active(model.focus.one_preview_line);
        // Nothing left to cap once the preview
        // text is hidden altogether.
        // Kept current by update_view.
        focus_preview_row.set_sensitive(!model.focus.hide_preview);
        focus_preview_row.set_title(&i18n("One line of preview text"));
        {
            let sender = sender.clone();
            focus_preview_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleFocus(
                    crate::config::FocusPart::OnePreviewLine,
                    row.is_active(),
                ));
            });
        }
        preferences_group_4.add(&focus_preview_row);
        let focus_subject_row = adw::SwitchRow::default();
        // Kept current by update_view.
        focus_subject_row.set_active(model.focus.hide_subject);
        focus_subject_row.set_title(&i18n("Hide the subject"));
        focus_subject_row.set_subtitle(&i18n("Tag chips hide with it."));
        {
            let sender = sender.clone();
            focus_subject_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleFocus(
                    crate::config::FocusPart::HideSubject,
                    row.is_active(),
                ));
            });
        }
        preferences_group_4.add(&focus_subject_row);
        let focus_rail_row = adw::SwitchRow::default();
        // Kept current by update_view.
        focus_rail_row.set_active(model.focus.rail_sidebar);
        focus_rail_row.set_title(&i18n("Fold the sidebar to the icon rail"));
        focus_rail_row.set_subtitle(&i18n("Hovering it slides the full sidebar out."));
        {
            let sender = sender.clone();
            focus_rail_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleFocus(
                    crate::config::FocusPart::RailSidebar,
                    row.is_active(),
                ));
            });
        }
        preferences_group_4.add(&focus_rail_row);
        let focus_reader_row = adw::SwitchRow::default();
        // Kept current by update_view.
        focus_reader_row.set_active(model.focus.reader_view);
        focus_reader_row.set_title(&i18n("Reader View"));
        focus_reader_row.set_subtitle(&i18n("Open messages in Reader View."));
        {
            let sender = sender.clone();
            focus_reader_row.connect_active_notify(move |row| {
                sender.input(PrefInput::ToggleFocus(
                    crate::config::FocusPart::ReaderView,
                    row.is_active(),
                ));
            });
        }
        preferences_group_4.add(&focus_reader_row);
        page.add(&preferences_group_4);
        Self {
            page,
            app_theme_row,
            text_scale_row,
            theme_row,
            app_icon_row,
            desktop_icons_row,
            settings_open_row,
            toolbar_editor_box,
            toolbar_fold_list,
            toolbar_fold_row,
            focus_row,
            focus_start_row,
            focus_toolbar_row,
            focus_list_header_row,
            focus_accounts_row,
            focus_unified_row,
            focus_avatars_row,
            focus_hide_preview_row,
            focus_preview_row,
            focus_subject_row,
            focus_rail_row,
            focus_reader_row,
        }
    }

    /// The view's `#[watch]` setters for this page, in the order the
    /// view had them (see the module comment of `pages`).
    pub(in crate::ui::preferences) fn update_view(&self, model: &Preferences) {
        self.focus_row.set_active(model.focus.enabled);
        self.focus_start_row.set_active(model.focus.start_focused);
        self.focus_toolbar_row
            .set_active(model.focus.reader_toolbar);
        self.focus_list_header_row
            .set_active(model.focus.list_header);
        self.focus_accounts_row
            .set_active(model.focus.hide_accounts);
        self.focus_unified_row.set_active(model.focus.fold_unified);
        self.focus_avatars_row.set_active(model.focus.hide_avatars);
        self.focus_hide_preview_row
            .set_active(model.focus.hide_preview);
        self.focus_preview_row
            .set_active(model.focus.one_preview_line);
        self.focus_preview_row
            .set_sensitive(!model.focus.hide_preview);
        self.focus_subject_row.set_active(model.focus.hide_subject);
        self.focus_rail_row.set_active(model.focus.rail_sidebar);
        self.focus_reader_row.set_active(model.focus.reader_view);
    }
}
