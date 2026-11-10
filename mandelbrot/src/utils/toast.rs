//! Macros and methods to display toasts in the interface.

use std::collections::HashMap;

use adw::prelude::*;

use super::freplace;
use crate::{
    Window,
    components::{LabelWithWidgets, Pill, ToastableDialog},
    prelude::*,
};

/// Show a toast with the given message on an ancestor of `widget`.
///
/// The simplest way to use this macro is for displaying a simple message. It
/// can be anything that implements `AsRef<str>`.
///
/// ```no_run
/// use gettextts::gettext;
///
/// use crate::toast;
///
/// # let widget = unimplemented!();
/// toast!(widget, gettext("Something happened"));
/// ```
///
/// This macro also supports replacing named variables with their value. It
/// supports both the `var` and the `var = expr` syntax. The variable value must
/// implement `ToString`.
///
/// ```no_run
/// use gettextts::gettext;
///
/// use crate::toast;
///
/// # let widget = unimplemented!();
/// # let error_nb = 0;
/// toast!(
///     widget,
///     gettext("Error number {n}: {msg}"),
///     n = error_nb.to_string(),
///     msg,
/// );
/// ```
///
/// To add [`Pill`]s to the toast, you can precede a type that implements
/// [`PillSource`] with `@`.
///
/// ```no_run
/// use gettextts::gettext;
/// use crate::toast;
/// use crate::session::{Room, User};
///
/// # let session = unimplemented!();
/// # let room_id = unimplemented!();
/// # let user_id = unimplemented!();
/// let room = Room::new(session, room_id);
/// let member = Member::new(room, user_id);
///
/// toast!(
///     widget,
///     gettext("Could not contact {user} in {room}"),
///     @user = member,
///     @room,
/// );
/// ```
///
/// For this macro to work, the widget must have one of these ancestors that can
/// show toasts:
///
/// - `ToastableDialog`
/// - `AdwPreferencesDialog`
/// - `AdwPreferencesWindow`
/// - `Window`
///
/// [`PillSource`]: crate::components::PillSource
#[macro_export]
macro_rules! toast {
    // Without vars, with or without a trailing comma.
    ($widget:expr, $message:expr $(,)?) => {
        {
            $crate::utils::toast::add_toast(
                $widget.upcast_ref(),
                adw::Toast::new($message.as_ref())
            );
        }
    };
    // With vars.
    ($widget:expr, $message:expr, $($tail:tt)+) => {
        {
            let (string_vars, pill_vars) = $crate::_toast_accum!([], [], $($tail)+);
            $crate::utils::toast::add_toast_with_vars(
                $widget.upcast_ref(),
                $message.as_ref(),
                &string_vars,
                &pill_vars.into()
            );
        }
    };
}

/// Macro to accumulate the variables passed to `toast!`.
///
/// Returns a `([(&str, String)],[(&str, Pill)])` tuple. The items in the first
/// array are `(var_name, var_value)` tuples, and the ones in the second array
/// are `(var_name, pill)` tuples.
#[doc(hidden)]
#[macro_export]
macro_rules! _toast_accum {
    // `var = val` syntax, without anything after.
    ([$($string_vars:tt)*], [$($pill_vars:tt)*], $var:ident = $val:expr) => {
        $crate::_toast_accum!([$($string_vars)*], [$($pill_vars)*], $var = $val,)
    };
    // `var = val` syntax, with a trailing comma or other vars after.
    ([$($string_vars:tt)*], [$($pill_vars:tt)*], $var:ident = $val:expr, $($tail:tt)*) => {
        $crate::_toast_accum!([$($string_vars)* (stringify!($var), $val.to_string()),], [$($pill_vars)*], $($tail)*)
    };
    // `var` syntax, with or without a trailing comma and other vars after.
    ([$($string_vars:tt)*], [$($pill_vars:tt)*], $var:ident $($tail:tt)*) => {
        $crate::_toast_accum!([$($string_vars)* (stringify!($var), $var.to_string()),], [$($pill_vars)*] $($tail)*)
    };
    // `@var = val` syntax, without anything after.
    ([$($string_vars:tt)*], [$($pill_vars:tt)*], @$var:ident = $val:expr) => {
        $crate::_toast_accum!([$($string_vars)*], [$($pill_vars)*], @$var = $val,)
    };
    // `@var = val` syntax, with a trailing comma or other vars after.
    ([$($string_vars:tt)*], [$($pill_vars:tt)*], @$var:ident = $val:expr, $($tail:tt)*) => {
        {
            use $crate::components::PillSourceExt;
            // We do not need to watch safety settings for pills, rooms will be watched
            // automatically.
            let pill: $crate::components::Pill = $val.to_pill($crate::components::AvatarImageSafetySetting::None, None);
            $crate::_toast_accum!([$($string_vars)*], [$($pill_vars)* (stringify!($var), pill),], $($tail)*)
        }
    };
    // `@var` syntax, with or without a trailing comma and other vars after.
    ([$($string_vars:tt)*], [$($pill_vars:tt)*], @$var:ident $($tail:tt)*) => {
        {
            use $crate::components::PillSourceExt;
            // We do not need to watch safety settings for pills, rooms will be watched
            // automatically.
            let pill: $crate::components::Pill = $var.to_pill($crate::components::AvatarImageSafetySetting::None, None);
            $crate::_toast_accum!([$($string_vars)*], [$($pill_vars)* (stringify!($var), pill),] $($tail)*)
        }
    };
    // No more vars, with or without trailing comma.
    ([$($string_vars:tt)*], [$($pill_vars:tt)*] $(,)?) => { ([$($string_vars)*], [$($pill_vars)*]) };
}

/// Add the given `AdwToast` to the ancestor of the given widget.
///
/// The widget must have one of these ancestors that can show toasts:
///
/// - `ToastableDialog`
/// - `AdwPreferencesDialog`
/// - `AdwPreferencesWindow`
/// - `Window`
pub(crate) fn add_toast(widget: &gtk::Widget, toast: adw::Toast) {
    if let Some(dialog) = widget
        .ancestor(ToastableDialog::static_type())
        .and_downcast::<ToastableDialog>()
    {
        dialog.add_toast(toast);
    } else if let Some(dialog) = widget
        .ancestor(adw::PreferencesDialog::static_type())
        .and_downcast::<adw::PreferencesDialog>()
    {
        dialog.add_toast(toast);
    } else if let Some(root) = widget.root() {
        // FIXME: AdwPreferencesWindow is deprecated but RoomDetails uses it.
        #[allow(deprecated)]
        if let Some(window) = root.downcast_ref::<adw::PreferencesWindow>() {
            use adw::prelude::PreferencesWindowExt;
            window.add_toast(toast);
        } else if let Some(window) = root.downcast_ref::<Window>() {
            window.add_toast(toast);
        } else {
            panic!("Trying to display a toast when the parent doesn't support it");
        }
    }
}

/// Add a toast with the given message and variables to the ancestor of the
/// given widget.
///
/// The widget must have one of these ancestors that can show toasts:
///
/// - `ToastableDialog`
/// - `AdwPreferencesDialog`
/// - `AdwPreferencesWindow`
/// - `Window`
pub(crate) fn add_toast_with_vars(
    widget: &gtk::Widget,
    message: &str,
    string_vars: &[(&str, String)],
    pill_vars: &HashMap<&str, Pill>,
) {
    let string_dict: Vec<_> = string_vars
        .iter()
        .map(|(key, val)| (*key, val.as_ref()))
        .collect();
    let message = freplace(message, &string_dict);

    let toast = if pill_vars.is_empty() {
        adw::Toast::new(&message)
    } else {
        let mut swapped_label = String::new();
        let mut widgets = Vec::with_capacity(pill_vars.len());
        let mut last_end = 0;

        // Find the locations of the pills in the message.
        let mut matches = pill_vars
            .keys()
            .flat_map(|key| {
                message
                    .match_indices(&format!("{{{key}}}"))
                    .map(|(start, _)| (start, *key))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        // Sort the locations, so we can insert the pills in the right order.
        matches.sort_unstable();

        for (start, key) in matches {
            swapped_label.push_str(&message[last_end..start]);
            swapped_label.push_str(LabelWithWidgets::PLACEHOLDER);
            last_end = start + key.len() + 2;
            widgets.push(
                pill_vars
                    .get(key)
                    .expect("match key should be in map")
                    .clone(),
            );
        }
        swapped_label.push_str(&message[last_end..message.len()]);

        let widget = LabelWithWidgets::new();
        widget.set_valign(gtk::Align::Center);
        widget.set_label_and_widgets(swapped_label, widgets);

        adw::Toast::builder().custom_title(&widget).build()
    };

    add_toast(widget, toast);
}
