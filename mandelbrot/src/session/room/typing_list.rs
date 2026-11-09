use gtk::{gio, glib, prelude::*, subclass::prelude::*};

use super::Member;

mod imp {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::TypingList)]
    pub struct TypingList {
        /// The list of members currently typing.
        members: RefCell<Vec<Member>>,
        /// Whether this list is empty.
        #[property(get, explicit_notify)]
        is_empty: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TypingList {
        const NAME: &'static str = "TypingList";
        type Type = super::TypingList;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for TypingList {}

    impl ListModelImpl for TypingList {
        fn item_type(&self) -> glib::Type {
            Member::static_type()
        }

        fn n_items(&self) -> u32 {
            self.members.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.members
                .borrow()
                .get(position as usize)
                .map(|member| member.clone().upcast())
        }
    }

    impl TypingList {
        /// Set whether the list is empty.
        fn set_is_empty(&self, is_empty: bool) {
            if self.is_empty.get() == is_empty {
                return;
            }

            self.is_empty.set(is_empty);
            self.obj().notify_is_empty();
        }

        /// Update this list with the given list of typing members.
        pub(super) fn update(&self, new_members: Vec<Member>) {
            if new_members.is_empty() {
                self.set_is_empty(true);

                return;
            }

            let (removed, added) = {
                let mut members = self.members.borrow_mut();
                let removed = members.len() as u32;
                let added = new_members.len() as u32;
                *members = new_members;
                (removed, added)
            };

            self.obj().items_changed(0, removed, added);
            self.set_is_empty(false);
        }
    }
}

glib::wrapper! {
    /// List of members that are currently typing.
    pub struct TypingList(ObjectSubclass<imp::TypingList>)
        @implements gio::ListModel;
}

impl TypingList {
    /// Construct a new empty `TypingList`.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Update this list with the given list of typing members.
    pub(super) fn update(&self, new_members: Vec<Member>) {
        self.imp().update(new_members);
    }
}

impl Default for TypingList {
    fn default() -> Self {
        Self::new()
    }
}
