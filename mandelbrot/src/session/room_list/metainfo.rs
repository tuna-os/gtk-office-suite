use std::{
    cell::RefCell,
    collections::{BTreeMap, HashSet},
    ops::Deref,
    rc::Rc,
};

use futures_util::lock::Mutex;
use gtk::{glib, prelude::*};
use indexmap::IndexMap;
use ruma::OwnedRoomId;
use serde::{Deserialize, Serialize};
use tracing::error;

use super::RoomList;
use crate::{session::Room, spawn, spawn_tokio};

const ROOMS_METAINFO_KEY: &str = "rooms_metainfo";

/// The rooms metainfo that allow to restore the [`RoomList`] in its previous
/// state.
#[derive(Debug, Default, Clone)]
pub struct RoomListMetainfo(Rc<RoomListMetainfoInner>);

impl RoomListMetainfo {
    /// Set the parent `RoomList`.
    pub fn set_room_list(&self, room_list: &RoomList) {
        self.room_list.set(Some(room_list));
    }

    /// Load the rooms and their metainfo from the store.
    pub async fn load_rooms(&self) -> IndexMap<OwnedRoomId, Room> {
        let Some(session) = self.room_list().session() else {
            return IndexMap::new();
        };
        let client = session.client();

        // Load the serialized map from the store.
        let client_clone = client.clone();
        let handle = spawn_tokio!(async move {
            client_clone
                .state_store()
                .get_custom_value(ROOMS_METAINFO_KEY.as_bytes())
                .await
        });

        let mut rooms_metainfo: RoomsMetainfoMap = match handle.await.unwrap() {
            Ok(Some(value)) => match serde_json::from_slice(&value) {
                Ok(metainfo) => metainfo,
                Err(error) => {
                    error!("Could not deserialize rooms metainfo: {error}");
                    Default::default()
                }
            },
            Ok(None) => Default::default(),
            Err(error) => {
                error!("Could not load rooms metainfo: {error}");
                Default::default()
            }
        };

        // We need to acquire the lock now to make sure we have the full map before any
        // change happens and the map tries to be persisted.
        let mut rooms_metainfo_guard = self.rooms_metainfo.lock().await;

        // Restore rooms and listen to changes.
        let matrix_rooms = client.rooms();
        let mut rooms = IndexMap::with_capacity(matrix_rooms.len());

        for matrix_room in matrix_rooms {
            let room_id = matrix_room.room_id().to_owned();
            let metainfo = rooms_metainfo.remove(&room_id);

            let room = Room::new(&session, matrix_room, metainfo);

            self.watch_room(&room);

            if let Some(metainfo) = metainfo {
                rooms_metainfo_guard.insert(room_id.clone(), metainfo);
            }

            rooms.insert(room_id, room);
        }

        rooms
    }

    /// Watch the given room for metainfo changes.
    pub fn watch_room(&self, room: &Room) {
        let inner_weak = std::rc::Rc::<RoomListMetainfoInner>::downgrade(&self.0);
        room.connect_notify_local(None, move |room, param_spec| {
            if !matches!(param_spec.name(), "latest-activity" | "is-read") {
                return;
            }

            let inner_weak = inner_weak.clone();
            let room_id = room.room_id().to_owned();

            spawn!(async move {
                let Some(inner) = inner_weak.upgrade() else {
                    return;
                };

                inner.update_rooms_metainfo_for_room(room_id).await;
            });
        });
    }
}

impl Deref for RoomListMetainfo {
    type Target = RoomListMetainfoInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

type RoomsMetainfoMap = BTreeMap<OwnedRoomId, RoomMetainfo>;

#[derive(Debug, Default)]
pub struct RoomListMetainfoInner {
    /// The rooms metainfos.
    ///
    /// This is in a Mutex because persisting the data in the store is async and
    /// we only want one operation at a time.
    rooms_metainfo: Mutex<RoomsMetainfoMap>,
    /// Set of room IDs for which the metainfo should be updated.
    ///
    /// This list is kept to avoid queuing the same room several times in a row
    /// while we wait for the async operation to finish.
    pending_rooms_metainfo_updates: RefCell<HashSet<OwnedRoomId>>,
    /// The parent `RoomList`.
    room_list: glib::WeakRef<RoomList>,
}

impl RoomListMetainfoInner {
    /// The parent `RoomList`.
    fn room_list(&self) -> RoomList {
        self.room_list.upgrade().unwrap()
    }

    /// Persist the metainfo in the store.
    async fn persist(&self, rooms_metainfo: &RoomsMetainfoMap) {
        let Some(session) = self.room_list().session() else {
            return;
        };
        let value = match serde_json::to_vec(rooms_metainfo) {
            Ok(value) => value,
            Err(error) => {
                error!("Could not serialize rooms metainfo: {error}");
                return;
            }
        };

        let client = session.client();
        let handle = spawn_tokio!(async move {
            client
                .state_store()
                .set_custom_value(ROOMS_METAINFO_KEY.as_bytes(), value)
                .await
        });

        if let Err(error) = handle.await.unwrap() {
            error!("Could not store rooms metainfo: {error}");
        }
    }

    /// Update the room metainfo for the room with the given ID.
    async fn update_rooms_metainfo_for_room(&self, room_id: OwnedRoomId) {
        self.pending_rooms_metainfo_updates
            .borrow_mut()
            .insert(room_id);

        while !self.pending_rooms_metainfo_updates.borrow().is_empty() {
            if !self.try_update_rooms_metainfo().await {
                return;
            }
        }
    }

    /// Update the rooms metainfo if a lock can be acquired.
    ///
    /// Returns `true` if the lock could be acquired.
    async fn try_update_rooms_metainfo(&self) -> bool {
        let Some(mut rooms_metainfo_guard) = self.rooms_metainfo.try_lock() else {
            return false;
        };

        let room_ids = self.pending_rooms_metainfo_updates.take();

        if room_ids.is_empty() {
            return true;
        }

        let room_list = self.room_list();
        let mut has_changed = false;

        for (room, room_id) in room_ids
            .into_iter()
            .filter_map(|room_id| room_list.get(&room_id).map(|room| (room, room_id)))
        {
            let metainfo = rooms_metainfo_guard.entry(room_id).or_default();
            has_changed |= metainfo.update(&room);
        }

        if has_changed {
            self.persist(&rooms_metainfo_guard).await;
        }

        true
    }
}

/// The room metainfo that needs to be persisted in the state store .
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct RoomMetainfo {
    pub latest_activity: u64,
    pub is_read: bool,
}

impl RoomMetainfo {
    /// Update this `RoomMetainfo` for the given `Room`.
    ///
    /// Returns `true` if the data was updated.
    fn update(&mut self, room: &Room) -> bool {
        let mut has_changed = false;

        let latest_activity = room.latest_activity();
        if self.latest_activity != latest_activity {
            self.latest_activity = latest_activity;
            has_changed = true;
        }

        let is_read = room.is_read();
        if self.is_read != is_read {
            self.is_read = is_read;
            has_changed = true;
        }

        has_changed
    }
}
