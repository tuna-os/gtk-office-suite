use gtk::{glib, prelude::*, subclass::prelude::*};

use crate::session::User;

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        marker::PhantomData,
    };

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::InviteItem)]
    pub struct InviteItem {
        /// The user data of the item.
        #[property(get, construct_only)]
        user: OnceCell<User>,
        /// Whether the user is invited.
        #[property(get, set = Self::set_is_invitee, explicit_notify)]
        is_invitee: Cell<bool>,
        /// Whether the user can be invited.
        #[property(get = Self::can_invite)]
        can_invite: PhantomData<bool>,
        /// The reason why the user cannot be invited, when applicable.
        #[property(get, set = Self::set_invite_exception, explicit_notify, nullable)]
        invite_exception: RefCell<Option<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for InviteItem {
        const NAME: &'static str = "RoomDetailsInviteItem";
        type Type = super::InviteItem;
    }

    #[glib::derived_properties]
    impl ObjectImpl for InviteItem {}

    impl InviteItem {
        /// Set whether this user is invited.
        fn set_is_invitee(&self, is_invitee: bool) {
            if self.is_invitee.get() == is_invitee {
                return;
            }

            self.is_invitee.set(is_invitee);
            self.obj().notify_is_invitee();
        }

        /// Whether the user can be invited.
        fn can_invite(&self) -> bool {
            self.invite_exception.borrow().is_none()
        }

        /// Set the reason the user can't be invited.
        fn set_invite_exception(&self, exception: Option<String>) {
            if exception == *self.invite_exception.borrow() {
                return;
            }

            let could_invite = self.can_invite();

            self.invite_exception.replace(exception);

            let obj = self.obj();
            obj.notify_invite_exception();

            if could_invite != self.can_invite() {
                obj.notify_can_invite();
            }
        }
    }
}

glib::wrapper! {
    /// An item of the result of a search in the user directory.
    ///
    /// This also keeps track whether the user is invited or the reason why they cannot be invited.
    pub struct InviteItem(ObjectSubclass<imp::InviteItem>);
}

impl InviteItem {
    /// Construct a new `InviteItem` with the given user.
    pub fn new(user: &impl IsA<User>) -> Self {
        glib::Object::builder().property("user", user).build()
    }
}
