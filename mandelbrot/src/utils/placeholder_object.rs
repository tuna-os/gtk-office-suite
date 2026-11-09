use gtk::{glib, prelude::*, subclass::prelude::*};

mod imp {
    use std::cell::RefCell;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::PlaceholderObject)]
    pub struct PlaceholderObject {
        /// The identifier of this item.
        #[property(get, set)]
        id: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PlaceholderObject {
        const NAME: &'static str = "PlaceholderObject";
        type Type = super::PlaceholderObject;
    }

    #[glib::derived_properties]
    impl ObjectImpl for PlaceholderObject {}
}

glib::wrapper! {
    /// A GObject to use as a placeholder.
    ///
    /// It can be used for example to add extra widgets in a list model and can
    /// be identified with its ID.
    pub struct PlaceholderObject(ObjectSubclass<imp::PlaceholderObject>);
}

impl PlaceholderObject {
    pub fn new(id: &str) -> Self {
        glib::Object::builder().property("id", id).build()
    }
}

impl PartialEq<str> for PlaceholderObject {
    /// Tests the equality between the `PlaceholderObject`'s ID and the given
    /// string.
    fn eq(&self, other: &str) -> bool {
        self.id().eq(other)
    }
}
