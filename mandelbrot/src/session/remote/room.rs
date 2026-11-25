use std::{cell::RefCell, time::Duration};

use gtk::{glib, glib::clone, prelude::*, subclass::prelude::*};
use matrix_sdk::room_preview::RoomPreview;
use ruma::{
    OwnedMxcUri, OwnedRoomAliasId, OwnedRoomId,
    room::{JoinRuleSummary, RoomSummary},
};
use tracing::warn;

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
        fn set_join_rule(&self, join_rule: Option<&JoinRuleSummary>) {
            let can_knock = matches!(
                join_rule,
                Some(JoinRuleSummary::Knock | JoinRuleSummary::KnockRestricted(_))
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

        /// Set the room data from a room summary.
        pub(super) fn set_data(&self, data: RoomSummary) {
            self.set_fields(
                data.room_id,
                data.canonical_alias,
                data.name,
                data.topic,
                data.num_joined_members,
                Some(&data.join_rule),
                data.avatar_url,
            );
        }

        /// Set the room data from a room preview.
        fn set_preview(&self, preview: RoomPreview) {
            self.set_fields(
                preview.room_id,
                preview.canonical_alias,
                preview.name,
                preview.topic,
                preview.num_joined_members,
                preview.join_rule.as_ref(),
                preview.avatar_url,
            );
        }

        /// Set the room data from its individual fields.
        #[allow(clippy::too_many_arguments)]
        fn set_fields(
            &self,
            room_id: OwnedRoomId,
            canonical_alias: Option<OwnedRoomAliasId>,
            name: Option<String>,
            topic: Option<String>,
            num_joined_members: impl TryInto<u32>,
            join_rule: Option<&JoinRuleSummary>,
            avatar_url: Option<OwnedMxcUri>,
        ) {
            self.set_room_id(room_id);
            self.set_canonical_alias(canonical_alias);
            self.set_name(name.into_clean_string());
            self.set_topic(topic.into_clean_string());
            self.set_joined_members_count(num_joined_members.try_into().unwrap_or(u32::MAX));
            self.set_join_rule(join_rule);

            if let Some(image) = self.obj().avatar_data().image() {
                image.set_uri_and_info(avatar_url, None);
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
        ///
        /// The SDK tries the room summary endpoint first, then falls back to
        /// the room directory and the room state.
        pub(super) async fn load_data(&self) {
            let Some(session) = self.session.upgrade() else {
                self.last_request_time.take();
                return;
            };

            self.set_loading_state(LoadingState::Loading);

            let uri = self.uri();
            let client = session.client();
            let id = uri.id.clone();
            let via = uri.via.clone();

            let handle = spawn_tokio!(async move { client.get_room_preview(&id, via).await });

            let Some(result) = self.request_abort_handle.await_task(handle).await else {
                // The task was aborted, which means that the object was dropped.
                return;
            };

            match result {
                Ok(preview) => self.set_preview(preview),
                Err(error) => {
                    warn!("Could not get room preview for room `{}`: {error}", uri.id);
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
