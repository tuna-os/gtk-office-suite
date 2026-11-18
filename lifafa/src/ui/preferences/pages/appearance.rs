//! Settings → Appearance: style, text size, the theme and app-icon
//! galleries and the reader toolbar.

use adw::prelude::*;

use super::super::{PrefInput, PrefOutput, ViewSender};
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
}

impl AppearancePage {
    pub(in crate::ui::preferences) fn build(sender: &ViewSender) -> Self {
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
        }
    }
}
