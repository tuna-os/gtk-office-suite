//! Settings → Notifications: the new-mail alerts, their buttons and sound
//! (#244, #292), and how often mail is fetched.

use super::*;

impl Preferences {
    pub(super) fn on_change_fetch_interval(&mut self, index: u32, sender: ComponentSender<Self>) {
        let secs = FETCH_INTERVALS
            .get(index as usize)
            .map(|(_, s)| *s)
            .unwrap_or(0);
        let _ = sender.output(PrefOutput::SetFetchInterval(secs));
    }

    pub(super) fn on_toggle_notifications(&mut self, on: bool, sender: ComponentSender<Self>) {
        self.notifications = on;
        let _ = sender.output(PrefOutput::SetNotifications(on));
    }

    pub(super) fn on_toggle_notification_button(
        &mut self,
        button: crate::config::NotificationButton,
        on: bool,
        sender: ComponentSender<Self>,
    ) {
        // A greyed-out row cannot be switched, so a fourth never
        // arrives here; the guard is for the belt and braces.
        if on && !self.notification_buttons.get(button) && self.notification_buttons.full() {
            return;
        }
        self.notification_buttons.set(button, on);
        let _ = sender.output(PrefOutput::SetNotificationButtons(
            self.notification_buttons,
        ));
    }

    pub(super) fn on_choose_sound(&mut self, sender: ComponentSender<Self>, root: &adw::Window) {
        let dialog = gtk::FileDialog::builder()
            .title(i18n("Choose a Sound"))
            .build();
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&i18n("Sounds")));
        filter.add_mime_type("audio/*");
        let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        dialog.set_filters(Some(&filters));
        dialog.set_default_filter(Some(&filter));
        let s = sender.clone();
        dialog.open(Some(root), gtk::gio::Cancellable::NONE, move |res| {
            let Ok(file) = res else { return };
            let Some(path) = file.path() else { return };
            s.input(PrefInput::SoundChosen(path));
        });
    }

    pub(super) fn on_sound_chosen(&mut self, path: std::path::PathBuf) {
        match crate::config::set_custom_sound(&path) {
            Ok(copy) => {
                crate::desktop::notify::play_sound(
                    &crate::config::SoundSource::File(copy.clone()),
                    true,
                );
                self.custom_sound = Some(copy);
            }
            Err(e) => tracing::warn!(
                "could not keep {} as the new-mail sound: {e}",
                path.display()
            ),
        }
    }

    pub(super) fn on_play_sound(&mut self) {
        if let Some(source) = self.sound.source() {
            crate::desktop::notify::play_sound(&source, true);
        }
    }

    pub(super) fn on_toggle_sound(&mut self, on: bool) {
        if self.sound.enabled != on {
            self.sound.enabled = on;
            crate::config::save_new_mail_sound(&self.sound);
        }
    }

    pub(super) fn on_toggle_sound_focused(&mut self, on: bool) {
        if self.sound.when_focused != on {
            self.sound.when_focused = on;
            crate::config::save_new_mail_sound(&self.sound);
        }
    }

    pub(super) fn on_change_sound(&mut self, index: u32, sender: ComponentSender<Self>) {
        let name = crate::config::BUILTIN_SOUNDS
            .get(index as usize)
            .copied()
            .unwrap_or(crate::config::CUSTOM_SOUND);
        if self.sound.sound == name {
            return;
        }
        self.sound.sound = name.to_string();
        crate::config::save_new_mail_sound(&self.sound);
        // Picking a sound plays it; Custom File with no file yet
        // goes straight to the file chooser.
        match self.sound.source() {
            Some(source) => crate::desktop::notify::play_sound(&source, true),
            None => sender.input(PrefInput::ChooseSound),
        }
    }
}
