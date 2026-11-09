//! Moving through the wizard: the language, Next and Back, and finishing.

use super::*;

impl Welcome {
    pub(super) fn on_language_changed(&mut self, i: u32, sender: ComponentSender<Self>) {
        if let Some((_, code)) = crate::ui::preferences::language_choices().get(i as usize) {
            let _ = sender.output(WelcomeOutput::Language(code.clone()));
        }
    }

    pub(super) fn on_next(&mut self, widgets: &WelcomeWidgets, sender: ComponentSender<Self>) {
        // position() is fractional mid-scroll; round to the page the
        // user sees as current, so rapid clicks don't repeat a page.
        let pos = widgets.carousel.position().round() as u32;
        if pos + 1 >= widgets.carousel.n_pages() {
            // The footer button on the last page is Finish.
            sender.input_sender().emit(WelcomeInput::Finish);
            return;
        }
        let next = widgets.carousel.nth_page(pos + 1);
        widgets.carousel.scroll_to(&next, true);
        widgets.back_btn.set_visible(true);
    }

    pub(super) fn on_back(&mut self, widgets: &WelcomeWidgets) {
        let pos = widgets.carousel.position().round() as usize;
        if pos > 0 {
            let prev = widgets.carousel.nth_page(pos as u32 - 1);
            widgets.carousel.scroll_to(&prev, true);
            widgets.back_btn.set_visible(pos > 1);
        }
    }

    pub(super) fn on_finish(
        &mut self,
        widgets: &WelcomeWidgets,
        sender: ComponentSender<Self>,
        root: &adw::Window,
    ) {
        let prefs = WelcomePrefs {
            block_remote: widgets.sw_remote.is_active(),
            gravatar: widgets.sw_gravatar.is_active(),
            sender_logos: widgets.sw_logos.is_active(),
            notification_content: widgets.sw_notif.is_active(),
            preview_lines: widgets.preview_row.selected(),
            avatars: widgets.sw_avatars.is_active(),
            threading: widgets.sw_threading.is_active(),
            app_icon: widgets.icon_choice.borrow().clone(),
        };
        let _ = sender.output(WelcomeOutput::Prefs(prefs));
        let _ = sender.output(WelcomeOutput::Done);
        root.close();
    }
}
