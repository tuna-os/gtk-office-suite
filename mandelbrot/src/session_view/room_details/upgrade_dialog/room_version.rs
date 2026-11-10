use gtk::{glib, prelude::*, subclass::prelude::*};
use ruma::RoomVersionId;

mod imp {
    use std::{
        cell::{Cell, OnceCell},
        marker::PhantomData,
    };

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::RoomVersion)]
    pub struct RoomVersion {
        /// The ID of the version.
        id: OnceCell<RoomVersionId>,
        /// The ID of the version as a string.
        #[property(get = Self::id_string)]
        id_string: PhantomData<String>,
        /// Whether the version is stable.
        #[property(get, construct_only)]
        is_stable: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RoomVersion {
        const NAME: &'static str = "RoomUpgradeDialogRoomVersion";
        type Type = super::RoomVersion;
    }

    #[glib::derived_properties]
    impl ObjectImpl for RoomVersion {}

    impl RoomVersion {
        /// Set the ID of this version.
        pub(super) fn set_id(&self, id: RoomVersionId) {
            self.id.set(id).expect("id is uninitialized");
        }

        /// The ID of this version.
        pub(super) fn id(&self) -> &RoomVersionId {
            self.id.get().expect("id is initialized")
        }

        /// The ID of this version as a string.
        fn id_string(&self) -> String {
            self.id().to_string()
        }
    }
}

glib::wrapper! {
    /// A room version.
    pub struct RoomVersion(ObjectSubclass<imp::RoomVersion>);
}

impl RoomVersion {
    /// Constructs a new `RoomVersion`.
    pub fn new(id: RoomVersionId, is_stable: bool) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("is-stable", is_stable)
            .build();

        obj.imp().set_id(id);

        obj
    }

    /// The ID of this version.
    pub(crate) fn id(&self) -> &RoomVersionId {
        self.imp().id()
    }
}
