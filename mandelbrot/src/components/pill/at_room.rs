use gettextrs::gettext;
use gtk::{glib, prelude::*, subclass::prelude::*};
use ruma::RoomId;

use crate::{components::PillSource, prelude::*, session::Room};

mod imp {
    use std::cell::OnceCell;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::AtRoom)]
    pub struct AtRoom {
        /// The room represented by this mention.
        #[property(get, set = Self::set_room, construct_only)]
        room: OnceCell<Room>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AtRoom {
        const NAME: &'static str = "AtRoom";
        type Type = super::AtRoom;
        type ParentType = PillSource;
    }

    #[glib::derived_properties]
    impl ObjectImpl for AtRoom {}

    impl PillSourceImpl for AtRoom {
        fn identifier(&self) -> String {
            gettext("Notify the whole room")
        }
    }

    impl AtRoom {
        /// Set the room represented by this mention.
        fn set_room(&self, room: Room) {
            let room = self.room.get_or_init(|| room);

            // Bind the avatar image so it always looks the same.
            room.avatar_data()
                .bind_property("image", &self.obj().avatar_data(), "image")
                .sync_create()
                .build();
        }

        /// The ID of the room represented by this mention.
        pub(super) fn room_id(&self) -> &RoomId {
            self.room
                .get()
                .expect("room should be initialized")
                .room_id()
        }
    }
}

glib::wrapper! {
    /// A helper `PillSource` to represent an `@room` mention.
    pub struct AtRoom(ObjectSubclass<imp::AtRoom>) @extends PillSource;
}

impl AtRoom {
    /// Constructs an `@room` mention for the given room.
    pub fn new(room: &Room) -> Self {
        glib::Object::builder()
            .property("display-name", "@room")
            .property("room", room)
            .build()
    }

    /// The ID of the room represented by this mention.
    pub fn room_id(&self) -> &RoomId {
        self.imp().room_id()
    }
}
