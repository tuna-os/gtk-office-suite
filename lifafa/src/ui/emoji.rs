//! The emoji chooser of the one-line text fields (the subject, the search
//! fields, the names in Settings), opened at the cursor.
//!
//! GTK opens a field's chooser (Ctrl+., Ctrl+; or Insert Emoji in its menu)
//! pointing at the middle of the whole field, wherever the cursor is. It
//! takes the chooser stored on the field under `gtk-emoji-chooser`, making
//! one only when there is none, and pops it up as it is. So just before GTK
//! opens it, the app makes the field's chooser itself if it has none yet,
//! and points it at the cursor. A chooser is made on first use and kept by
//! its field, as GTK does: it builds a button for every emoji there is, so
//! one per field the focus merely passes through would be too many, and one
//! moved between fields never appears again (GTK 4.22).
//!
//! Multi-line text views place their own at the cursor already, and the
//! composer's body has its own (`rich_editor`, #348).

use gtk::glib;
use gtk::prelude::*;

/// GTK's key for a field's chooser (gtktext.c).
const KEY: &std::ffi::CStr = c"gtk-emoji-chooser";

/// Set up the text fields of every window, those open now and those still
/// to come. Not every window is the application's (Settings is not), so
/// this goes by GTK's list of all of them.
pub(crate) fn follow_all() {
    let windows = gtk::Window::toplevels();
    let each = |model: &gtk::gio::ListModel, from: u32, count: u32| {
        for i in from..from + count {
            if let Some(w) = model.item(i).and_downcast::<gtk::Window>() {
                follow(&w);
            }
        }
    };
    each(&windows, 0, windows.n_items());
    windows.connect_items_changed(move |model, pos, _, added| each(model, pos, added));
}

/// Have `window` set up each text field it focuses, once.
fn follow(window: &gtk::Window) {
    unsafe {
        if window.data::<bool>("hylki-emoji-follow").is_some() {
            return;
        }
        window.set_data("hylki-emoji-follow", true);
    }
    window.connect_focus_widget_notify(|w| {
        if let Some(text) = gtk::prelude::GtkWindowExt::focus(w).and_downcast::<gtk::Text>() {
            watch(&text);
        }
    });
}

/// Ready `text`'s chooser before each of GTK's two ways of opening it: its
/// keys (caught on the way down, before the field's own binding acts on
/// them) and its menu (a right-click, ahead of the menu's own gesture).
fn watch(text: &gtk::Text) {
    // Once per field.
    unsafe {
        if text.data::<bool>("hylki-emoji-watch").is_some() {
            return;
        }
        text.set_data("hylki-emoji-watch", true);
    }
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(|k, keyval, _, state| {
        use gtk::gdk::{Key, ModifierType};
        let mods = state
            & (ModifierType::CONTROL_MASK | ModifierType::SHIFT_MASK | ModifierType::ALT_MASK);
        if mods == ModifierType::CONTROL_MASK && matches!(keyval, Key::period | Key::semicolon) {
            if let Some(text) = k.widget().and_downcast::<gtk::Text>() {
                ready(&text);
            }
        }
        glib::Propagation::Proceed
    });
    text.add_controller(keys);
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_SECONDARY);
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    click.connect_pressed(|g, _, _, _| {
        if let Some(text) = g.widget().and_downcast::<gtk::Text>() {
            ready(&text);
        }
    });
    text.add_controller(click);
}

/// Whether a field takes emoji at all: not a password, a number or a PIN,
/// and not one that says so itself.
fn takes_emoji(text: &gtk::Text) -> bool {
    use gtk::{InputHints, InputPurpose};
    text.is_editable()
        && text.property::<bool>("visibility")
        && !text.input_hints().contains(InputHints::NO_EMOJI)
        && !matches!(
            text.input_purpose(),
            InputPurpose::Number
                | InputPurpose::Digits
                | InputPurpose::Phone
                | InputPurpose::Pin
                | InputPurpose::Password
        )
}

/// Make `text`'s chooser if it has none, and point it at the cursor.
fn ready(text: &gtk::Text) {
    if !takes_emoji(text) {
        return;
    }
    let chooser = stored(text).unwrap_or_else(|| make(text));
    let (strong, _) = text.compute_cursor_extents(text.position().max(0) as usize);
    chooser.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
        strong.x() as i32,
        strong.y() as i32,
        1,
        strong.height().max(1.0) as i32,
    )));
}

/// The chooser on `text`, whoever made it.
fn stored(text: &gtk::Text) -> Option<gtk::EmojiChooser> {
    use glib::translate::{FromGlibPtrNone, ToGlibPtr};
    let obj: *mut glib::gobject_ffi::GObject = text.upcast_ref::<glib::Object>().to_glib_none().0;
    let ptr = unsafe { glib::gobject_ffi::g_object_get_data(obj, KEY.as_ptr()) };
    if ptr.is_null() {
        return None;
    }
    let widget: gtk::Widget =
        unsafe { gtk::Widget::from_glib_none(ptr as *mut gtk::ffi::GtkWidget) };
    widget.downcast().ok()
}

/// A chooser for `text`, made and stored as GTK makes its own: parented to
/// the field, which unparents it when it goes, and stored with no
/// reference of its own. The emoji replaces the selection at the cursor.
fn make(text: &gtk::Text) -> gtk::EmojiChooser {
    use glib::translate::ToGlibPtr;
    let chooser = gtk::EmojiChooser::new();
    chooser.set_parent(text);
    let obj: *mut glib::gobject_ffi::GObject = text.upcast_ref::<glib::Object>().to_glib_none().0;
    let ptr: *mut gtk::ffi::GtkEmojiChooser = chooser.to_glib_none().0;
    unsafe { glib::gobject_ffi::g_object_set_data(obj, KEY.as_ptr(), ptr as glib::ffi::gpointer) };
    let weak = text.downgrade();
    chooser.connect_emoji_picked(move |_, emoji| {
        let Some(text) = weak.upgrade() else { return };
        text.delete_selection();
        let mut pos = text.position();
        text.insert_text(emoji, &mut pos);
        text.set_position(pos);
    });
    chooser
}
