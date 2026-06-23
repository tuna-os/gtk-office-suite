use libadwaita as adw;
use gtk4::prelude::*;

pub fn make_app(id: &str) -> gtk4::Application {
    gtk4::Application::new(Some(id), gtk4::gio::ApplicationFlags::empty())
}

pub fn apply_dark_mode() {
    let style = adw::StyleManager::default();
    style.set_color_scheme(adw::ColorScheme::PreferDark);
}

pub fn make_header_bar() -> adw::HeaderBar {
    let h = adw::HeaderBar::new();
    let open = gtk4::Button::with_label("Open");
    h.pack_start(&open);
    let save = gtk4::Button::with_label("Save");
    h.pack_end(&save);
    h
}

pub fn make_toolbar() -> gtk4::Box {
    let t = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    t.set_halign(gtk4::Align::Center);
    t.add_css_class("toolbar");
    for (icon, label) in &[
        ("format-text-bold-symbolic", "Bold"),
        ("format-text-italic-symbolic", "Italic"),
        ("format-text-underline-symbolic", "Underline")
    ] {
        let btn = gtk4::ToggleButton::new();
        btn.set_icon_name(*icon);
        btn.set_tooltip_text(Some(*label));
        t.append(&btn);
    }
    t
}
