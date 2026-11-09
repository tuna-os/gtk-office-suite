//! The Ctrl shortcuts of the main window, which answer with an app message
//! rather than a [`Shortcut`].

use crate::ui::keymap::Shortcut;

use super::msg::AppMsg;

/// The Ctrl shortcuts that work whether or not single-key ones are on (#328).
pub(super) fn ctrl_shortcut_for(key: gtk::gdk::Key, shift: bool) -> Option<AppMsg> {
    use gtk::gdk::Key;
    match key {
        Key::n | Key::N if !shift => Some(AppMsg::Shortcut(Shortcut::Compose)),
        Key::n | Key::N => Some(AppMsg::NewFromTemplate),
        Key::r | Key::R if !shift => Some(AppMsg::Shortcut(Shortcut::Reply)),
        Key::r | Key::R => Some(AppMsg::Shortcut(Shortcut::ReplyAll)),
        Key::u | Key::U if !shift => Some(AppMsg::ViewSource),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_shortcuts_follow_other_mail_apps() {
        use gtk::gdk::Key;
        // #328. Shift arrives as the capital, and Caps Lock alone as well.
        assert!(matches!(
            ctrl_shortcut_for(Key::n, false),
            Some(AppMsg::Shortcut(Shortcut::Compose))
        ));
        assert!(matches!(
            ctrl_shortcut_for(Key::r, false),
            Some(AppMsg::Shortcut(Shortcut::Reply))
        ));
        assert!(matches!(
            ctrl_shortcut_for(Key::R, false),
            Some(AppMsg::Shortcut(Shortcut::Reply))
        ));
        assert!(matches!(
            ctrl_shortcut_for(Key::R, true),
            Some(AppMsg::Shortcut(Shortcut::ReplyAll))
        ));
        assert!(matches!(
            ctrl_shortcut_for(Key::u, false),
            Some(AppMsg::ViewSource)
        ));
        assert!(matches!(
            ctrl_shortcut_for(Key::N, true),
            Some(AppMsg::NewFromTemplate)
        ));
        // Taken elsewhere, or not ours.
        assert!(ctrl_shortcut_for(Key::U, true).is_none());
        assert!(ctrl_shortcut_for(Key::f, false).is_none());
    }
}
