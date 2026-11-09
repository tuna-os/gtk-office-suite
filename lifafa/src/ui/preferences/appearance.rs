//! Settings → Appearance and Date & Time.

use super::*;

impl Preferences {
    pub(super) fn on_change_date_style(&mut self, i: u32, sender: ComponentSender<Self>) {
        if let Some((_, style)) = DATE_STYLES.get(i as usize) {
            let _ = sender.output(PrefOutput::SetDateStyle(*style));
        }
    }

    pub(super) fn on_change_clock_style(&mut self, i: u32, sender: ComponentSender<Self>) {
        if let Some((_, style)) = CLOCK_STYLES.get(i as usize) {
            let _ = sender.output(PrefOutput::SetClockStyle(*style));
        }
    }

    pub(super) fn on_change_app_theme(&mut self, index: u32, sender: ComponentSender<Self>) {
        let theme = APP_THEMES
            .get(index as usize)
            .map(|(_, t)| *t)
            .unwrap_or_default();
        let _ = sender.output(PrefOutput::SetAppTheme(theme));
    }

    pub(super) fn on_change_text_scale(&mut self, index: u32, sender: ComponentSender<Self>) {
        let percent = crate::desktop::text_scale::STEPS
            .get(index as usize)
            .copied()
            .unwrap_or(100);
        let _ = sender.output(PrefOutput::SetTextScale(percent));
    }

    pub(super) fn on_change_message_theme(&mut self, index: u32, sender: ComponentSender<Self>) {
        let theme = MESSAGE_THEMES
            .get(index as usize)
            .map(|(_, t)| *t)
            .unwrap_or_default();
        let _ = sender.output(PrefOutput::SetMessageTheme(theme));
    }
}
