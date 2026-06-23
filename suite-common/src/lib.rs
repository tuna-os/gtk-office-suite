use gtk4::prelude::*;
use libadwaita as adw;
pub fn make_app(id: &str) -> adw::Application {
    adw::Application::new(Some(id), gtk4::gio::ApplicationFlags::empty())
}
pub fn make_header_bar(title: &str) -> adw::HeaderBar {
    let h = adw::HeaderBar::new();
    h.set_centering_policy(adw::CenteringPolicy::Strict);
    let title_widget = adw::WindowTitle::new(title, "");
    h.set_title_widget(Some(&title_widget));

    // Open button
    let open_btn = gtk4::Button::builder()
        .icon_name("document-open-symbolic")
        .tooltip_text("Open")
        .css_classes(vec!["flat".to_string()])
        .build();
    open_btn.set_widget_name("open");
    h.pack_start(&open_btn);

    // Save button
    let save_btn = gtk4::Button::builder()
        .icon_name("document-save-symbolic")
        .tooltip_text("Save")
        .css_classes(vec!["flat".to_string()])
        .build();
    save_btn.set_widget_name("save");
    h.pack_end(&save_btn);

    h
}

pub fn make_toolbar() -> gtk4::Box {
    let t = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    t.set_halign(gtk4::Align::Center);
    t.add_css_class("toolbar");
    t.add_css_class("linked");

    let b = gtk4::ToggleButton::builder()
        .icon_name("format-text-bold-symbolic")
        .tooltip_text("Bold")
        .css_classes(vec!["flat".to_string()])
        .build();
    let i = gtk4::ToggleButton::builder()
        .icon_name("format-text-italic-symbolic")
        .tooltip_text("Italic")
        .css_classes(vec!["flat".to_string()])
        .build();
    let u = gtk4::ToggleButton::builder()
        .icon_name("format-text-underline-symbolic")
        .tooltip_text("Underline")
        .css_classes(vec!["flat".to_string()])
        .build();

    t.append(&b);
    t.append(&i);
    t.append(&u);
    t
}

/// Apply system dark mode preference.
pub fn apply_dark_mode() {
    let style = libadwaita::StyleManager::default();
    let dark = style.is_dark();
    if dark {
        // GTK4 handles dark mode via the theme, but web/app content needs explicit CSS
        if let Some(display) = gtk4::gdk::Display::default() {
            let settings = gtk4::Settings::default();
            settings.set_gtk_application_prefer_dark_theme(true);
        }
    }
}
