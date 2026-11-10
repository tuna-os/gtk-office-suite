use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};

use gtk::{
    gio, glib,
    glib::{clone, closure_local},
    prelude::*,
    subclass::prelude::*,
};
use indexmap::IndexMap;
use matrix_sdk::sync::RoomUpdates;
use matrix_sdk_ui::eyeball_im::VectorDiff;
use ruma::{OwnedRoomId, OwnedRoomOrAliasId, OwnedServerName, RoomId, RoomOrAliasId, UserId};
use tracing::{error, warn};

mod metainfo;
mod room_info;

use self::metainfo::RoomListMetainfo;
pub use self::{metainfo::RoomMetainfo, room_info::RoomListRoomInfo};
use crate::{
    gettext_f,
    prelude::*,
    session::{Room, Session},
    spawn_tokio,
};

mod imp {
    use std::{cell::RefCell, sync::LazyLock};

    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::RoomList)]
    pub struct RoomList {
        /// The list of rooms.
        pub(super) list: RefCell<IndexMap<OwnedRoomId, Room>>,
        /// The list of rooms we are currently joining.
        pub(super) joining_rooms: RefCell<HashSet<OwnedRoomOrAliasId>>,
        /// The list of rooms that were upgraded and for which we have not
        /// joined the successor yet.
        tombstoned_rooms: RefCell<HashSet<OwnedRoomId>>,
        /// The current session.
        #[property(get, construct_only)]
        session: glib::WeakRef<Session>,
        /// The rooms metainfo that allow to restore this `RoomList` from its
        /// previous state.
        metainfo: RoomListMetainfo,
        /// A mirror of the entries of the SDK's `RoomListService` room list,
        /// when simplified sliding sync is used.
        ///
        /// It is only used to interpret the positional [`VectorDiff`]s
        /// received from the SDK, the rooms are then added to or removed from
        /// `list` by comparing the room membership before and after a batch of
        /// diffs, since the ordering of the sidebar is handled on top of this
        /// list.
        sliding_sync_entries: RefCell<Vec<OwnedRoomId>>,
        pub(super) get_wait_source: RefCell<Option<glib::SourceId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RoomList {
        const NAME: &'static str = "RoomList";
        type Type = super::RoomList;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for RoomList {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("joining-rooms-changed").build()]);
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();
            self.metainfo.set_room_list(&self.obj());
        }

        fn dispose(&self) {
            if let Some(source) = self.get_wait_source.take() {
                source.remove();
            }
        }
    }

    impl ListModelImpl for RoomList {
        fn item_type(&self) -> glib::Type {
            Room::static_type()
        }

        fn n_items(&self) -> u32 {
            self.list.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.list
                .borrow()
                .get_index(position as usize)
                .map(|(_, v)| v.upcast_ref::<glib::Object>())
                .cloned()
        }
    }

    impl RoomList {
        /// Get the room with the given room ID, if any.
        pub(super) fn get(&self, room_id: &RoomId) -> Option<Room> {
            self.list.borrow().get(room_id).cloned()
        }

        /// Whether this list contains the room with the given ID.
        fn contains(&self, room_id: &RoomId) -> bool {
            self.list.borrow().contains_key(room_id)
        }

        /// Remove the given room identifier from the rooms we are currently
        /// joining.
        fn remove_joining_room(&self, identifier: &RoomOrAliasId) {
            let removed = self.joining_rooms.borrow_mut().remove(identifier);

            if removed {
                self.obj().emit_by_name::<()>("joining-rooms-changed", &[]);
            }
        }

        /// Add the given room identified to the rooms we are currently joining.
        fn add_joining_room(&self, identifier: OwnedRoomOrAliasId) {
            let inserted = self.joining_rooms.borrow_mut().insert(identifier);

            if inserted {
                self.obj().emit_by_name::<()>("joining-rooms-changed", &[]);
            }
        }

        /// Remove the given room identifier from the rooms we are currently
        /// joining and replace it with the given room ID if the room is
        /// not in the list yet.
        fn remove_or_replace_joining_room(&self, identifier: &RoomOrAliasId, room_id: &RoomId) {
            {
                let mut joining_rooms = self.joining_rooms.borrow_mut();
                joining_rooms.remove(identifier);

                if !self.contains(room_id) {
                    joining_rooms.insert(room_id.to_owned().into());
                }
            }
            self.obj().emit_by_name::<()>("joining-rooms-changed", &[]);
        }

        /// Add a room that was tombstoned but for which we have not joined the
        /// successor yet.
        pub(super) fn add_tombstoned_room(&self, room_id: OwnedRoomId) {
            self.tombstoned_rooms.borrow_mut().insert(room_id);
        }

        /// Handle when items were added to the list.
        fn items_added(&self, added: usize) {
            let position = {
                let list = self.list.borrow();

                let position = list.len().saturating_sub(added);

                let mut tombstoned_rooms_to_remove = Vec::new();
                for (_room_id, room) in list.iter().skip(position) {
                    room.connect_room_forgotten(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |room| {
                            imp.remove(room.room_id());
                        }
                    ));

                    // Check if the new room is the successor to a tombstoned room.
                    if let Some(predecessor_id) = room.predecessor_id()
                        && self.tombstoned_rooms.borrow().contains(predecessor_id)
                        && let Some(room) = self.get(predecessor_id)
                    {
                        room.update_successor();
                        tombstoned_rooms_to_remove.push(predecessor_id.clone());
                    }
                }

                if !tombstoned_rooms_to_remove.is_empty() {
                    let mut tombstoned_rooms = self.tombstoned_rooms.borrow_mut();
                    for room_id in tombstoned_rooms_to_remove {
                        tombstoned_rooms.remove(&room_id);
                    }
                }

                position
            };

            self.obj().items_changed(position as u32, 0, added as u32);
        }

        /// Remove the room with the given ID.
        fn remove(&self, room_id: &RoomId) {
            let removed = self.list.borrow_mut().shift_remove_full(room_id);
            self.tombstoned_rooms.borrow_mut().remove(room_id);

            if let Some((position, ..)) = removed {
                self.obj().items_changed(position as u32, 1, 0);
            }
        }

        /// Load the list of rooms from the `Store`.
        pub(super) async fn load(&self) {
            let rooms = self.metainfo.load_rooms().await;
            let added = rooms.len();
            self.list.borrow_mut().extend(rooms);

            self.items_added(added);
        }

        /// Handle room updates received via sync.
        pub(super) fn handle_room_updates(&self, rooms: RoomUpdates) {
            let Some(session) = self.session.upgrade() else {
                return;
            };
            let client = session.client();

            let mut new_rooms = HashMap::new();

            for (room_id, left_room) in rooms.left {
                let room = if let Some(room) = self.get(&room_id) {
                    room
                } else if let Some(matrix_room) = client.get_room(&room_id) {
                    new_rooms
                        .entry(room_id.clone())
                        .or_insert_with(|| Room::new(&session, matrix_room, None))
                        .clone()
                } else {
                    warn!("Could not find left room {room_id}");
                    continue;
                };

                self.remove_joining_room((*room_id).into());
                room.handle_ambiguity_changes(left_room.ambiguity_changes.values());
            }

            for (room_id, joined_room) in rooms.joined {
                let room = if let Some(room) = self.get(&room_id) {
                    room
                } else if let Some(matrix_room) = client.get_room(&room_id) {
                    new_rooms
                        .entry(room_id.clone())
                        .or_insert_with(|| Room::new(&session, matrix_room, None))
                        .clone()
                } else {
                    warn!("Could not find joined room {room_id}");
                    continue;
                };

                self.remove_joining_room((*room_id).into());
                self.metainfo.watch_room(&room);
                room.handle_ambiguity_changes(joined_room.ambiguity_changes.values());
            }

            for (room_id, _invited_room) in rooms.invited {
                let room = if let Some(room) = self.get(&room_id) {
                    room
                } else if let Some(matrix_room) = client.get_room(&room_id) {
                    new_rooms
                        .entry(room_id.clone())
                        .or_insert_with(|| Room::new(&session, matrix_room, None))
                        .clone()
                } else {
                    warn!("Could not find invited room {room_id}");
                    continue;
                };

                self.remove_joining_room((*room_id).into());
                self.metainfo.watch_room(&room);
            }

            for (room_id, _knocked_room) in rooms.knocked {
                let room = if let Some(room) = self.get(&room_id) {
                    room
                } else if let Some(matrix_room) = client.get_room(&room_id) {
                    new_rooms
                        .entry(room_id.clone())
                        .or_insert_with(|| Room::new(&session, matrix_room, None))
                        .clone()
                } else {
                    warn!("Could not find knocked room {room_id}");
                    continue;
                };

                self.remove_joining_room((*room_id).into());
                self.metainfo.watch_room(&room);
            }

            if !new_rooms.is_empty() {
                let added = new_rooms.len();
                self.list.borrow_mut().extend(new_rooms);
                self.items_added(added);
            }
        }

        /// Handle a batch of diffs of the sliding sync room list entries.
        ///
        /// The diffs are applied to the local mirror of the SDK's entries,
        /// then the room membership of the mirror before and after the batch
        /// is compared to add or remove rooms. This ignores reorderings, since
        /// the ordering of the rooms is handled by the models on top of this
        /// list.
        pub(super) fn handle_sliding_sync_entries(&self, diff_list: Vec<VectorDiff<OwnedRoomId>>) {
            let Some(session) = self.session.upgrade() else {
                return;
            };
            let client = session.client();

            let (old_set, new_set) = {
                let mut entries = self.sliding_sync_entries.borrow_mut();
                let old_set = entries.iter().cloned().collect::<HashSet<_>>();

                for diff in diff_list {
                    Self::apply_sliding_sync_entries_diff(&mut entries, diff);
                }

                let new_set = entries.iter().cloned().collect::<HashSet<_>>();
                (old_set, new_set)
            };

            // Remove the rooms that are not in the entries anymore. This
            // should only happen when a room was removed from the store, other
            // removals are handled via the `room-forgotten` signal.
            for room_id in old_set.difference(&new_set) {
                if client.get_room(room_id).is_none() {
                    self.remove(room_id);
                }
            }

            // Add the new rooms.
            let mut new_rooms = HashMap::new();
            for room_id in new_set.difference(&old_set) {
                self.remove_joining_room((**room_id).into());

                if self.contains(room_id) {
                    // The room is already in the list, e.g. because it was
                    // restored from the store, and it is already watched.
                    continue;
                }

                let Some(matrix_room) = client.get_room(room_id) else {
                    warn!("Could not find room {room_id} from sliding sync entries");
                    continue;
                };

                let room = Room::new(&session, matrix_room, None);
                self.metainfo.watch_room(&room);
                new_rooms.insert(room_id.clone(), room);
            }

            if !new_rooms.is_empty() {
                let added = new_rooms.len();
                self.list.borrow_mut().extend(new_rooms);
                self.items_added(added);
            }
        }

        /// Apply a single sliding sync entries diff to the local mirror of the
        /// SDK's entries.
        fn apply_sliding_sync_entries_diff(
            entries: &mut Vec<OwnedRoomId>,
            diff: VectorDiff<OwnedRoomId>,
        ) {
            match diff {
                VectorDiff::Append { values } => {
                    entries.extend(values);
                }
                VectorDiff::Clear => {
                    entries.clear();
                }
                VectorDiff::PushFront { value } => {
                    entries.insert(0, value);
                }
                VectorDiff::PushBack { value } => {
                    entries.push(value);
                }
                VectorDiff::PopFront => {
                    if entries.is_empty() {
                        warn!("Could not pop front sliding sync entry: list is empty");
                    } else {
                        entries.remove(0);
                    }
                }
                VectorDiff::PopBack => {
                    if entries.pop().is_none() {
                        warn!("Could not pop back sliding sync entry: list is empty");
                    }
                }
                VectorDiff::Insert { index, value } => {
                    if index <= entries.len() {
                        entries.insert(index, value);
                    } else {
                        warn!("Could not insert sliding sync entry: index {index} out of bounds");
                        entries.push(value);
                    }
                }
                VectorDiff::Set { index, value } => {
                    if let Some(entry) = entries.get_mut(index) {
                        *entry = value;
                    } else {
                        warn!("Could not set sliding sync entry: index {index} out of bounds");
                        entries.push(value);
                    }
                }
                VectorDiff::Remove { index } => {
                    if index < entries.len() {
                        entries.remove(index);
                    } else {
                        warn!("Could not remove sliding sync entry: index {index} out of bounds");
                    }
                }
                VectorDiff::Truncate { length } => {
                    entries.truncate(length);
                }
                VectorDiff::Reset { values } => {
                    *entries = values.into_iter().collect();
                }
            }
        }

        /// Handle the ambiguity changes of the room updates received via sync.
        ///
        /// This is used with sliding sync, where the room membership is
        /// handled via the sliding sync entries instead.
        pub(super) fn handle_ambiguity_changes(&self, rooms: &RoomUpdates) {
            for (room_id, left_room) in &rooms.left {
                if let Some(room) = self.get(room_id) {
                    room.handle_ambiguity_changes(left_room.ambiguity_changes.values());
                }
            }

            for (room_id, joined_room) in &rooms.joined {
                if let Some(room) = self.get(room_id) {
                    room.handle_ambiguity_changes(joined_room.ambiguity_changes.values());
                }
            }
        }

        /// Join the room with the given identifier.
        pub(super) async fn join_by_id_or_alias(
            &self,
            identifier: OwnedRoomOrAliasId,
            via: Vec<OwnedServerName>,
        ) -> Result<OwnedRoomId, String> {
            let Some(session) = self.session.upgrade() else {
                return Err("Could not upgrade Session".to_owned());
            };
            let client = session.client();
            let identifier_clone = identifier.clone();

            self.add_joining_room(identifier.clone());

            let handle = spawn_tokio!(async move {
                client
                    .join_room_by_id_or_alias(&identifier_clone, &via)
                    .await
            });

            match handle.await.expect("task was not aborted") {
                Ok(matrix_room) => {
                    self.remove_or_replace_joining_room(&identifier, matrix_room.room_id());
                    Ok(matrix_room.room_id().to_owned())
                }
                Err(error) => {
                    self.remove_joining_room(&identifier);
                    error!("Joining room {identifier} failed: {error}");

                    let error = gettext_f(
                        // Translators: Do NOT translate the content between '{' and '}', this is a
                        // variable name.
                        "Could not join room {room_name}",
                        &[("room_name", identifier.as_str())],
                    );

                    Err(error)
                }
            }
        }

        /// Request an invite.
        pub(super) async fn knock(
            &self,
            identifier: OwnedRoomOrAliasId,
            via: Vec<OwnedServerName>,
        ) -> Result<OwnedRoomId, String> {
            let Some(session) = self.session.upgrade() else {
                return Err("Could not upgrade Session".to_owned());
            };
            let client = session.client();

            let identifier_clone = identifier.clone();
            let handle =
                spawn_tokio!(async move { client.knock(identifier_clone, None, via).await });

            match handle.await.expect("task was not aborted") {
                Ok(matrix_room) => Ok(matrix_room.room_id().to_owned()),
                Err(error) => {
                    error!("Invite request for room {identifier} failed: {error}");

                    let error = gettext_f(
                        // Translators: Do NOT translate the content between '{' and '}', this is a
                        // variable name.
                        "Could not request an invite to room {room_name}",
                        &[("room_name", identifier.as_str())],
                    );

                    Err(error)
                }
            }
        }
    }
}

glib::wrapper! {
    /// List of all rooms known by the user.
    ///
    /// This is the parent `GListModel` of the sidebar from which all other models
    /// are derived.
    ///
    /// The `RoomList` also takes care of, so called *pending rooms*, i.e.
    /// rooms the user requested to join, but received no response from the
    /// server yet.
    pub struct RoomList(ObjectSubclass<imp::RoomList>)
        @implements gio::ListModel;
}

impl RoomList {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }

    /// Load the list of rooms from the `Store`.
    pub(crate) async fn load(&self) {
        self.imp().load().await;
    }

    /// Get a snapshot of the rooms list.
    pub(crate) fn snapshot(&self) -> Vec<Room> {
        self.imp().list.borrow().values().cloned().collect()
    }

    /// Whether we are currently joining the room with the given identifier.
    pub(crate) fn is_joining_room(&self, identifier: &RoomOrAliasId) -> bool {
        self.imp().joining_rooms.borrow().contains(identifier)
    }

    /// Get the room with the given room ID, if any.
    pub(crate) fn get(&self, room_id: &RoomId) -> Option<Room> {
        self.imp().get(room_id)
    }

    /// Get the room with the given identifier, if any.
    pub(crate) fn get_by_identifier(&self, identifier: &RoomOrAliasId) -> Option<Room> {
        let room_alias = match <&RoomId>::try_from(identifier) {
            Ok(room_id) => return self.get(room_id),
            Err(room_alias) => room_alias,
        };

        let mut matches = self
            .imp()
            .list
            .borrow()
            .iter()
            .filter(|(_, room)| {
                // We don't want a room that is not joined, it might not be the proper room for
                // the given alias anymore.
                if !room.is_joined() {
                    return false;
                }

                let matrix_room = room.matrix_room();
                matrix_room.canonical_alias().as_deref() == Some(room_alias)
                    || matrix_room.alt_aliases().iter().any(|a| a == room_alias)
            })
            .map(|(room_id, room)| (room_id.clone(), room.clone()))
            .collect::<HashMap<_, _>>();

        if matches.len() <= 1 {
            return matches.into_values().next();
        }

        // The alias is shared between upgraded rooms. We want the latest room, so
        // filter out those that are predecessors.
        let predecessors = matches
            .values()
            .filter_map(|room| room.predecessor_id().cloned())
            .collect::<Vec<_>>();
        for room_id in predecessors {
            matches.remove(&room_id);
        }

        if matches.len() <= 1 {
            return matches.into_values().next();
        }

        // Ideally this should not happen, return the one with the latest activity.
        matches
            .into_values()
            .fold(None::<Room>, |latest_room, room| {
                latest_room
                    .filter(|r| r.latest_activity() >= room.latest_activity())
                    .or(Some(room))
            })
    }

    /// Wait till the room with the given ID becomes available.
    pub(crate) async fn get_wait(
        &self,
        room_id: &RoomId,
        timeout: Option<Duration>,
    ) -> Option<Room> {
        if let Some(room) = self.get(room_id) {
            return Some(room);
        }

        let imp = self.imp();
        let (sender, receiver) = futures_channel::oneshot::channel();

        let room_id = room_id.to_owned();
        let sender_cell = Rc::new(Cell::new(Some(sender)));

        let handler_id = self.connect_items_changed(clone!(
            #[strong]
            sender_cell,
            move |obj, _, _, _| {
                if let Some(room) = obj.get(&room_id)
                    && let Some(sender) = sender_cell.take()
                {
                    let _ = sender.send(Some(room));
                }
            }
        ));

        if let Some(timeout) = timeout {
            let get_wait_source = glib::timeout_add_local_once(timeout, move || {
                if let Some(sender) = sender_cell.take() {
                    let _ = sender.send(None);
                }
            });
            imp.get_wait_source.replace(Some(get_wait_source));
        }

        let room = receiver.await.ok().flatten();

        self.disconnect(handler_id);

        // Remove the source if we got a room.
        if let Some(source) = imp.get_wait_source.take().filter(|_| room.is_some()) {
            source.remove();
        }

        room
    }

    /// Get the joined room that is a direct chat with the user with the given
    /// ID.
    ///
    /// If several rooms are found, returns the room with the latest activity.
    pub(crate) fn direct_chat(&self, user_id: &UserId) -> Option<Room> {
        self.imp()
            .list
            .borrow()
            .values()
            .filter(|r| {
                // A joined room where the direct member is the given user.
                r.is_joined() && r.direct_member().as_ref().map(|m| &**m.user_id()) == Some(user_id)
            })
            // Take the room with the latest activity.
            .max_by(|x, y| x.latest_activity().cmp(&y.latest_activity()))
            .cloned()
    }

    /// Add a room that was tombstoned but for which we haven't joined the
    /// successor yet.
    pub(crate) fn add_tombstoned_room(&self, room_id: OwnedRoomId) {
        self.imp().add_tombstoned_room(room_id);
    }

    /// Handle room updates received via sync.
    pub(crate) fn handle_room_updates(&self, rooms: RoomUpdates) {
        self.imp().handle_room_updates(rooms);
    }

    /// Handle a batch of diffs of the sliding sync room list entries.
    pub(crate) fn handle_sliding_sync_entries(&self, diff_list: Vec<VectorDiff<OwnedRoomId>>) {
        self.imp().handle_sliding_sync_entries(diff_list);
    }

    /// Handle the ambiguity changes of the room updates received via sync.
    ///
    /// This is used with sliding sync, where the room membership is handled
    /// via the sliding sync entries instead.
    pub(crate) fn handle_ambiguity_changes(&self, rooms: &RoomUpdates) {
        self.imp().handle_ambiguity_changes(rooms);
    }

    /// Join the room with the given identifier.
    pub(crate) async fn join_by_id_or_alias(
        &self,
        identifier: OwnedRoomOrAliasId,
        via: Vec<OwnedServerName>,
    ) -> Result<OwnedRoomId, String> {
        self.imp().join_by_id_or_alias(identifier, via).await
    }

    /// Request an invite to the room with the given identifier.
    pub(crate) async fn knock(
        &self,
        identifier: OwnedRoomOrAliasId,
        via: Vec<OwnedServerName>,
    ) -> Result<OwnedRoomId, String> {
        self.imp().knock(identifier, via).await
    }

    /// Connect to the signal emitted when the list of rooms we are currently
    /// joining changed.
    pub fn connect_joining_rooms_changed<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "joining-rooms-changed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}
