use std::{cell::RefCell, fmt, rc::Rc};

use quick_cache::unsync::Cache;
use ruma::{OwnedRoomOrAliasId, OwnedUserId, RoomId};

use super::{RemoteRoom, RemoteUser};
use crate::{session::Session, utils::matrix::MatrixRoomIdUri};

/// The data of the [`RemoteCache`].
struct RemoteCacheData {
    /// Remote rooms.
    rooms: RefCell<Cache<OwnedRoomOrAliasId, RemoteRoom>>,
    /// Remote users.
    users: RefCell<Cache<OwnedUserId, RemoteUser>>,
}

/// An API to query remote data and cache it.
#[derive(Clone)]
pub(crate) struct RemoteCache {
    session: Session,
    data: Rc<RemoteCacheData>,
}

impl RemoteCache {
    /// Construct a new `RemoteCache` for the given session.
    pub(crate) fn new(session: Session) -> Self {
        Self {
            session,
            data: RemoteCacheData {
                rooms: Cache::new(30).into(),
                users: Cache::new(30).into(),
            }
            .into(),
        }
    }

    /// Get the remote room for the given URI.
    pub(crate) fn room(&self, uri: MatrixRoomIdUri) -> RemoteRoom {
        let mut rooms = self.data.rooms.borrow_mut();

        // Check if the room is in the cache.
        if let Some(room) = rooms.get(&uri.id) {
            room.load_data_if_stale();
            return room.clone();
        }

        // Check if the alias or ID matches a room in the cache, in case the URI uses
        // another ID than the one we used as a key for the cache.
        let mut found_id = None;
        let id_or_alias = <&RoomId>::try_from(&*uri.id);

        for (id, room) in rooms.iter() {
            match id_or_alias {
                Ok(room_id) => {
                    if room.room_id().is_some_and(|id| id == room_id) {
                        found_id = Some(id.clone());
                        break;
                    }
                }
                Err(room_alias) => {
                    if room
                        .canonical_alias()
                        .is_some_and(|alias| alias == room_alias)
                    {
                        found_id = Some(id.clone());
                        break;
                    }
                }
            }
        }

        if let Some(id) = found_id {
            let room = rooms.get(&id).expect("room should be in cache");
            room.load_data_if_stale();
            return room.clone();
        }

        // We did not find it, create the room.
        let id = uri.id.clone();
        let room = RemoteRoom::new(&self.session, uri);
        rooms.insert(id, room.clone());

        room
    }

    /// Get the remote user for the given ID.
    pub(crate) fn user(&self, user_id: OwnedUserId) -> RemoteUser {
        let mut users = self.data.users.borrow_mut();

        // Check if the user is in the cache.
        if let Some(user) = users.get(&user_id) {
            user.load_profile_if_stale();
            return user.clone();
        }

        // We did not find it, create the user.
        let user = RemoteUser::new(&self.session, user_id.clone());
        users.insert(user_id, user.clone());

        user
    }
}

impl fmt::Debug for RemoteCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteCache").finish_non_exhaustive()
    }
}
