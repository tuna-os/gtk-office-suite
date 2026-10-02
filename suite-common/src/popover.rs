//! Popovers that are reused, not created and unparented on every opening
//! (#1192).
//!
//! The pattern this replaces built a new popover each time and unparented it
//! from an idle callback once it closed. Unparenting unrealizes the popover,
//! which drops its surface, but the window can still hold a widget inside it
//! as the pointer's target. The window's next layout then asks that target's
//! native for its surface to request a motion event, gets none, and GTK
//! 4.14's `gdk_surface_request_motion` writes through the null pointer. The
//! GUI stress campaign hit that in Tables' column menu whenever the pointer
//! happened to be over the menu as it closed.
//!
//! A popover that is only ever popped down keeps its surface, so the window
//! never sees one without. Each host keeps one popover per purpose, found by
//! name, and unparents it only when the host itself is destroyed.

use gtk4::{self as gtk, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;

/// The popover named `name` on `host`, created and parented on first use.
///
/// Callers set its child, position and other properties on every opening;
/// a different purpose on the same host uses a different name, so it never
/// inherits another's settings.
pub fn reused(host: &impl IsA<gtk::Widget>, name: &str) -> gtk::Popover {
    let host = host.as_ref();
    let mut child = host.first_child();
    while let Some(w) = child {
        if w.widget_name() == name {
            if let Ok(popover) = w.clone().downcast::<gtk::Popover>() {
                return popover;
            }
        }
        child = w.next_sibling();
    }
    let popover = gtk::Popover::new();
    popover.set_widget_name(name);
    popover.set_parent(host);
    let p = popover.clone();
    host.connect_destroy(move |_| p.unparent());
    popover
}

/// Run `f` the next time `popover` closes, and only then: a reused popover
/// keeps its handlers, so one connected per opening would run once for
/// every opening so far.
pub fn on_next_close(popover: &gtk::Popover, f: impl Fn() + 'static) {
    let id: Rc<RefCell<Option<gtk::glib::SignalHandlerId>>> = Rc::new(RefCell::new(None));
    let slot = id.clone();
    let handler = popover.connect_closed(move |p| {
        if let Some(id) = slot.borrow_mut().take() {
            p.disconnect(id);
        }
        f();
    });
    *id.borrow_mut() = Some(handler);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn one_popover_per_host_and_name_and_close_handlers_run_once() {
        crate::gtk_test::run(|| {
            // A host that, unlike a box, does not release its children
            // when disposed, so the release below is this module's.
            let host = gtk::DrawingArea::new();
            let a = reused(&host, "first");
            assert_eq!(a, reused(&host, "first"), "the same name finds the same popover");
            let b = reused(&host, "second");
            assert_ne!(a, b, "another name gets its own");
            let mut children = 0;
            let mut c = host.first_child();
            while let Some(w) = c {
                children += 1;
                c = w.next_sibling();
            }
            assert_eq!(children, 2);

            let ran = Rc::new(Cell::new(0));
            let r = ran.clone();
            on_next_close(&a, move || r.set(r.get() + 1));
            a.emit_by_name::<()>("closed", &[]);
            a.emit_by_name::<()>("closed", &[]);
            assert_eq!(ran.get(), 1, "a next-close handler runs once");

            // The host owned only by its window, as in the apps: destroying
            // the window disposes it, and that releases its popovers.
            let window = gtk::Window::new();
            window.set_child(Some(&host));
            drop(host);
            window.destroy();
            drop(window);
            assert!(a.parent().is_none() && b.parent().is_none(), "the host's destruction releases them");
        });
    }
}
