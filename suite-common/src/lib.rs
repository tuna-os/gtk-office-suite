use libadwaita as adw;
use gtk4::prelude::*;

pub fn make_app(id: &str) -> gtk4::Application {
    gtk4::Application::new(Some(id), gtk4::gio::ApplicationFlags::empty())
}

/// Build a standard window chrome: ToolbarView with header bar + toolbar.
pub fn build_chrome(window: &adw::ApplicationWindow, toolbar: &gtk4::Box) {
    let header = adw::HeaderBar::new();
    let menu = gtk4::gio::Menu::new();
    menu.append(Some("Preferences"), Some("app.preferences"));
    menu.append(Some("Keyboard Shortcuts"), Some("app.shortcuts"));
    menu.append(Some("About"), Some("app.about"));
    let btn = gtk4::MenuButton::new();
    btn.set_icon_name("open-menu-symbolic");
    btn.set_menu_model(Some(&menu));
    header.pack_end(&btn);

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header);
    toolbar_view.add_top_bar(toolbar);
    toolbar_view.set_top_bar_style(adw::ToolbarStyle::Raised);
    window.set_content(Some(&toolbar_view));
}

pub fn make_toolbar() -> gtk4::Box {
    let t = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    t.set_halign(gtk4::Align::Center);
    t.add_css_class("toolbar");
    for label in &["B", "I", "U"] {
        t.append(&gtk4::ToggleButton::with_label(label));
    }
    t
}

/// Apply dark mode.
pub fn apply_dark() {
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
}
