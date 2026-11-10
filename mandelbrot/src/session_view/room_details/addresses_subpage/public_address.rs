use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;
use ruma::OwnedRoomAliasId;

mod imp {
    use std::cell::{Cell, OnceCell};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::PublicAddress)]
    pub struct PublicAddress {
        /// The room alias.
        alias: OnceCell<OwnedRoomAliasId>,
        /// Whether this is the main address.
        #[property(get, set = Self::set_is_main, explicit_notify)]
        is_main: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PublicAddress {
        const NAME: &'static str = "RoomDetailsAddressesSubpagePublicAddress";
        type Type = super::PublicAddress;
    }

    #[glib::derived_properties]
    impl ObjectImpl for PublicAddress {}

    impl PublicAddress {
        /// Set the room alias.
        pub(super) fn set_alias(&self, alias: OwnedRoomAliasId) {
            self.alias.set(alias).expect("alias is uninitialized");
        }

        /// The room alias.
        pub(super) fn alias(&self) -> &OwnedRoomAliasId {
            self.alias.get().expect("alias is initialized")
        }

        /// Set whether this is the main address.
        fn set_is_main(&self, is_main: bool) {
            if self.is_main.get() == is_main {
                return;
            }

            self.is_main.set(is_main);
            self.obj().notify_is_main();
        }
    }
}

glib::wrapper! {
    /// A public address.
    pub struct PublicAddress(ObjectSubclass<imp::PublicAddress>);
}

impl PublicAddress {
    /// Constructs a new `PublicAddress`.
    pub fn new(alias: OwnedRoomAliasId, is_main: bool) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("is-main", is_main)
            .build();
        obj.imp().set_alias(alias);
        obj
    }

    /// The room alias.
    pub(crate) fn alias(&self) -> &OwnedRoomAliasId {
        self.imp().alias()
    }
}
