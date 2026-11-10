//! Settings → System: tray and launcher, storage, the GNOME Files
//! extension and links.

use adw::prelude::*;

use super::super::system::{link_group_description, nautilus_status_text, storage_text};
use super::super::{PrefInput, PrefOutput, Preferences, ViewSender};
use crate::i18n::i18n;

/// The page, and the rows on it that the window reaches later.
#[derive(Debug)]
pub(in crate::ui::preferences) struct SystemPage {
    pub(in crate::ui::preferences) page: adw::PreferencesPage,
    pub(in crate::ui::preferences) language_row: adw::ComboRow,
    pub(in crate::ui::preferences) renderer_row: adw::ComboRow,
    pub(in crate::ui::preferences) background_row: adw::SwitchRow,
    pub(in crate::ui::preferences) autostart_row: adw::SwitchRow,
    pub(in crate::ui::preferences) fullscreen_row: adw::SwitchRow,
    pub(in crate::ui::preferences) tray_row: adw::SwitchRow,
    pub(in crate::ui::preferences) tray_icon_row: adw::ComboRow,
    pub(in crate::ui::preferences) tray_mail_row: adw::SwitchRow,
    pub(in crate::ui::preferences) launcher_badge_row: adw::ComboRow,
    pub(in crate::ui::preferences) single_key_row: adw::SwitchRow,
    pub(in crate::ui::preferences) gmail_keys_row: adw::SwitchRow,
    pub(in crate::ui::preferences) status_bar_row: adw::ComboRow,
    pub(in crate::ui::preferences) console_mode_row: adw::SwitchRow,
    pub(in crate::ui::preferences) storage_row: adw::ActionRow,
    pub(in crate::ui::preferences) storage_remove_btn: gtk::Button,
    pub(in crate::ui::preferences) keep_downloads_row: adw::ComboRow,
    pub(in crate::ui::preferences) nautilus_row: adw::ActionRow,
    pub(in crate::ui::preferences) nautilus_remove_btn: gtk::Button,
    pub(in crate::ui::preferences) nautilus_install_btn: gtk::Button,
    pub(in crate::ui::preferences) nautilus_cmd_row: adw::EntryRow,
    pub(in crate::ui::preferences) files_action_row: adw::ComboRow,
    pub(in crate::ui::preferences) files_large_row: adw::ComboRow,
    pub(in crate::ui::preferences) files_limit_row: adw::SpinRow,
    pub(in crate::ui::preferences) link_browser_row: adw::ComboRow,
}

impl SystemPage {
    pub(in crate::ui::preferences) fn build(model: &Preferences, sender: &ViewSender) -> Self {
        let page = adw::PreferencesPage::default();
        let preferences_group_1 = adw::PreferencesGroup::default();
        preferences_group_1.set_title(&i18n("System"));
        let language_row = adw::ComboRow::default();
        language_row.set_title(&i18n("Language"));
        language_row.set_subtitle(&i18n("Applies after a restart."));
        {
            let sender = sender.clone();
            language_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeLanguage(row.selected()));
            });
        }
        preferences_group_1.add(&language_row);
        let renderer_row = adw::ComboRow::default();
        renderer_row.set_title(&i18n("Renderer"));
        renderer_row.set_subtitle(&i18n(
            "Try OpenGL or Software on older graphics. Applies after a restart.",
        ));
        {
            let sender = sender.clone();
            renderer_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeRenderer(row.selected()));
            });
        }
        preferences_group_1.add(&renderer_row);
        let background_row = adw::SwitchRow::default();
        background_row.set_title(&i18n("Keep running in the background"));
        background_row.set_subtitle(&i18n(
            "Closing the window hides it. Quit from Background Apps.",
        ));
        {
            let sender = sender.clone();
            background_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetRunInBackground(row.is_active()));
            });
        }
        preferences_group_1.add(&background_row);
        let autostart_row = adw::SwitchRow::default();
        autostart_row.set_title(&i18n("Start at login"));
        autostart_row.set_subtitle(&i18n("Starts hidden."));
        {
            let sender = sender.clone();
            autostart_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetAutostart(row.is_active()));
            });
        }
        preferences_group_1.add(&autostart_row);
        let fullscreen_row = adw::SwitchRow::default();
        fullscreen_row.set_title(&i18n("Start fullscreen"));
        fullscreen_row.set_subtitle(&i18n("F11 switches fullscreen on and off."));
        {
            let sender = sender.clone();
            fullscreen_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetStartFullscreen(row.is_active()));
            });
        }
        preferences_group_1.add(&fullscreen_row);
        let tray_row = adw::SwitchRow::default();
        tray_row.set_title(&i18n("Show a tray icon"));
        tray_row.set_subtitle(&i18n("To use on GNOME, install an AppIndicator extension."));
        {
            let sender = sender.clone();
            tray_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetTray(row.is_active()));
            });
        }
        preferences_group_1.add(&tray_row);
        let tray_icon_row = adw::ComboRow::default();
        tray_icon_row.set_title(&i18n("Tray icon"));
        {
            let sender = sender.clone();
            tray_icon_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeTrayIcon(row.selected()));
            });
        }
        preferences_group_1.add(&tray_icon_row);
        let tray_mail_row = adw::SwitchRow::default();
        tray_mail_row.set_title(&i18n("Unread mail in the tray menu"));
        {
            let sender = sender.clone();
            tray_mail_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetTrayMail(row.is_active()));
            });
        }
        preferences_group_1.add(&tray_mail_row);
        let launcher_badge_row = adw::ComboRow::default();
        launcher_badge_row.set_title(&i18n("Unread count on the app icon"));
        launcher_badge_row.set_subtitle(&i18n(
            "To use on GNOME, install a dock extension such as Dash to Dock.",
        ));
        {
            let sender = sender.clone();
            launcher_badge_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeLauncherBadge(row.selected()));
            });
        }
        preferences_group_1.add(&launcher_badge_row);
        let single_key_row = adw::SwitchRow::default();
        single_key_row.set_title(&i18n("Single-key shortcuts"));
        single_key_row.set_subtitle(&i18n("Press Ctrl+? to list all shortcuts."));
        {
            let sender = sender.clone();
            single_key_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetSingleKey(row.is_active()));
            });
        }
        preferences_group_1.add(&single_key_row);
        let gmail_keys_row = adw::SwitchRow::default();
        gmail_keys_row.set_title(&i18n("Gmail keys"));
        gmail_keys_row.set_subtitle(&i18n(
            "Swaps the single-key shortcuts for Gmail equivalents.",
        ));
        {
            let sender = sender.clone();
            gmail_keys_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetGmailKeys(row.is_active()));
            });
        }
        preferences_group_1.add(&gmail_keys_row);
        let status_bar_row = adw::ComboRow::default();
        status_bar_row.set_title(&i18n("Status bar"));
        {
            let sender = sender.clone();
            status_bar_row.connect_selected_notify(move |row| {
                let _ = sender.output(PrefOutput::SetStatusBarTop(row.selected() == 1));
            });
        }
        preferences_group_1.add(&status_bar_row);
        let console_mode_row = adw::SwitchRow::default();
        console_mode_row.set_title(&i18n("Console mode"));
        console_mode_row.set_subtitle(&i18n("Shows a console in the status bar."));
        {
            let sender = sender.clone();
            console_mode_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetConsoleMode(row.is_active()));
            });
        }
        preferences_group_1.add(&console_mode_row);
        let action_row_2 = adw::ActionRow::default();
        action_row_2.set_title(&i18n("Export log"));
        action_row_2.set_subtitle(&i18n(
            "For bug reports. Addresses are cut to their domain for privacy.",
        ));
        action_row_2.set_activatable(true);
        {
            let sender = sender.clone();
            action_row_2.connect_activated(move |_| sender.input(PrefInput::ExportLog));
        }
        let image_3 = gtk::Image::default();
        image_3.set_icon_name(Some("go-next-symbolic"));
        action_row_2.add_suffix(&image_3);
        preferences_group_1.add(&action_row_2);
        page.add(&preferences_group_1);
        // What downloaded mail takes up, and how long it
        // stays (#394).
        let preferences_group_4 = adw::PreferencesGroup::default();
        preferences_group_4.set_title(&i18n("Storage"));
        let storage_row = adw::ActionRow::default();
        storage_row.set_title(&i18n("Downloaded mail"));
        // Kept current by update_view.
        storage_row.set_subtitle(&storage_text(model.storage));
        let storage_remove_btn = gtk::Button::default();
        storage_remove_btn.set_label(&i18n("Remove…"));
        storage_remove_btn.set_valign(gtk::Align::Center);
        storage_remove_btn.add_css_class("destructive-action");
        // Kept current by update_view.
        storage_remove_btn.set_sensitive(model.storage.is_some_and(|(b, f, _)| b + f > 0));
        {
            let sender = sender.clone();
            storage_remove_btn.connect_clicked(move |_| sender.input(PrefInput::RemoveDownloads));
        }
        storage_row.add_suffix(&storage_remove_btn);
        preferences_group_4.add(&storage_row);
        let keep_downloads_row = adw::ComboRow::default();
        keep_downloads_row.set_title(&i18n("Keep downloaded mail"));
        keep_downloads_row.set_subtitle(&i18n("Mail inside an account's offline period is kept."));
        {
            let sender = sender.clone();
            keep_downloads_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeKeepDownloads(row.selected()));
            });
        }
        preferences_group_4.add(&keep_downloads_row);
        page.add(&preferences_group_4);
        // The Files right-click extension (#188).
        let preferences_group_5 = adw::PreferencesGroup::default();
        preferences_group_5.set_title(&i18n("GNOME Files Integration"));
        preferences_group_5.set_description(Some(
                        i18n("Adds “Send with Hylki” to the right-click menu in Files. Needs the nautilus-python package and a restart of Files.").as_str()
                    ));
        let nautilus_row = adw::ActionRow::default();
        nautilus_row.set_title(&i18n("Right-click menu entry"));
        // Kept current by update_view.
        nautilus_row.set_subtitle(&nautilus_status_text(&model.nautilus));
        let nautilus_remove_btn = gtk::Button::default();
        nautilus_remove_btn.set_label(&i18n("Remove"));
        nautilus_remove_btn.set_valign(gtk::Align::Center);
        // Kept current by update_view.
        nautilus_remove_btn.set_visible(
            model.nautilus.status != crate::desktop::nautilus_ext::Status::NotInstalled,
        );
        {
            let sender = sender.clone();
            nautilus_remove_btn.connect_clicked(move |_| sender.input(PrefInput::NautilusRemove));
        }
        nautilus_row.add_suffix(&nautilus_remove_btn);
        let nautilus_install_btn = gtk::Button::default();
        // Kept current by update_view.
        nautilus_install_btn.set_label(&nautilus_install_label(model));
        nautilus_install_btn.set_valign(gtk::Align::Center);
        nautilus_install_btn.add_css_class("suggested-action");
        // Kept current by update_view.
        nautilus_install_btn
            .set_visible(model.nautilus.status != crate::desktop::nautilus_ext::Status::Installed);
        {
            let sender = sender.clone();
            nautilus_install_btn.connect_clicked(move |_| sender.input(PrefInput::NautilusInstall));
        }
        nautilus_row.add_suffix(&nautilus_install_btn);
        preferences_group_5.add(&nautilus_row);
        // The loader's install command, ready to paste, while
        // there is no sign Files can load the extension.
        let nautilus_cmd_row = adw::EntryRow::default();
        nautilus_cmd_row.set_title(&i18n("Install nautilus-python first"));
        nautilus_cmd_row.set_text(model.nautilus_cmd.unwrap_or_default());
        nautilus_cmd_row.set_editable(false);
        nautilus_cmd_row.add_css_class("monospace");
        // Kept current by update_view.
        nautilus_cmd_row.set_visible(nautilus_cmd_wanted(model));
        let button_6 = gtk::Button::default();
        button_6.set_icon_name("edit-copy-symbolic");
        button_6.set_valign(gtk::Align::Center);
        button_6.set_tooltip_text(Some(i18n("Copy").as_str()));
        button_6.add_css_class("flat");
        {
            let cmd = model.nautilus_cmd.unwrap_or_default().to_string();
            button_6.connect_clicked(move |b| {
                b.clipboard().set_text(&cmd);
                b.set_icon_name("verified-checkmark-symbolic");
                let b = b.clone();
                gtk::glib::timeout_add_local_once(
                    std::time::Duration::from_millis(1200),
                    move || {
                        b.set_icon_name("edit-copy-symbolic");
                    },
                );
            });
        }
        nautilus_cmd_row.add_suffix(&button_6);
        preferences_group_5.add(&nautilus_cmd_row);
        let action_row_7 = adw::ActionRow::default();
        action_row_7.set_title(&i18n("Restart Files"));
        action_row_7.set_subtitle(&i18n("Closes every Files window, like “nautilus -q”."));
        let button_8 = gtk::Button::default();
        button_8.set_label(&i18n("Restart"));
        button_8.set_valign(gtk::Align::Center);
        {
            let sender = sender.clone();
            button_8.connect_clicked(move |_| sender.input(PrefInput::NautilusRestartFiles));
        }
        action_row_7.add_suffix(&button_8);
        preferences_group_5.add(&action_row_7);
        // What the handed-in files open into, and
        // what happens when they are big.
        let files_action_row = adw::ComboRow::default();
        files_action_row.set_title(&i18n("Default Send with Hylki behavior"));
        {
            let sender = sender.clone();
            files_action_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeFilesAction(row.selected()));
            });
        }
        preferences_group_5.add(&files_action_row);
        let files_large_row = adw::ComboRow::default();
        files_large_row.set_title(&i18n("Default large attachments behavior"));
        files_large_row.set_subtitle(&i18n(
            "Without a cloud storage account, files are attached.",
        ));
        {
            let sender = sender.clone();
            files_large_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeFilesLarge(row.selected()));
            });
        }
        preferences_group_5.add(&files_large_row);
        let files_limit_row = adw::SpinRow::default();
        files_limit_row.set_title(&i18n("Size limit"));
        files_limit_row.set_subtitle(&i18n("In MB, for all the files together."));
        files_limit_row.set_adjustment(Some(&gtk::Adjustment::new(
            20.0, 1.0, 5000.0, 1.0, 10.0, 0.0,
        )));
        files_limit_row.set_numeric(true);
        {
            let sender = sender.clone();
            files_limit_row.connect_value_notify(move |row| {
                sender.input(PrefInput::ChangeFilesLimit(row.value().round() as u32));
            });
        }
        preferences_group_5.add(&files_limit_row);
        page.add(&preferences_group_5);
        // Where a link in a message goes (#232).
        let preferences_group_9 = adw::PreferencesGroup::default();
        preferences_group_9.set_title(&i18n("Links"));
        preferences_group_9.set_description(Some(link_group_description(&model.browsers).as_str()));
        // Its handler is connected after the list
        // is filled below, not here: giving a
        // ComboRow its model moves the selection
        // to the first item, and that notification
        // would read as the user choosing
        // "System default".
        let link_browser_row = adw::ComboRow::default();
        link_browser_row.set_title(&i18n("Open links in"));
        preferences_group_9.add(&link_browser_row);
        page.add(&preferences_group_9);
        Self {
            page,
            language_row,
            renderer_row,
            background_row,
            autostart_row,
            fullscreen_row,
            tray_row,
            tray_icon_row,
            tray_mail_row,
            launcher_badge_row,
            single_key_row,
            gmail_keys_row,
            status_bar_row,
            console_mode_row,
            storage_row,
            storage_remove_btn,
            keep_downloads_row,
            nautilus_row,
            nautilus_remove_btn,
            nautilus_install_btn,
            nautilus_cmd_row,
            files_action_row,
            files_large_row,
            files_limit_row,
            link_browser_row,
        }
    }

    /// The view's `#[watch]` setters for this page, in the order the
    /// view had them (see the module comment of `pages`).
    pub(in crate::ui::preferences) fn update_view(&self, model: &Preferences) {
        self.storage_row.set_subtitle(&storage_text(model.storage));
        self.storage_remove_btn
            .set_sensitive(model.storage.is_some_and(|(b, f, _)| b + f > 0));
        self.nautilus_row
            .set_subtitle(&nautilus_status_text(&model.nautilus));
        self.nautilus_remove_btn.set_visible(
            model.nautilus.status != crate::desktop::nautilus_ext::Status::NotInstalled,
        );
        self.nautilus_install_btn
            .set_label(&nautilus_install_label(model));
        self.nautilus_install_btn
            .set_visible(model.nautilus.status != crate::desktop::nautilus_ext::Status::Installed);
        self.nautilus_cmd_row
            .set_visible(nautilus_cmd_wanted(model));
    }
}

/// The Files extension's install button: Update over an older copy.
fn nautilus_install_label(model: &Preferences) -> String {
    if model.nautilus.status == crate::desktop::nautilus_ext::Status::Outdated {
        i18n("Update")
    } else {
        i18n("Install")
    }
}

/// Whether the loader's install command is shown: there is one for this
/// system, and no sign that Files can load the extension.
fn nautilus_cmd_wanted(model: &Preferences) -> bool {
    model.nautilus_cmd.is_some() && !model.nautilus.loaded && model.nautilus.loader != Some(true)
}
