//! Settings → Date and Time.

use adw::prelude::*;

use super::super::{PrefInput, ViewSender};
use crate::i18n::i18n;

/// The page, and the rows on it that the window reaches later.
#[derive(Debug)]
pub(in crate::ui::preferences) struct DateTimePage {
    pub(in crate::ui::preferences) page: adw::PreferencesPage,
    pub(in crate::ui::preferences) date_style_row: adw::ComboRow,
    pub(in crate::ui::preferences) clock_style_row: adw::ComboRow,
}

impl DateTimePage {
    pub(in crate::ui::preferences) fn build(sender: &ViewSender) -> Self {
        let page = adw::PreferencesPage::default();
        let preferences_group_1 = adw::PreferencesGroup::default();
        preferences_group_1.set_title(&i18n("Date and Time"));
        preferences_group_1.set_description(Some(
            i18n("Dates follow the system unless a format is chosen here.").as_str(),
        ));
        let date_style_row = adw::ComboRow::default();
        date_style_row.set_title(&i18n("Date format"));
        {
            let sender = sender.clone();
            date_style_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeDateStyle(row.selected()));
            });
        }
        preferences_group_1.add(&date_style_row);
        let clock_style_row = adw::ComboRow::default();
        clock_style_row.set_title(&i18n("Clock"));
        {
            let sender = sender.clone();
            clock_style_row.connect_selected_notify(move |row| {
                sender.input(PrefInput::ChangeClockStyle(row.selected()));
            });
        }
        preferences_group_1.add(&clock_style_row);
        page.add(&preferences_group_1);
        Self {
            page,
            date_style_row,
            clock_style_row,
        }
    }
}
