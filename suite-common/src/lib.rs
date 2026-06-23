use libadwaita as adw;
use gtk4::prelude::*;

pub fn make_app(id: &str) -> gtk4::Application {
    gtk4::Application::new(Some(id), gtk4::gio::ApplicationFlags::empty())
}

pub fn make_header_bar() -> adw::HeaderBar {
    let h = adw::HeaderBar::new();
    let menu = gtk4::gio::Menu::new();
    menu.append(Some("Preferences"), Some("app.preferences"));
    menu.append(Some("Keyboard Shortcuts"), Some("app.shortcuts"));
    menu.append(Some("About"), Some("app.about"));
    let btn = gtk4::MenuButton::new();
    btn.set_icon_name("open-menu-symbolic");
    btn.set_menu_model(Some(&menu));
    h.pack_end(&btn);
    h
}

pub fn make_toolbar() -> gtk4::Box {
    let t = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    t.set_halign(gtk4::Align::Center);
    t.add_css_class("toolbar");
    for label in &["B", "I", "U"] { t.append(&gtk4::ToggleButton::with_label(label)); }
    t
}
