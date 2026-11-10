//! Settings → Reading: the reading pane, Reader View and printing.

use adw::prelude::*;

use super::super::{PrefInput, PrefOutput, ViewSender};
use crate::i18n::i18n;

/// The page, and the rows on it that the window reaches later.
#[derive(Debug)]
pub(in crate::ui::preferences) struct ReadingPage {
    pub(in crate::ui::preferences) page: adw::PreferencesPage,
    pub(in crate::ui::preferences) read_mark_row: adw::ComboRow,
    pub(in crate::ui::preferences) message_theme_row: adw::ComboRow,
    pub(in crate::ui::preferences) theme_switch_row: adw::SwitchRow,
    pub(in crate::ui::preferences) account_chip_row: adw::SwitchRow,
    pub(in crate::ui::preferences) reader_zoom_row: adw::ComboRow,
    pub(in crate::ui::preferences) override_fonts_row: adw::SwitchRow,
    pub(in crate::ui::preferences) reader_font_row: adw::ActionRow,
    pub(in crate::ui::preferences) reader_font_button: gtk::FontDialogButton,
    pub(in crate::ui::preferences) override_colors_row: adw::SwitchRow,
    pub(in crate::ui::preferences) plain_mono_row: adw::SwitchRow,
    pub(in crate::ui::preferences) plain_font_row: adw::ActionRow,
    pub(in crate::ui::preferences) plain_font_button: gtk::FontDialogButton,
    pub(in crate::ui::preferences) single_message_card_row: adw::SwitchRow,
    pub(in crate::ui::preferences) always_show_recipients_row: adw::SwitchRow,
    pub(in crate::ui::preferences) pgp_labels_row: adw::SwitchRow,
    pub(in crate::ui::preferences) find_place_row: adw::ComboRow,
    pub(in crate::ui::preferences) reader_switch_row: adw::SwitchRow,
    pub(in crate::ui::preferences) reader_default_row: adw::ComboRow,
    pub(in crate::ui::preferences) print_page_numbers_row: adw::SwitchRow,
    pub(in crate::ui::preferences) print_date_row: adw::SwitchRow,
    pub(in crate::ui::preferences) print_punch_margin_row: adw::SwitchRow,
}

impl ReadingPage {
    pub(in crate::ui::preferences) fn build(sender: &ViewSender) -> Self {
        let page = adw::PreferencesPage::default();
        let preferences_group_1 = adw::PreferencesGroup::default();
        preferences_group_1.set_title(&i18n("Reading"));
        let read_mark_row = adw::ComboRow::default();
        read_mark_row.set_title(&i18n("Mark as read"));
        read_mark_row.set_subtitle(&i18n(
            "Conversations mark each message as it comes into view.",
        ));
        {
            let sender = sender.clone();
            read_mark_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeReadMark(row.selected()));
            });
        }
        preferences_group_1.add(&read_mark_row);
        let message_theme_row = adw::ComboRow::default();
        message_theme_row.set_title(&i18n("Message appearance"));
        {
            let sender = sender.clone();
            message_theme_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeMessageTheme(row.selected()));
            });
        }
        preferences_group_1.add(&message_theme_row);
        let theme_switch_row = adw::SwitchRow::default();
        theme_switch_row.set_title(&i18n("Light / Dark Mode switch"));
        theme_switch_row.set_subtitle(&i18n("Displays in the message header for each message."));
        {
            let sender = sender.clone();
            theme_switch_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetThemeSwitch(row.is_active()));
            });
        }
        preferences_group_1.add(&theme_switch_row);
        let account_chip_row = adw::SwitchRow::default();
        account_chip_row.set_title(&i18n("Show account in message header"));
        account_chip_row.set_subtitle(&i18n("The account a message is in, above its subject."));
        {
            let sender = sender.clone();
            account_chip_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetAccountChip(row.is_active()));
            });
        }
        preferences_group_1.add(&account_chip_row);
        let reader_zoom_row = adw::ComboRow::default();
        reader_zoom_row.set_title(&i18n("Default zoom"));
        reader_zoom_row.set_subtitle(&i18n("Ctrl+0 returns to the default."));
        {
            let sender = sender.clone();
            reader_zoom_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeReaderZoom(row.selected()));
            });
        }
        preferences_group_1.add(&reader_zoom_row);
        let override_fonts_row = adw::SwitchRow::default();
        override_fonts_row.set_title(&i18n("Use my own font"));
        override_fonts_row.set_subtitle(&i18n("The font below replaces the sender's."));
        {
            let sender = sender.clone();
            override_fonts_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetOverrideFonts(row.is_active()));
            });
        }
        preferences_group_1.add(&override_fonts_row);
        let reader_font_row = adw::ActionRow::default();
        reader_font_row.set_title(&i18n("Message font"));
        reader_font_row.set_subtitle(&i18n("The interface font unless another is chosen."));
        let reader_font_button = gtk::FontDialogButton::default();
        reader_font_button.set_valign(gtk::Align::Center);
        reader_font_button.set_dialog(&gtk::FontDialog::new());
        reader_font_button.set_level(gtk::FontLevel::Font);
        reader_font_button.set_use_font(true);
        {
            let sender = sender.clone();
            reader_font_button.connect_font_desc_notify(move |button| {
                let font = button
                    .font_desc()
                    .map(|d| d.to_string())
                    .unwrap_or_default();
                let _ = sender.output(PrefOutput::SetReaderFont(font));
            });
        }
        reader_font_row.add_suffix(&reader_font_button);
        preferences_group_1.add(&reader_font_row);
        let override_colors_row = adw::SwitchRow::default();
        override_colors_row.set_title(&i18n("Use my own colors"));
        override_colors_row.set_subtitle(&i18n(
            "Ignores the sender's colors. Pictures stay; links use the accent color.",
        ));
        {
            let sender = sender.clone();
            override_colors_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetOverrideColors(row.is_active()));
            });
        }
        preferences_group_1.add(&override_colors_row);
        let plain_mono_row = adw::SwitchRow::default();
        plain_mono_row.set_title(&i18n("Plain-text messages in monospace"));
        plain_mono_row.set_subtitle(&i18n("Formatted messages are not affected."));
        {
            let sender = sender.clone();
            plain_mono_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetPlainMonospace(row.is_active()));
            });
        }
        preferences_group_1.add(&plain_mono_row);
        let plain_font_row = adw::ActionRow::default();
        plain_font_row.set_title(&i18n("Monospace font"));
        plain_font_row.set_subtitle(&i18n(
            "The system's monospace font unless another is chosen.",
        ));
        let plain_font_button = gtk::FontDialogButton::default();
        plain_font_button.set_valign(gtk::Align::Center);
        plain_font_button.set_dialog(&gtk::FontDialog::new());
        plain_font_button.set_level(gtk::FontLevel::Font);
        plain_font_button.set_use_font(true);
        {
            let sender = sender.clone();
            plain_font_button.connect_font_desc_notify(move |button| {
                let font = button
                    .font_desc()
                    .map(|d| d.to_string())
                    .unwrap_or_default();
                let _ = sender.output(PrefOutput::SetPlainFont(font));
            });
        }
        plain_font_row.add_suffix(&plain_font_button);
        preferences_group_1.add(&plain_font_row);
        let single_message_card_row = adw::SwitchRow::default();
        single_message_card_row.set_title(&i18n("Single messages as cards"));
        single_message_card_row.set_subtitle(&i18n("Off fills the pane edge to edge."));
        {
            let sender = sender.clone();
            single_message_card_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetSingleMessageCard(row.is_active()));
            });
        }
        preferences_group_1.add(&single_message_card_row);
        let always_show_recipients_row = adw::SwitchRow::default();
        always_show_recipients_row.set_title(&i18n("Always show recipients"));
        {
            let sender = sender.clone();
            always_show_recipients_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetAlwaysShowRecipients(row.is_active()));
            });
        }
        preferences_group_1.add(&always_show_recipients_row);
        let pgp_labels_row = adw::SwitchRow::default();
        pgp_labels_row.set_title(&i18n("Name the OpenPGP result"));
        pgp_labels_row.set_subtitle(&i18n("Off shows the icons alone."));
        {
            let sender = sender.clone();
            pgp_labels_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetPgpLabels(row.is_active()));
            });
        }
        preferences_group_1.add(&pgp_labels_row);
        let find_place_row = adw::ComboRow::default();
        find_place_row.set_title(&i18n("Find bar position"));
        {
            let sender = sender.clone();
            find_place_row.connect_selected_notify(move |row| {
                let _ = sender.output(PrefOutput::SetReaderFindBottom(row.selected() == 1));
            });
        }
        preferences_group_1.add(&find_place_row);
        page.add(&preferences_group_1);
        let preferences_group_2 = adw::PreferencesGroup::default();
        preferences_group_2.set_title(&i18n("Reader View"));
        preferences_group_2.set_description(Some(&i18n(
            "A message's text alone, without the sender's layout, colors and fonts.",
        )));
        let reader_switch_row = adw::SwitchRow::default();
        reader_switch_row.set_title(&i18n("Show Reader View switch in message header"));
        {
            let sender = sender.clone();
            reader_switch_row.connect_active_notify(move |row| {
                let _ = sender.output(PrefOutput::SetReaderSwitch(row.is_active()));
            });
        }
        preferences_group_2.add(&reader_switch_row);
        let reader_default_row = adw::ComboRow::default();
        reader_default_row.set_title(&i18n("When a message is opened"));
        reader_default_row
            .set_subtitle(&i18n("The header's switch still changes the open message."));
        {
            let sender = sender.clone();
            reader_default_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeReaderDefault(row.selected()));
            });
        }
        preferences_group_2.add(&reader_default_row);
        page.add(&preferences_group_2);
        let preferences_group_3 = adw::PreferencesGroup::default();
        preferences_group_3.set_title(&i18n("Printing"));
        let print_page_numbers_row = adw::SwitchRow::default();
        print_page_numbers_row.set_title(&i18n("Page numbers"));
        {
            let sender = sender.clone();
            print_page_numbers_row.connect_active_notify(move |row| {
                sender.input(PrefInput::TogglePrintPageNumbers(row.is_active()));
            });
        }
        preferences_group_3.add(&print_page_numbers_row);
        let print_date_row = adw::SwitchRow::default();
        print_date_row.set_title(&i18n("Date printed"));
        {
            let sender = sender.clone();
            print_date_row.connect_active_notify(move |row| {
                sender.input(PrefInput::TogglePrintDate(row.is_active()));
            });
        }
        preferences_group_3.add(&print_date_row);
        let print_punch_margin_row = adw::SwitchRow::default();
        print_punch_margin_row.set_title(&i18n("Wider left margin"));
        print_punch_margin_row.set_subtitle(&i18n("25 mm, so punched holes miss the text."));
        {
            let sender = sender.clone();
            print_punch_margin_row.connect_active_notify(move |row| {
                sender.input(PrefInput::TogglePrintPunchMargin(row.is_active()));
            });
        }
        preferences_group_3.add(&print_punch_margin_row);
        page.add(&preferences_group_3);
        Self {
            page,
            read_mark_row,
            message_theme_row,
            theme_switch_row,
            account_chip_row,
            reader_zoom_row,
            override_fonts_row,
            reader_font_row,
            reader_font_button,
            override_colors_row,
            plain_mono_row,
            plain_font_row,
            plain_font_button,
            single_message_card_row,
            always_show_recipients_row,
            pgp_labels_row,
            find_place_row,
            reader_switch_row,
            reader_default_row,
            print_page_numbers_row,
            print_date_row,
            print_punch_margin_row,
        }
    }
}
