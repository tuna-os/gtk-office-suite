//! call_ui.rs — "Present in Call…" (INTEGRATION-PLAN A5, ADR 0016).
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Decks never captures itself and never hands frames to anyone: Mandelbrot
//! shares into the call, through the ScreenCast portal, whatever the person
//! picks in the desktop's own picker. This item is a shortcut to that. It
//! starts the slide show, then asks Mandelbrot to open its Share Screen
//! picker (its `app.share-screen` action, over
//! `org.freedesktop.Application.ActivateAction`), where the slide window,
//! titled "<deck> — Slide Show", is the one to pick.
//!
//! The item shows only while Mandelbrot is running: its bus name is watched,
//! and the menu item is hidden while the action is disabled. Inside Flatpak
//! that needs `--talk-name=org.tunaos.mandelbrot`; without it the name is
//! never seen and the item never shows.

use adw::prelude::*;
use gtk4::{gio, glib};
use libadwaita as adw;

/// Mandelbrot's application id: its bus name, and its object path below.
pub(crate) const MANDELBROT: &str = "org.tunaos.mandelbrot";
const MANDELBROT_PATH: &str = "/org/tunaos/mandelbrot";
/// The action ADR 0016 names; both sides keep it.
pub(crate) const SHARE_ACTION: &str = "share-screen";

fn menu_section() -> gio::Menu {
    let section = gio::Menu::new();
    let item = gio::MenuItem::new(Some("Present in _Call…"), Some("app.present-in-call"));
    item.set_attribute_value("hidden-when", Some(&"action-disabled".to_variant()));
    section.append_item(&item);
    section
}

/// The ActivateAction call's arguments: the action, no parameter, no
/// platform data (`(sava{sv})`).
fn activate_args(action: &str) -> glib::Variant {
    glib::Variant::tuple_from_iter([
        action.to_variant(),
        glib::Variant::array_from_iter_with_type(glib::VariantTy::VARIANT, std::iter::empty::<glib::Variant>()),
        glib::VariantDict::new(None).end(),
    ])
}

/// Register `app.present-in-call` and its menu item.
pub(crate) fn register(app: &adw::Application, window: &adw::ApplicationWindow, header: &adw::HeaderBar) {
    if let Some(menu) = suite_common::menus::primary_menu(header.upcast_ref()) {
        menu.insert_section(1, None, &menu_section());
    }
    let act = gio::SimpleAction::new("present-in-call", None);
    act.set_enabled(false);
    let win = window.downgrade();
    let app2 = app.clone();
    act.connect_activate(move |_, _| {
        app2.activate_action("present", None);
        let win = win.clone();
        let Ok(conn) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else { return };
        conn.call(
            Some(MANDELBROT),
            MANDELBROT_PATH,
            "org.freedesktop.Application",
            "ActivateAction",
            Some(&activate_args(SHARE_ACTION)),
            None,
            gio::DBusCallFlags::NONE,
            5000,
            gio::Cancellable::NONE,
            move |result| {
                if result.is_err() {
                    // A Mandelbrot without Share Screen, or one that went
                    // away: say how to share by hand rather than nothing.
                    let window = win.upgrade();
                    suite_common::show_error_dialog(
                        window.as_ref(),
                        &suite_common::i18n("Could Not Start Sharing"),
                        &suite_common::i18n("Mandelbrot did not open its screen sharing. In the call, choose Share Screen and pick the window ending in \u{201c}Slide Show\u{201d}."),
                    );
                }
            },
        );
    });
    app.add_action(&act);
    // Enabled while Mandelbrot is on the bus. The watch lasts as long as the
    // application, so its id is not kept.
    let (on, off) = (act.downgrade(), act.downgrade());
    let _ = gio::bus_watch_name(
        gio::BusType::Session,
        MANDELBROT,
        gio::BusNameWatcherFlags::NONE,
        move |_, _, _| {
            if let Some(a) = on.upgrade() {
                a.set_enabled(true);
            }
        },
        move |_, _| {
            if let Some(a) = off.upgrade() {
                a.set_enabled(false);
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_call_names_the_action_with_no_parameter_and_no_platform_data() {
        let args = activate_args(SHARE_ACTION);
        assert_eq!(args.type_().as_str(), "(sava{sv})");
        assert_eq!(args.child_value(0).str(), Some("share-screen"));
        assert_eq!(args.child_value(1).n_children(), 0);
        assert_eq!(args.child_value(2).n_children(), 0);
    }

    #[test]
    fn the_item_hides_while_mandelbrot_is_not_running() {
        let m = menu_section();
        assert_eq!(
            m.item_attribute_value(0, "action", None).and_then(|v| v.str().map(str::to_owned)).as_deref(),
            Some("app.present-in-call")
        );
        assert_eq!(
            m.item_attribute_value(0, "hidden-when", None).and_then(|v| v.str().map(str::to_owned)).as_deref(),
            Some("action-disabled")
        );
    }
}
