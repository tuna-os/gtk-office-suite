// SPDX-License-Identifier: GPL-3.0-or-later
//
// Menus the apps add to: the header bar's main menu, and a context menu
// opened by a right-click (docs/GNOME-GUIDELINES.md §6).

use gtk4::{self as gtk, gio, prelude::*};

/// The main menu that `widget` (a header bar, or a window) holds: the
/// model of its "open-menu" button, for an app to add a section to.
pub fn primary_menu(widget: &gtk::Widget) -> Option<gio::Menu> {
    if let Some(b) = widget.downcast_ref::<gtk::MenuButton>() {
        if b.icon_name().as_deref() == Some("open-menu-symbolic") {
            return b.menu_model().and_then(|m| m.downcast::<gio::Menu>().ok());
        }
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        if let Some(m) = primary_menu(&c) {
            return Some(m);
        }
        child = c.next_sibling();
    }
    None
}

/// An unlabelled section of `items` (label, action), as GNOME's main menus
/// group their items.
pub fn section(items: &[(&str, &str)]) -> gio::Menu {
    let menu = gio::Menu::new();
    for (label, action) in items {
        menu.append(Some(label), Some(action));
    }
    menu
}

/// Open `model` as a context menu where `widget` is right-clicked (or
/// long-pressed on a touch screen), and on Shift+F10 or the Menu key at
/// the widget's centre. `before` runs first, with the click's position, so
/// the caller can move the selection under the pointer.
///
/// The menu is parented beside `widget`, not on it, and reused rather than
/// unparented on close, as `crate::popover` explains: a widget that draws
/// its own accessible children (the Tables grid) hides a popover inside it
/// from screen readers, and an unparented popover can crash GTK 4.14 (#1192).
pub fn attach_context_menu(widget: &impl IsA<gtk::Widget>, model: &gio::Menu, before: impl Fn(f64, f64) + 'static) {
    let widget = widget.as_ref().clone();
    let model = model.clone();
    let before = std::rc::Rc::new(before);
    let open = std::rc::Rc::new(move |w: &gtk::Widget, x: f64, y: f64| {
        before(x, y);
        let host: gtk::Widget = w.parent().unwrap_or_else(|| w.clone());
        let popover = popover_menu(&host, &model);
        let (hx, hy) = w
            .compute_point(&host, &gtk::graphene::Point::new(x as f32, y as f32))
            .map_or((x as i32, y as i32), |p| (p.x() as i32, p.y() as i32));
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(hx, hy, 1, 1)));
        popover.popup();
    });
    let right = gtk::GestureClick::new();
    right.set_button(gtk::gdk::BUTTON_SECONDARY);
    {
        let (open, w) = (open.clone(), widget.clone());
        right.connect_pressed(move |g, _, x, y| {
            g.set_state(gtk::EventSequenceState::Claimed);
            open(&w, x, y);
        });
    }
    widget.add_controller(right);
    let press = gtk::GestureLongPress::new();
    press.set_touch_only(true);
    {
        let (open, w) = (open.clone(), widget.clone());
        press.connect_pressed(move |_, x, y| open(&w, x, y));
    }
    widget.add_controller(press);
    // Capture phase: a widget that handles its own keys (the Tables grid
    // types into cells) would otherwise take Shift+F10 first.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let w = widget.clone();
        keys.connect_key_pressed(move |_, key, _, mods| {
            let menu_key = key == gtk::gdk::Key::Menu
                || (key == gtk::gdk::Key::F10 && mods.contains(gtk::gdk::ModifierType::SHIFT_MASK));
            if !menu_key {
                return gtk::glib::Propagation::Proceed;
            }
            open(&w, f64::from(w.width()) / 2.0, f64::from(w.height()) / 2.0);
            gtk::glib::Propagation::Stop
        });
    }
    widget.add_controller(keys);
}

/// The context menu popover on `host`, made on first use and kept.
fn popover_menu(host: &gtk::Widget, model: &gio::Menu) -> gtk::PopoverMenu {
    const NAME: &str = "context-menu";
    let mut child = host.first_child();
    while let Some(w) = child {
        if w.widget_name() == NAME {
            if let Ok(p) = w.clone().downcast::<gtk::PopoverMenu>() {
                return p;
            }
        }
        child = w.next_sibling();
    }
    let popover = gtk::PopoverMenu::from_model(Some(model));
    popover.set_widget_name(NAME);
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_parent(host);
    let p = popover.clone();
    host.connect_destroy(move |_| p.unparent());
    popover
}
