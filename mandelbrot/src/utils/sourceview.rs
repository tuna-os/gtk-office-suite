//! Collection of methods for interacting with `GtkSourceView`.

use gtk::glib;
use sourceview::prelude::*;

/// Setup the style scheme for the given buffer.
pub(crate) fn setup_style_scheme(buffer: &sourceview::Buffer) {
    let manager = adw::StyleManager::default();

    buffer.set_style_scheme(style_scheme().as_ref());

    manager.connect_dark_notify(glib::clone!(
        #[weak]
        buffer,
        move |_| {
            buffer.set_style_scheme(style_scheme().as_ref());
        }
    ));
}

/// Get the style scheme for the current appearance.
pub(crate) fn style_scheme() -> Option<sourceview::StyleScheme> {
    let manager = adw::StyleManager::default();
    let scheme_name = if manager.is_dark() {
        "Adwaita-dark"
    } else {
        "Adwaita"
    };

    sourceview::StyleSchemeManager::default().scheme(scheme_name)
}
