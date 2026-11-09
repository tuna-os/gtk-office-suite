//! One column of a single-line message row (#334): it asks for the same
//! width on every row, whatever its text and however bold, and gives way
//! as the others do when the list is too narrow for them all.
//!
//! A label alone cannot do this. Sized in characters it grows with bold
//! text, so unread rows fell out of line; sized in pixels its minimum is
//! that width, and a narrow list cut the row off at the date. In a bin the
//! natural width is the column's, the minimum is the label's own (an
//! ellipsis), and every row shares out a shortage the same way.

use adw::prelude::*;
use gtk::{glib, subclass::prelude::*};

mod imp {
    use super::*;
    use std::cell::Cell;

    #[derive(Default)]
    pub(crate) struct ColumnBin {
        /// The width asked for; 0 asks for no more than the minimum, for
        /// the column that takes whatever room the others leave, and a
        /// negative width asks for whatever the child does.
        pub(super) width: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ColumnBin {
        const NAME: &'static str = "HylkiColumnBin";
        type Type = super::ColumnBin;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for ColumnBin {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for ColumnBin {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = self.obj().first_child() else {
                return (0, 0, -1, -1);
            };
            let (min, nat, base_min, base_nat) = child.measure(orientation, for_size);
            if orientation == gtk::Orientation::Horizontal {
                let width = self.width.get();
                if width < 0 {
                    (min, nat, -1, -1)
                } else if width > 0 {
                    (min.min(width), width, -1, -1)
                } else {
                    (min, min, -1, -1)
                }
            } else {
                (min, nat, base_min, base_nat)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.obj().first_child() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}

glib::wrapper! {
    pub(crate) struct ColumnBin(ObjectSubclass<imp::ColumnBin>)
        @extends gtk::Widget;
}

impl ColumnBin {
    /// A column `width` pixels wide, or 0 for the one that takes the rest.
    pub(crate) fn new(width: i32) -> Self {
        let obj: Self = glib::Object::new();
        obj.imp().width.set(width);
        obj
    }

    /// Ask for `width` instead (see `width`): a column resized.
    pub(super) fn set_width(&self, width: i32) {
        if self.imp().width.replace(width) != width {
            self.queue_resize();
        }
    }

    /// The width asked for, as set (not the width given).
    pub(super) fn asked_width(&self) -> i32 {
        self.imp().width.get()
    }

    /// Hold `child`, or nothing.
    pub(crate) fn set_child(&self, child: Option<&impl IsA<gtk::Widget>>) {
        while let Some(old) = self.first_child() {
            old.unparent();
        }
        if let Some(child) = child {
            child.set_parent(self);
        }
    }
}
