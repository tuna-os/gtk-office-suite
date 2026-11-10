//! Helpers to add key bindings to widgets.

use std::ops::Deref;

use gtk::{gdk, subclass::prelude::*};

/// List of keys that activate a widget.
// Copied from GtkButton's source code.
const ACTIVATE_KEYS: &[gdk::Key] = &[
    gdk::Key::space,
    gdk::Key::KP_Space,
    gdk::Key::Return,
    gdk::Key::ISO_Enter,
    gdk::Key::KP_Enter,
];

/// Add key bindings to the given class to trigger the given action to activate
/// a widget.
///
/// Since gtk4-rs 0.11.4, `add_binding_action()` lives on
/// [`WidgetClassManualExt`], which is implemented for `glib::Class<W>` rather
/// than for the class struct itself, so it is reached through the class
/// struct's [`Deref`]. Callers still pass their `&mut Self::Class` unchanged.
pub(crate) fn add_activate_bindings<T>(klass: &mut T, action: &str)
where
    T: WidgetClassExt + Deref,
    T::Target: WidgetClassManualExt,
{
    for key in ACTIVATE_KEYS {
        klass.add_binding_action(*key, gdk::ModifierType::empty(), action);
    }
}

/// List of key and modifier combos that trigger a context menu to appear.
const CONTEXT_MENU_BINDINGS: &[(gdk::Key, gdk::ModifierType)] = &[
    (gdk::Key::F10, gdk::ModifierType::SHIFT_MASK),
    (gdk::Key::Menu, gdk::ModifierType::empty()),
];

/// Add key bindings to the given class to trigger the given action to show a
/// context menu.
///
/// See [`add_activate_bindings()`] for why the [`Deref`] bound is needed.
pub(crate) fn add_context_menu_bindings<T>(klass: &mut T, action: &str)
where
    T: WidgetClassExt + Deref,
    T::Target: WidgetClassManualExt,
{
    for (key, modifier) in CONTEXT_MENU_BINDINGS {
        klass.add_binding_action(*key, *modifier, action);
    }
}
