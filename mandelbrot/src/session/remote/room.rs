use std::{cell::RefCell, time::Duration};

use gtk::{glib, glib::clone, prelude::*, subclass::prelude::*};
use matrix_sdk::reqwest::StatusCode;
use ruma::{
    OwnedRoomAliasId, OwnedRoomId,
    api::client::{room::get_summary, space::get_hierarchy},
    assign,
    room::{JoinRuleSummary, RoomSummary},
    uint,
};
use tracing::{debug, warn};

use crate::{
    components::{AvatarImage, AvatarUriSource, PillSource},
    prelude::*,
    session::{RoomListRoomInfo, Session},
    spawn, spawn_tokio,
    utils::{AbortableHandle, LoadingState, matrix::MatrixRoomIdUri, string::linkify},
};

/// The time after which the data of a room is assumed to be stale.
///
/// This matches 1 day.
const DATA_VALIDITY_DURATION: Duration = Duration::from_hours(24);

mod imp {
    use std::{
        cell::{Cell, OnceCell},
        time::Instant,
    };

    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::RemoteRoom)]
    pub struct RemoteRoom {
        /// The current session.
        #[property(get, set = Self::set_session, construct_only)]
        session: glib::WeakRef<Session>,
        /// The Matrix URI of this room.
        uri: OnceCell<MatrixRoomIdUri>,
        /// The ID of this room.
        room_id: RefCell<Option<OwnedRoomId>>,
        /// The canonical alias of this room.
        canonical_alias: RefCell<Option<OwnedRoomAliasId>>,
        /// The name that is set for this room.
        ///
        /// This can be empty, the display name should be used instead in the
        /// interface.
        #[property(get)]
        name: RefCell<Option<String>>,
        /// The topic of this room.
        #[property(get)]
        topic: RefCell<Option<String>>,
        /// The linkified topic of this room.
        ///
        /// This is the string that should be used in the interface when markup
        /// is allowed.
        #[property(get)]
        topic_linkified: RefCell<Option<String>>,
        /// The number of joined members in the room.
        #[property(get)]
        joined_members_count: Cell<u32>,
        /// Whether we can knock on the room.
        #[property(get)]
        can_knock: Cell<bool>,
        /// The information about this room in the room list.
        #[property(get)]
        room_list_info: RoomListRoomInfo,
        /// The loading state.
        #[property(get, builder(LoadingState::default()))]
        loading_state: Cell<LoadingState>,
        /// The time of the last request.
        last_request_time: Cell<Option<Instant>>,
        request_abort_handle: AbortableHandle,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RemoteRoom {
        const NAME: &'static str = "RemoteRoom";
        type Type = super::RemoteRoom;
        type ParentType = PillSource;
    }

    #[glib::derived_properties]
    impl ObjectImpl for RemoteRoom {}

    impl PillSourceImpl for RemoteRoom {
        fn identifier(&self) -> String {
            self.uri().id.to_string()
        }
    }

    impl RemoteRoom {
        /// Set the current session.
        fn set_session(&self, session: &Session) {
            self.session.set(Some(session));

            self.obj().avatar_data().set_image(Some(AvatarImage::new(
                session,
                AvatarUriSource::Room,
                None,
                None,
            )));

            self.room_list_info.set_room_list(session.room_list());
        }

        /// Set the Matrix URI of this room.
        pub(super) fn set_uri(&self, uri: MatrixRoomIdUri) {
            if let Ok(room_id) = uri.id.clone().try_into() {
                self.set_room_id(room_id);
            }

            self.uri
                .set(uri)
                .expect("Matrix URI should be uninitialized");

            self.update_identifiers();
            self.update_display_name();
        }

        /// The Matrix URI of this room.
        pub(super) fn uri(&self) -> &MatrixRoomIdUri {
            self.uri.get().expect("Matrix URI should be initialized")
        }

        /// Set the ID of this room.
        fn set_room_id(&self, room_id: OwnedRoomId) {
            self.room_id.replace(Some(room_id));
        }

        /// The ID of this room.
        pub(super) fn room_id(&self) -> Option<OwnedRoomId> {
            self.room_id.borrow().clone()
        }

        /// Set the canonical alias of this room.
        fn set_canonical_alias(&self, alias: Option<OwnedRoomAliasId>) {
            if *self.canonical_alias.borrow() == alias {
                return;
            }

            self.canonical_alias.replace(alias);
            self.update_display_name();
        }

        /// The canonical alias of this room.
        pub(super) fn canonical_alias(&self) -> Option<OwnedRoomAliasId> {
            self.canonical_alias
                .borrow()
                .clone()
                .or_else(|| self.uri().id.clone().try_into().ok())
        }

        /// Update the identifiers to watch in the room list.
        fn update_identifiers(&self) {
            let id = self.uri().id.clone();
            let room_id = self
                .room_id()
                .filter(|room_id| room_id.as_str() != id.as_str())
                .map(Into::into);
            let canonical_alias = self
                .canonical_alias()
                .filter(|alias| alias.as_str() != id.as_str())
                .map(Into::into);

            let identifiers = room_id
                .into_iter()
                .chain(canonical_alias)
                .chain(Some(id))
                .collect();

            self.room_list_info.set_identifiers(identifiers);
        }

        /// Set the name of this room.
        fn set_name(&self, name: Option<String>) {
            if *self.name.borrow() == name {
                return;
            }

            self.name.replace(name);

            self.obj().notify_name();
            self.update_display_name();
        }

        /// The display name of this room.
        pub(super) fn update_display_name(&self) {
            let display_name = self
                .name
                .borrow()
                .clone()
                .or_else(|| {
                    self.canonical_alias
                        .borrow()
                        .as_ref()
                        .map(ToString::to_string)
                })
                .unwrap_or_else(|| self.identifier());

            self.obj().set_display_name(display_name);
        }

        /// Set the topic of this room.
        fn set_topic(&self, topic: Option<String>) {
            let topic =
                topic.filter(|s| !s.is_empty() && s.find(|c: char| !c.is_whitespace()).is_some());

            if *self.topic.borrow() == topic {
                return;
            }

            let topic_linkified = topic.as_deref().map(|t| {
                // Detect links.
                let mut s = linkify(t);
                // Remove trailing spaces.
                s.truncate_end_whitespaces();
                s
            });

            self.topic.replace(topic);
            self.topic_linkified.replace(topic_linkified);

            let obj = self.obj();
            obj.notify_topic();
            obj.notify_topic_linkified();
        }

        /// Set the number of joined members in the room.
        fn set_joined_members_count(&self, count: u32) {
            if self.joined_members_count.get() == count {
                return;
            }

            self.joined_members_count.set(count);
            self.obj().notify_joined_members_count();
        }

        /// Set the join rule of the room.
        fn set_join_rule(&self, join_rule: &JoinRuleSummary) {
            let can_knock = matches!(
                join_rule,
                JoinRuleSummary::Knock | JoinRuleSummary::KnockRestricted(_)
            );

            if self.can_knock.get() == can_knock {
                return;
            }

            self.can_knock.set(can_knock);
            self.obj().notify_can_knock();
        }

        /// Set the loading state.
        pub(super) fn set_loading_state(&self, loading_state: LoadingState) {
            if self.loading_state.get() == loading_state {
                return;
            }

            self.loading_state.set(loading_state);

            if loading_state == LoadingState::Error {
                // Reset the request time so we try it again the next time.
                self.last_request_time.take();
            }

            self.obj().notify_loading_state();
        }

        /// Set the room data.
        pub(super) fn set_data(&self, data: RoomSummary) {
            self.set_room_id(data.room_id);
            self.set_canonical_alias(data.canonical_alias);
            self.set_name(data.name.into_clean_string());
            self.set_topic(data.topic.into_clean_string());
            self.set_joined_members_count(data.num_joined_members.try_into().unwrap_or(u32::MAX));
            self.set_join_rule(&data.join_rule);

            if let Some(image) = self.obj().avatar_data().image() {
                image.set_uri_and_info(data.avatar_url, None);
            }

            self.update_identifiers();
            self.set_loading_state(LoadingState::Ready);
        }

        /// Whether the data of the room is considered to be stale.
        pub(super) fn is_data_stale(&self) -> bool {
            self.last_request_time
                .get()
                .is_none_or(|last_time| last_time.elapsed() > DATA_VALIDITY_DURATION)
        }

        /// Update the last request time to now.
        pub(super) fn update_last_request_time(&self) {
            self.last_request_time.set(Some(Instant::now()));
        }

        /// Request the data of this room.
        pub(super) async fn load_data(&self) {
            let Some(session) = self.session.upgrade() else {
                self.last_request_time.take();
                return;
            };

            self.set_loading_state(LoadingState::Loading);

            // Try to load data from the summary endpoint first, and if it is not supported
            // try the space hierarchy endpoint.
            if !self.load_data_from_summary(&session).await {
                self.load_data_from_space_hierarchy(&session).await;
            }
        }

        /// Load the data of this room using the room summary endpoint.
        ///
        /// At the time of writing this code, MSC3266 has been accepted but the
        /// endpoint is not part of a Matrix spec release.
        ///
        /// Returns `false` if the endpoint is not supported by the homeserver.
        async fn load_data_from_summary(&self, session: &Session) -> bool {
            let uri = self.uri();
            let client = session.client();

            let request = get_summary::v1::Request::new(uri.id.clone(), uri.via.clone());
            let handle = spawn_tokio!(async move { client.send(request).await });

            let Some(result) = self.request_abort_handle.await_task(handle).await else {
                // The task was aborted, which means that the object was dropped.
                return true;
            };

            match result {
                Ok(response) => {
                    self.set_data(response.summary);
                    true
                }
                Err(error) => {
                    if error
                        .as_client_api_error()
                        .is_some_and(|error| error.status_code == StatusCode::NOT_FOUND)
                    {
                        return false;
                    }

                    warn!(
                        "Could not get room details from summary endpoint for room `{}`: {error}",
                        uri.id
                    );
                    self.set_loading_state(LoadingState::Error);
                    true
                }
            }
        }

        /// Load the data of this room using the space hierarchy endpoint.
        ///
        /// This endpoint should work for any room already known by the
        /// homeserver.
        async fn load_data_from_space_hierarchy(&self, session: &Session) {
            let uri = self.uri();
            let client = session.client();

            // The endpoint only works with a room ID.
            let room_id = match OwnedRoomId::try_from(uri.id.clone()) {
                Ok(room_id) => room_id,
                Err(alias) => {
                    let client_clone = client.clone();
                    let handle =
                        spawn_tokio!(async move { client_clone.resolve_room_alias(&alias).await });

                    let Some(result) = self.request_abort_handle.await_task(handle).await else {
                        // The task was aborted, which means that the object was dropped.
                        return;
                    };

                    match result {
                        Ok(response) => response.room_id,
                        Err(error) => {
                            warn!("Could not resolve room alias `{}`: {error}", uri.id);
                            self.set_loading_state(LoadingState::Error);
                            return;
                        }
                    }
                }
            };

            let request = assign!(get_hierarchy::v1::Request::new(room_id.clone()), {
                // We are only interested in the single room.
                limit: Some(uint!(1))
            });
            let handle = spawn_tokio!(async move { client.send(request).await });

            let Some(result) = self.request_abort_handle.await_task(handle).await else {
                // The task was aborted, which means that the object was dropped.
                return;
            };

            match result {
                Ok(response) => {
                    if let Some(chunk) = response
                        .rooms
                        .into_iter()
                        .next()
                        .filter(|c| c.summary.room_id == room_id)
                    {
                        self.set_data(chunk.summary);
                    } else {
                        debug!("Space hierarchy endpoint did not return requested room");
                        self.set_loading_state(LoadingState::Error);
                    }
                }
                Err(error) => {
                    warn!(
                        "Could not get room details from space hierarchy endpoint for room `{}`: {error}",
                        uri.id
                    );
                    self.set_loading_state(LoadingState::Error);
                }
            }
        }
    }
}

glib::wrapper! {
    /// A Room that can only be updated by making remote calls, i.e. it won't be updated via sync.
    pub struct RemoteRoom(ObjectSubclass<imp::RemoteRoom>)
        @extends PillSource;
}

impl RemoteRoom {
    /// Construct a new `RemoteRoom` for the given URI, without any data.
    fn without_data(session: &Session, uri: MatrixRoomIdUri) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("session", session)
            .build();
        obj.imp().set_uri(uri);
        obj
    }

    /// Construct a new `RemoteRoom` for the given URI.
    ///
    /// This method automatically makes a request to load the room's data.
    pub(super) fn new(session: &Session, uri: MatrixRoomIdUri) -> Self {
        let obj = Self::without_data(session, uri);
        obj.load_data_if_stale();
        obj
    }

    /// Construct a new `RemoteRoom` for the given URI and data.
    pub(crate) fn with_data(
        session: &Session,
        uri: MatrixRoomIdUri,
        data: impl Into<RoomSummary>,
    ) -> Self {
        let obj = Self::without_data(session, uri);
        obj.imp().set_data(data.into());

        obj
    }

    /// The Matrix URI of this room.
    pub(crate) fn uri(&self) -> &MatrixRoomIdUri {
        self.imp().uri()
    }

    /// The ID of this room.
    pub(crate) fn room_id(&self) -> Option<OwnedRoomId> {
        self.imp().room_id()
    }

    /// The canonical alias of this room.
    pub(crate) fn canonical_alias(&self) -> Option<OwnedRoomAliasId> {
        self.imp().canonical_alias()
    }

    /// Load the data of this room if it is considered to be stale.
    pub(super) fn load_data_if_stale(&self) {
        let imp = self.imp();

        if !imp.is_data_stale() {
            // The data is still valid, nothing to do.
            return;
        }

        // Set the request time right away, to prevent several requests at the same
        // time.
        imp.update_last_request_time();

        spawn!(clone!(
            #[weak]
            imp,
            async move {
                imp.load_data().await;
            }
        ));
    }
}
