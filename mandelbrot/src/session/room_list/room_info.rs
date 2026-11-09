use gtk::{glib, glib::clone, prelude::*, subclass::prelude::*};
use ruma::OwnedRoomOrAliasId;

use super::RoomList;
use crate::{session::Room, utils::BoundObject};

mod imp {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::RoomListRoomInfo)]
    pub struct RoomListRoomInfo {
        /// The room identifiers to watch.
        identifiers: RefCell<Vec<OwnedRoomOrAliasId>>,
        /// The room list to watch.
        #[property(get, set = Self::set_room_list, explicit_notify)]
        room_list: BoundObject<RoomList>,
        /// The local room matching the identifiers, if any.
        #[property(get)]
        local_room: glib::WeakRef<Room>,
        /// Whether we are currently joining the room.
        #[property(get)]
        is_joining: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RoomListRoomInfo {
        const NAME: &'static str = "RoomListRoomInfo";
        type Type = super::RoomListRoomInfo;
    }

    #[glib::derived_properties]
    impl ObjectImpl for RoomListRoomInfo {}

    impl RoomListRoomInfo {
        /// Set the room identifiers to watch.
        pub(super) fn set_identifiers(&self, identifiers: Vec<OwnedRoomOrAliasId>) {
            if *self.identifiers.borrow() == identifiers {
                return;
            }

            self.identifiers.replace(identifiers);

            self.update_local_room();
            self.update_is_joining();
        }

        /// Set the room identifiers to watch.
        pub(super) fn set_room_list(&self, room_list: RoomList) {
            if self.room_list.obj().is_some_and(|list| list == room_list) {
                return;
            }

            let items_changed_handler = room_list.connect_items_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _, _, _| {
                    imp.update_local_room();
                }
            ));
            let joining_rooms_handler = room_list.connect_joining_rooms_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_is_joining();
                }
            ));

            self.room_list.set(
                room_list,
                vec![items_changed_handler, joining_rooms_handler],
            );

            self.update_local_room();
            self.update_is_joining();
            self.obj().notify_room_list();
        }

        /// Update the local room matching this remote room.
        fn update_local_room(&self) {
            let Some(room_list) = self.room_list.obj() else {
                return;
            };

            let local_room = self
                .identifiers
                .borrow()
                .iter()
                .find_map(|identifier| room_list.get_by_identifier(identifier));

            self.set_local_room(local_room.as_ref());
        }

        /// Set the local room matching this remote room.
        fn set_local_room(&self, room: Option<&Room>) {
            let prev_room = self.local_room.upgrade();

            if prev_room.as_ref() == room {
                return;
            }

            self.local_room.set(room);
            self.obj().notify_local_room();
        }

        /// Update whether we are currently joining the room.
        fn update_is_joining(&self) {
            let Some(room_list) = self.room_list.obj() else {
                return;
            };

            let is_joining = self
                .identifiers
                .borrow()
                .iter()
                .any(|identifier| room_list.is_joining_room(identifier));

            self.set_is_joining(is_joining);
        }

        /// Set whether we are currently joining the room.
        fn set_is_joining(&self, is_joining: bool) {
            if self.is_joining.get() == is_joining {
                return;
            }

            self.is_joining.set(is_joining);
            self.obj().notify_is_joining();
        }
    }
}

glib::wrapper! {
    /// API to get information about the status of a room in a [`RoomList`].
    pub struct RoomListRoomInfo(ObjectSubclass<imp::RoomListRoomInfo>);
}

impl RoomListRoomInfo {
    /// Construct a new empty `RoomListRoomInfo`.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set the room identifiers to watch.
    pub(crate) fn set_identifiers(&self, identifiers: Vec<OwnedRoomOrAliasId>) {
        self.imp().set_identifiers(identifiers);
    }
}

impl Default for RoomListRoomInfo {
    fn default() -> Self {
        Self::new()
    }
}
