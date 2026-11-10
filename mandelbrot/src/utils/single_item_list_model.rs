use gtk::{gio, glib, prelude::*, subclass::prelude::*};

mod imp {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::SingleItemListModel)]
    pub struct SingleItemListModel {
        /// The item contained by this model.
        #[property(get, set = Self::set_item, explicit_notify, nullable)]
        item: RefCell<Option<glib::Object>>,
        /// Whether the item is hidden.
        #[property(get, set = Self::set_is_hidden, explicit_notify)]
        is_hidden: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SingleItemListModel {
        const NAME: &'static str = "SingleItemListModel";
        type Type = super::SingleItemListModel;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for SingleItemListModel {}

    impl ListModelImpl for SingleItemListModel {
        fn item_type(&self) -> glib::Type {
            self.item
                .borrow()
                .as_ref()
                .map_or_else(glib::Object::static_type, glib::Object::type_)
        }

        fn n_items(&self) -> u32 {
            (!self.is_empty()).into()
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            if self.is_hidden.get() || position != 0 {
                return None;
            }

            self.item.borrow().clone().and_upcast()
        }
    }

    impl SingleItemListModel {
        /// Set the item contained by this model.
        fn set_item(&self, item: Option<glib::Object>) {
            if *self.item.borrow() == item {
                return;
            }

            let was_empty = self.is_empty();

            self.item.replace(item);
            self.obj().notify_item();

            self.notify_items_changed(was_empty);
        }

        /// Set whether the item is hidden.
        fn set_is_hidden(&self, hidden: bool) {
            if self.is_hidden.get() == hidden {
                return;
            }

            let was_empty = self.is_empty();

            self.is_hidden.set(hidden);
            self.obj().notify_is_hidden();

            if was_empty != self.is_empty() {
                self.notify_items_changed(was_empty);
            }
        }

        /// Whether this model is empty.
        fn is_empty(&self) -> bool {
            self.is_hidden.get() || self.item.borrow().is_none()
        }

        /// Notify that the number of items changed.
        fn notify_items_changed(&self, was_empty: bool) {
            let is_empty = self.is_empty();

            let removed = (!was_empty).into();
            let added = (!is_empty).into();
            self.obj().items_changed(0, removed, added);
        }
    }
}

glib::wrapper! {
    /// A list model that can contain at most a single item.
    pub struct SingleItemListModel(ObjectSubclass<imp::SingleItemListModel>)
        @implements gio::ListModel;
}

impl SingleItemListModel {
    /// Construct a new `SingleItemListModel` for the given item.
    pub fn new(item: Option<&impl IsA<glib::Object>>) -> Self {
        glib::Object::builder().property("item", item).build()
    }
}

impl Default for SingleItemListModel {
    fn default() -> Self {
        Self::new(None::<&glib::Object>)
    }
}
