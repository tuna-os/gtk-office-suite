use std::{cell::RefCell, collections::HashSet};

use futures_util::StreamExt;
use gettextrs::gettext;
use gtk::{
    glib,
    glib::{clone, closure_local},
    prelude::*,
    subclass::prelude::*,
};
use matrix_sdk::{
    Result as MatrixResult, RoomDisplayName, RoomMemberships, RoomState,
    deserialized_responses::{AmbiguityChange, RawSyncOrStrippedState},
    event_handler::EventHandlerDropGuard,
    room::Room as MatrixRoom,
    send_queue::RoomSendQueueUpdate,
};
use ruma::{
    EventId, MatrixToUri, OwnedEventId, OwnedRoomId, OwnedUserId, RoomId, UserId,
    api::{
        client::receipt::create_receipt::v3::ReceiptType as ApiReceiptType,
        error::{ErrorKind, LimitExceededErrorData, RetryAfter},
    },
    events::room::{
        guest_access::GuestAccess,
        history_visibility::HistoryVisibility,
        member::{MembershipState, RoomMemberEventContent, SyncRoomMemberEvent},
    },
    room_version_rules::RoomVersionRules,
};
use serde::Deserialize;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{debug, error, warn};

mod aliases;
mod category;
mod highlight_flags;
mod join_rule;
mod member;
mod member_list;
mod permissions;
mod timeline;
mod typing_list;

pub(crate) use self::{
    aliases::{AddAltAliasError, RegisterLocalAliasError, RoomAliases},
    category::{RoomCategory, TargetRoomCategory},
    highlight_flags::HighlightFlags,
    join_rule::{JoinRule, JoinRuleValue},
    member::{Member, Membership},
    member_list::*,
    permissions::*,
    timeline::*,
    typing_list::TypingList,
};
use super::{
    IdentityVerification, Session, User, notifications::NotificationsRoomSetting,
    room_list::RoomMetainfo,
};
use crate::{
    components::{AtRoom, AvatarImage, AvatarUriSource, PillSource},
    gettext_f,
    prelude::*,
    spawn, spawn_tokio,
    utils::{BoundObjectWeakRef, string::linkify},
};

/// The default duration in seconds that we wait for before retrying failed
/// sending requests.
const DEFAULT_RETRY_AFTER: u32 = 30;

mod imp {
    use std::{
        cell::{Cell, OnceCell},
        marker::PhantomData,
        sync::LazyLock,
        time::SystemTime,
    };

    use glib::subclass::Signal;

    use super::*;

    mod info_sync;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::Room)]
    pub struct Room {
        /// The room API of the SDK.
        matrix_room: OnceCell<MatrixRoom>,
        /// The current session.
        #[property(get, set = Self::set_session, construct_only)]
        session: glib::WeakRef<Session>,
        /// The ID of this room, as a string.
        #[property(get = Self::room_id_string)]
        room_id_string: PhantomData<String>,
        /// The aliases of this room.
        #[property(get)]
        aliases: RoomAliases,
        /// The name that is set for this room.
        ///
        /// This can be empty, the display name should be used instead in the
        /// interface.
        #[property(get)]
        name: RefCell<Option<String>>,
        /// Whether this room has an avatar explicitly set.
        ///
        /// This is `false` if there is no avatar or if the avatar is the one
        /// from the other member.
        #[property(get)]
        has_avatar: Cell<bool>,
        /// The topic of this room.
        #[property(get)]
        topic: RefCell<Option<String>>,
        /// The linkified topic of this room.
        ///
        /// This is the string that should be used in the interface when markup
        /// is allowed.
        #[property(get)]
        topic_linkified: RefCell<Option<String>>,
        /// The category of this room.
        #[property(get, builder(RoomCategory::default()))]
        category: Cell<RoomCategory>,
        /// Whether this room is a direct chat.
        #[property(get)]
        is_direct: Cell<bool>,
        /// Whether this room has been upgraded.
        #[property(get)]
        is_tombstoned: Cell<bool>,
        /// The ID of the room that was upgraded and that this one replaces.
        pub(super) predecessor_id: OnceCell<OwnedRoomId>,
        /// The ID of the room that was upgraded and that this one replaces, as
        /// a string.
        #[property(get = Self::predecessor_id_string)]
        predecessor_id_string: PhantomData<Option<String>>,
        /// The ID of the successor of this Room, if this room was upgraded.
        pub(super) successor_id: OnceCell<OwnedRoomId>,
        /// The ID of the successor of this Room, if this room was upgraded, as
        /// a string.
        #[property(get = Self::successor_id_string)]
        successor_id_string: PhantomData<Option<String>>,
        /// The successor of this Room, if this room was upgraded and the
        /// successor was joined.
        #[property(get)]
        successor: glib::WeakRef<super::Room>,
        /// The members of this room.
        #[property(get)]
        pub(super) members: glib::WeakRef<MemberList>,
        members_drop_guard: OnceCell<EventHandlerDropGuard>,
        /// The number of joined members in the room, according to the
        /// homeserver.
        #[property(get)]
        joined_members_count: Cell<u64>,
        /// The member corresponding to our own user.
        #[property(get)]
        own_member: OnceCell<Member>,
        /// Whether this room is a current invite or an invite that was declined
        /// or retracted.
        #[property(get)]
        is_invite: Cell<bool>,
        /// The user who sent the invite to this room.
        ///
        /// This is only set when this room is an invitation.
        #[property(get)]
        inviter: RefCell<Option<Member>>,
        /// The other member of the room, if this room is a direct chat and
        /// there is only one other member.
        #[property(get)]
        direct_member: RefCell<Option<Member>>,
        /// The live timeline of this room.
        #[property(get)]
        live_timeline: OnceCell<Timeline>,
        /// The timestamp of the room's latest activity.
        ///
        /// This is the timestamp of the latest event that counts as possibly
        /// unread.
        ///
        /// If it is not known, it will return `0`.
        #[property(get)]
        latest_activity: Cell<u64>,
        /// Whether this room is marked as unread.
        #[property(get)]
        is_marked_unread: Cell<bool>,
        /// Whether all messages of this room are read.
        #[property(get)]
        is_read: Cell<bool>,
        /// The number of unread notifications of this room.
        #[property(get)]
        notification_count: Cell<u64>,
        /// whether this room has unread notifications.
        #[property(get)]
        has_notifications: Cell<bool>,
        /// The highlight state of the room.
        #[property(get)]
        highlight: Cell<HighlightFlags>,
        /// Whether this room is encrypted.
        #[property(get)]
        is_encrypted: Cell<bool>,
        /// The join rule of this room.
        #[property(get)]
        join_rule: JoinRule,
        /// Whether guests are allowed.
        #[property(get)]
        guests_allowed: Cell<bool>,
        /// The visibility of the history.
        #[property(get, builder(HistoryVisibilityValue::default()))]
        history_visibility: Cell<HistoryVisibilityValue>,
        /// The version of this room.
        #[property(get = Self::version)]
        version: PhantomData<String>,
        /// Whether this room is federated.
        #[property(get = Self::federated)]
        federated: PhantomData<bool>,
        /// The list of members currently typing in this room.
        #[property(get)]
        typing_list: TypingList,
        typing_drop_guard: OnceCell<EventHandlerDropGuard>,
        /// The notifications settings for this room.
        #[property(get, set = Self::set_notifications_setting, explicit_notify, builder(NotificationsRoomSetting::default()))]
        notifications_setting: Cell<NotificationsRoomSetting>,
        /// The permissions of our own user in this room
        #[property(get)]
        permissions: Permissions,
        /// An ongoing identity verification in this room.
        #[property(get, set = Self::set_verification, nullable, explicit_notify)]
        verification: BoundObjectWeakRef<IdentityVerification>,
        /// Whether the room info is initialized.
        ///
        /// Used to silence logs during initialization.
        #[property(get)]
        is_room_info_initialized: Cell<bool>,
        /// Whether we already attempted an auto-join.
        #[property(get)]
        attempted_auto_join: Cell<bool>,
        /// Whether this is a call room as defined by [MSC3417](https://github.com/matrix-org/matrix-spec-proposals/pull/3417)
        #[property(get = Self::is_call)]
        is_call: PhantomData<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Room {
        const NAME: &'static str = "Room";
        type Type = super::Room;
        type ParentType = PillSource;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Room {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("room-forgotten").build()]);
            SIGNALS.as_ref()
        }
    }

    impl PillSourceImpl for Room {
        fn identifier(&self) -> String {
            self.aliases
                .alias_string()
                .unwrap_or_else(|| self.room_id_string())
        }
    }

    impl Room {
        /// Initialize this room.
        pub(super) fn init(&self, matrix_room: MatrixRoom, metainfo: Option<RoomMetainfo>) {
            let obj = self.obj();

            self.matrix_room
                .set(matrix_room)
                .expect("matrix room is uninitialized");

            self.init_live_timeline();
            self.aliases.init(&obj);
            self.load_predecessor();
            self.watch_members();
            self.join_rule.init(&obj);
            self.set_up_typing();
            self.watch_send_queue();

            spawn!(
                glib::Priority::DEFAULT_IDLE,
                clone!(
                    #[weak(rename_to = imp)]
                    self,
                    async move {
                        imp.update_with_room_info(imp.matrix_room().clone_info())
                            .await;
                        imp.watch_room_info();

                        imp.is_room_info_initialized.set(true);
                        imp.obj().notify_is_room_info_initialized();

                        // Only initialize the following after we have loaded the category of the
                        // room since we only load them for some categories.

                        // Preload the timeline of rooms that the user is likely to visit and for
                        // which we offer to show the timeline.
                        let preload = matches!(
                            imp.category.get(),
                            RoomCategory::Favorite
                                | RoomCategory::Normal
                                | RoomCategory::LowPriority
                        );
                        imp.live_timeline().set_preload(preload);

                        imp.permissions.init(&imp.obj()).await;
                    }
                )
            );

            spawn!(
                glib::Priority::DEFAULT_IDLE,
                clone!(
                    #[weak(rename_to = imp)]
                    self,
                    async move {
                        imp.load_own_member().await;
                    }
                )
            );

            if let Some(RoomMetainfo {
                latest_activity,
                is_read,
            }) = metainfo
            {
                self.set_latest_activity(latest_activity);
                self.set_is_read(is_read);

                self.update_highlight();
            }
        }

        /// The room API of the SDK.
        pub(super) fn matrix_room(&self) -> &MatrixRoom {
            self.matrix_room.get().expect("matrix room was initialized")
        }

        /// Set the current session
        fn set_session(&self, session: &Session) {
            self.session.set(Some(session));

            let own_member = Member::new(&self.obj(), session.user_id().clone());
            self.own_member
                .set(own_member)
                .expect("own member was uninitialized");
        }

        /// The ID of this room.
        pub(super) fn room_id(&self) -> &RoomId {
            self.matrix_room().room_id()
        }

        /// The ID of this room, as a string.
        fn room_id_string(&self) -> String {
            self.matrix_room().room_id().to_string()
        }

        /// Update the name of this room.
        fn update_name(&self) {
            let name = self.matrix_room().name().into_clean_string();

            if *self.name.borrow() == name {
                return;
            }

            self.name.replace(name);
            self.obj().notify_name();
        }

        /// Load the display name from the SDK.
        async fn update_display_name(&self) {
            let matrix_room = self.matrix_room().clone();
            let handle = spawn_tokio!(async move { matrix_room.display_name().await });

            let sdk_display_name = handle
                .await
                .expect("task was not aborted")
                .inspect_err(|error| {
                    error!("Could not compute display name: {error}");
                })
                .ok();

            let mut display_name = if let Some(sdk_display_name) = sdk_display_name {
                match sdk_display_name {
                    RoomDisplayName::Named(s)
                    | RoomDisplayName::Calculated(s)
                    | RoomDisplayName::Aliased(s) => s,
                    RoomDisplayName::EmptyWas(s) => {
                        // Translators: This is the name of a room that is empty but had another
                        // user before. Do NOT translate the content between
                        // '{' and '}', this is a variable name.
                        gettext_f("Empty Room (was {user})", &[("user", &s)])
                    }
                    // Translators: This is the name of a room without other users.
                    RoomDisplayName::Empty => gettext("Empty Room"),
                }
            } else {
                Default::default()
            };

            display_name.clean_string();

            if display_name.is_empty() {
                // Translators: This is displayed when the room name is unknown yet.
                display_name = gettext("Unknown");
            }

            self.obj().set_display_name(display_name);
        }

        /// Set whether this room has an avatar explicitly set.
        fn set_has_avatar(&self, has_avatar: bool) {
            if self.has_avatar.get() == has_avatar {
                return;
            }

            self.has_avatar.set(has_avatar);
            self.obj().notify_has_avatar();
        }

        /// Update the avatar of the room.
        fn update_avatar(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let obj = self.obj();
            let avatar_data = obj.avatar_data();
            let matrix_room = self.matrix_room();

            let prev_avatar_url = avatar_data.image().and_then(|i| i.uri());
            let room_avatar_url = matrix_room.avatar_url();

            if prev_avatar_url.is_some() && prev_avatar_url == room_avatar_url {
                // The avatar did not change.
                return;
            }

            if let Some(avatar_url) = room_avatar_url {
                // The avatar has changed, update it.
                let avatar_info = matrix_room.avatar_info();

                if let Some(avatar_image) = avatar_data
                    .image()
                    .filter(|i| i.uri_source() == AvatarUriSource::Room)
                {
                    avatar_image.set_uri_and_info(Some(avatar_url), avatar_info);
                } else {
                    let avatar_image = AvatarImage::new(
                        &session,
                        AvatarUriSource::Room,
                        Some(avatar_url),
                        avatar_info,
                    );

                    avatar_data.set_image(Some(avatar_image.clone()));
                }

                self.set_has_avatar(true);
                return;
            }

            self.set_has_avatar(false);

            // If we have a direct member, use their avatar.
            if let Some(direct_member) = self.direct_member.borrow().as_ref() {
                avatar_data.set_image(direct_member.avatar_data().image());
            }

            let avatar_image = avatar_data.image();

            if let Some(avatar_image) = avatar_image
                .as_ref()
                .filter(|i| i.uri_source() == AvatarUriSource::Room)
            {
                // The room has no avatar, make sure we remove it.
                avatar_image.set_uri_and_info(None, None);
            } else if avatar_image.is_none() {
                // We always need an avatar image, even if it is empty.
                avatar_data.set_image(Some(AvatarImage::new(
                    &session,
                    AvatarUriSource::Room,
                    None,
                    None,
                )));
            }
        }

        /// Update the topic of this room.
        fn update_topic(&self) {
            let topic = self
                .matrix_room()
                .topic()
                .map(|mut s| {
                    s.strip_nul();
                    s.truncate_end_whitespaces();
                    s
                })
                .filter(|topic| !topic.is_empty());

            if *self.topic.borrow() == topic {
                return;
            }

            let topic_linkified = topic.as_ref().map(|t| {
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

        /// Set the category of this room.
        fn set_category(&self, category: RoomCategory) {
            let old_category = self.category.get();

            if old_category == RoomCategory::Outdated || old_category == category {
                return;
            }

            self.category.set(category);
            self.obj().notify_category();

            // Check if the previous state was different.
            let room_state = self.matrix_room().state();
            if !old_category.is_state(room_state) {
                if self.is_room_info_initialized.get() {
                    debug!(room_id = %self.room_id(), ?room_state, "The state of the room changed");
                }

                match room_state {
                    RoomState::Joined => {
                        if let Some(members) = self.members.upgrade() {
                            // If we where invited or left before, the list was likely not completed
                            // or might have changed.
                            members.reload();
                        }

                        self.set_up_typing();
                    }
                    RoomState::Left
                    | RoomState::Knocked
                    | RoomState::Banned
                    | RoomState::Invited => {}
                }
            }
        }

        /// Update the category from the SDK.
        pub(super) async fn update_category(&self) {
            // Do not load the category if this room was upgraded.
            if self.category.get() == RoomCategory::Outdated {
                return;
            }

            self.update_is_invite().await;
            self.update_inviter().await;

            let matrix_room = self.matrix_room();
            let state = matrix_room.state();

            // The state changed, reset the attempted auto-join.
            if state != RoomState::Invited {
                self.attempted_auto_join.take();
            }

            let category = match state {
                RoomState::Joined => {
                    if matrix_room.is_space() {
                        RoomCategory::Space
                    } else if matrix_room.is_favourite() {
                        RoomCategory::Favorite
                    } else if matrix_room.is_low_priority() {
                        RoomCategory::LowPriority
                    } else {
                        RoomCategory::Normal
                    }
                }
                RoomState::Invited => {
                    // Automatically accept invite that was after a knock.
                    if !self.attempted_auto_join.get()
                        && self.was_membership(&MembershipState::Knock).await
                    {
                        self.attempted_auto_join.set(true);

                        if self
                            .change_category(TargetRoomCategory::Normal)
                            .await
                            .is_ok()
                        {
                            // Wait for the next change to move automatically from knocked to
                            // joined.
                            return;
                        }
                    }

                    if self
                        .inviter
                        .borrow()
                        .as_ref()
                        .is_some_and(Member::is_ignored)
                    {
                        RoomCategory::Ignored
                    } else {
                        RoomCategory::Invited
                    }
                }
                RoomState::Knocked => RoomCategory::Knocked,
                RoomState::Left | RoomState::Banned => RoomCategory::Left,
            };

            self.set_category(category);
        }

        /// Set whether this room is a direct chat.
        async fn set_is_direct(&self, is_direct: bool) {
            if self.is_direct.get() == is_direct {
                return;
            }

            self.is_direct.set(is_direct);
            self.obj().notify_is_direct();

            self.update_direct_member().await;
        }

        /// Update whether the room is direct or not.
        pub(super) async fn update_is_direct(&self) {
            let matrix_room = self.matrix_room().clone();
            let handle = spawn_tokio!(async move { matrix_room.is_direct().await });

            match handle.await.expect("task was not aborted") {
                Ok(is_direct) => self.set_is_direct(is_direct).await,
                Err(error) => {
                    error!(room_id = %self.room_id(), "Could not load whether room is direct: {error}");
                }
            }
        }

        /// Update the tombstone for this room.
        fn update_tombstone(&self) {
            let matrix_room = self.matrix_room();

            if !matrix_room.is_tombstoned() || self.successor_id.get().is_some() {
                return;
            }
            let obj = self.obj();

            if let Some(successor_id) = matrix_room
                .tombstone_content()
                .and_then(|room_tombstone| room_tombstone.replacement_room)
            {
                self.successor_id
                    .set(successor_id)
                    .expect("successor ID should be uninitialized");
                obj.notify_successor_id_string();
            }

            // Try to get the successor.
            self.update_successor();

            // If the successor was not found, watch for it in the room list.
            if self.successor.upgrade().is_none()
                && let Some(session) = self.session.upgrade()
            {
                session
                    .room_list()
                    .add_tombstoned_room(self.room_id().to_owned());
            }

            if !self.is_tombstoned.get() {
                self.is_tombstoned.set(true);
                obj.notify_is_tombstoned();
            }
        }

        /// Update the successor of this room.
        pub(super) fn update_successor(&self) {
            if self.category.get() == RoomCategory::Outdated {
                return;
            }

            let Some(session) = self.session.upgrade() else {
                return;
            };
            let room_list = session.room_list();

            if let Some(successor) = self
                .successor_id
                .get()
                .and_then(|successor_id| room_list.get(successor_id))
            {
                // The Matrix spec says that we should use the "predecessor" field of the
                // m.room.create event of the successor, not the "successor" field of the
                // m.room.tombstone event, so check it just to be sure.
                if successor
                    .predecessor_id()
                    .is_some_and(|predecessor_id| predecessor_id == self.room_id())
                {
                    self.set_successor(&successor);
                    return;
                }
            }

            // The tombstone event can be redacted and we lose the successor, so search in
            // the room predecessors of other rooms.
            for room in room_list.iter::<super::Room>() {
                let Ok(room) = room else {
                    break;
                };

                if room
                    .predecessor_id()
                    .is_some_and(|predecessor_id| predecessor_id == self.room_id())
                {
                    self.set_successor(&room);
                    return;
                }
            }
        }

        /// The ID of the room that was upgraded and that this one replaces, as
        /// a string.
        fn predecessor_id_string(&self) -> Option<String> {
            self.predecessor_id.get().map(ToString::to_string)
        }

        /// Load the predecessor of this room.
        fn load_predecessor(&self) {
            let Some(event) = self.matrix_room().create_content() else {
                return;
            };
            let Some(predecessor) = event.predecessor else {
                return;
            };

            self.predecessor_id
                .set(predecessor.room_id)
                .expect("predecessor ID is uninitialized");
            self.obj().notify_predecessor_id_string();
        }

        /// The ID of the successor of this room, if this room was upgraded.
        fn successor_id_string(&self) -> Option<String> {
            self.successor_id.get().map(ToString::to_string)
        }

        /// Set the successor of this room.
        fn set_successor(&self, successor: &super::Room) {
            self.successor.set(Some(successor));
            self.obj().notify_successor();

            self.set_category(RoomCategory::Outdated);
        }

        /// Watch changes in the members list.
        fn watch_members(&self) {
            let matrix_room = self.matrix_room();

            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let handle = matrix_room.add_event_handler(move |event: SyncRoomMemberEvent| {
                let obj_weak = obj_weak.clone();
                async move {
                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            if let Some(obj) = obj_weak.upgrade() {
                                obj.imp().handle_member_event(&event);
                            }
                        });
                    });
                }
            });

            let drop_guard = matrix_room.client().event_handler_drop_guard(handle);
            self.members_drop_guard.set(drop_guard).unwrap();
        }

        /// Handle a member event received via sync
        fn handle_member_event(&self, event: &SyncRoomMemberEvent) {
            let user_id = event.state_key();

            if let Some(members) = self.members.upgrade() {
                members.update_member(user_id.clone());
            } else if user_id == self.own_member().user_id() {
                self.own_member().update();
            } else if let Some(member) = self
                .direct_member
                .borrow()
                .as_ref()
                .filter(|member| member.user_id() == user_id)
            {
                member.update();
            }

            // It might change the direct member if the number of members changed.
            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.update_direct_member().await;
                }
            ));
        }

        /// Set the number of joined members in the room, according to the
        /// homeserver.
        fn set_joined_members_count(&self, count: u64) {
            if self.joined_members_count.get() == count {
                return;
            }

            self.joined_members_count.set(count);
            self.obj().notify_joined_members_count();
        }

        /// The member corresponding to our own user.
        pub(super) fn own_member(&self) -> &Member {
            self.own_member.get().expect("Own member was initialized")
        }

        /// Load our own member from the store.
        async fn load_own_member(&self) {
            let own_member = self.own_member();
            let user_id = own_member.user_id().clone();
            let matrix_room = self.matrix_room().clone();

            let handle =
                spawn_tokio!(async move { matrix_room.get_member_no_sync(&user_id).await });

            match handle.await.expect("task was not aborted") {
                Ok(Some(matrix_member)) => own_member.update_from_room_member(&matrix_member),
                Ok(None) => {}
                Err(error) => error!(
                    "Could not load own member for room {}: {error}",
                    self.room_id()
                ),
            }
        }

        /// Update whether this room is a current invite or an invite that was
        /// declined or retracted.
        async fn update_is_invite(&self) {
            let matrix_room = self.matrix_room();

            let is_invite = match matrix_room.state() {
                RoomState::Invited => true,
                RoomState::Left | RoomState::Banned => {
                    self.was_membership(&MembershipState::Invite).await
                }
                _ => false,
            };

            if self.is_invite.get() == is_invite {
                return;
            }

            self.is_invite.set(is_invite);
            self.obj().notify_is_invite();
        }

        /// Check whether the previous membership of our user in this room
        /// matches the one that is given.
        async fn was_membership(&self, membership: &MembershipState) -> bool {
            let matrix_room = self.matrix_room();

            // To know if this was an invite we need to check in the member event of our own
            // user if the current membership is `invite`, or if the current membership is
            // `leave` or `ban`, and the previous membership was `invite`.
            let matrix_room_clone = matrix_room.clone();
            let handle = spawn_tokio!(async move {
                matrix_room_clone
                    .get_state_event_static_for_key::<RoomMemberEventContent, _>(
                        matrix_room_clone.own_user_id(),
                    )
                    .await
            });

            let raw_member_event = match handle.await.expect("task was not aborted") {
                Ok(Some(raw_member_event)) => raw_member_event,
                Ok(None) => {
                    return false;
                }
                Err(error) => {
                    error!("Could not get own member event: {error}");
                    return false;
                }
            };

            let member_event = match raw_member_event {
                RawSyncOrStrippedState::Sync(raw) => {
                    raw.deserialize_as_unchecked::<RoomMemberMembershipEvent>()
                }
                RawSyncOrStrippedState::Stripped(raw) => raw.deserialize_as_unchecked(),
            };

            let member_event = match member_event {
                Ok(member_event) => member_event,
                Err(error) => {
                    warn!("Could not deserialize room member event: {error}");
                    return false;
                }
            };

            // Check the current membership event, in case we did not get a state update
            // with the latest change.
            if member_event.content.membership == *membership {
                return true;
            }

            // Check the previous membership, in case we did get a state update with the
            // latest change.
            if let Some(prev_content) = member_event
                .unsigned
                .as_ref()
                .and_then(|unsigned| unsigned.prev_content.as_ref())
            {
                return prev_content.membership == *membership;
            }

            // If we do not have the `prev_content`, we need to fetch the previous state
            // event.
            let Some(replaces_state) = member_event
                .unsigned
                .and_then(|unsigned| unsigned.replaces_state)
            else {
                return false;
            };

            let matrix_room = matrix_room.clone();
            let handle = spawn_tokio!(async move {
                matrix_room.load_or_fetch_event(&replaces_state, None).await
            });

            let raw_prev_member_event = match handle.await.expect("task was not aborted") {
                Ok(event) => event,
                Err(error) => {
                    warn!("Could not fetch previous member event: {error}");
                    return false;
                }
            };

            match raw_prev_member_event
                .kind
                .raw()
                .deserialize_as_unchecked::<RoomMemberMembershipEvent>()
            {
                Ok(prev_member_event) => prev_member_event.content.membership == *membership,
                Err(error) => {
                    warn!("Could not deserialize previous member event: {error}");
                    false
                }
            }
        }

        /// Update the member that invited us to this room.
        async fn update_inviter(&self) {
            let matrix_room = self.matrix_room();

            // We are only interested in the inviter for current invites.
            if matrix_room.state() != RoomState::Invited {
                if self.inviter.take().is_some() {
                    self.obj().notify_inviter();
                }

                return;
            }

            let matrix_room = matrix_room.clone();
            let handle = spawn_tokio!(async move { matrix_room.invite_details().await });

            let invite = match handle.await.expect("task was not aborted") {
                Ok(invite) => invite,
                Err(error) => {
                    error!("Could not get invite: {error}");
                    return;
                }
            };

            let Some(inviter_member) = invite.inviter else {
                if self.inviter.take().is_some() {
                    self.obj().notify_inviter();
                }
                return;
            };

            if let Some(inviter) = self
                .inviter
                .borrow()
                .as_ref()
                .filter(|inviter| inviter.user_id() == inviter_member.user_id())
            {
                // Just update the member.
                inviter.update_from_room_member(&inviter_member);

                return;
            }

            let inviter = Member::new(&self.obj(), inviter_member.user_id().to_owned());
            inviter.update_from_room_member(&inviter_member);

            inviter
                .upcast_ref::<User>()
                .connect_is_ignored_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        spawn!(async move {
                            // When the user is ignored, this invite should be ignored too.
                            imp.update_category().await;
                        });
                    }
                ));

            self.inviter.replace(Some(inviter));

            self.obj().notify_inviter();
        }

        /// Set the other member of the room, if this room is a direct chat and
        /// there is only one other member.
        fn set_direct_member(&self, member: Option<Member>) {
            if *self.direct_member.borrow() == member {
                return;
            }

            self.direct_member.replace(member);
            self.obj().notify_direct_member();
            self.update_avatar();
        }

        /// The ID of the other user, if this is a direct chat and there is only
        /// one other user.
        async fn direct_user_id(&self) -> Option<OwnedUserId> {
            let matrix_room = self.matrix_room();

            // Check if the room is direct and if there is only one target.
            let mut direct_targets = matrix_room
                .direct_targets()
                .into_iter()
                .filter_map(|id| OwnedUserId::try_from(id).ok());

            let Some(direct_target_user_id) = direct_targets.next() else {
                // It is not a direct chat.
                return None;
            };

            if direct_targets.next().is_some() {
                // It is a direct chat with several users.
                return None;
            }

            // Check that there are still at most 2 members.
            let members_count = matrix_room.active_members_count();

            if members_count > 2 {
                // We only want a 1-to-1 room. The count might be 1 if the other user left, but
                // we can reinvite them.
                return None;
            }

            // Check that the members count is correct. It might not be correct if the room
            // was just joined, or if it is in an invited state.
            let matrix_room_clone = matrix_room.clone();
            let handle =
                spawn_tokio!(
                    async move { matrix_room_clone.members(RoomMemberships::ACTIVE).await }
                );

            let members = match handle.await.expect("task was not aborted") {
                Ok(m) => m,
                Err(error) => {
                    error!("Could not load room members: {error}");
                    vec![]
                }
            };

            let members_count = members_count.max(members.len() as u64);
            if members_count > 2 {
                // Same as before.
                return None;
            }

            let own_user_id = matrix_room.own_user_id();
            // Get the other member from the list.
            for member in members {
                let user_id = member.user_id();

                if user_id != direct_target_user_id && user_id != own_user_id {
                    // There is a non-direct member.
                    return None;
                }
            }

            Some(direct_target_user_id)
        }

        /// Update the other member of the room, if this room is a direct chat
        /// and there is only one other member.
        async fn update_direct_member(&self) {
            let Some(direct_user_id) = self.direct_user_id().await else {
                self.set_direct_member(None);
                return;
            };

            if self
                .direct_member
                .borrow()
                .as_ref()
                .is_some_and(|m| *m.user_id() == direct_user_id)
            {
                // Already up-to-date.
                return;
            }

            let direct_member = if let Some(members) = self.members.upgrade() {
                members.get_or_create(direct_user_id.clone())
            } else {
                Member::new(&self.obj(), direct_user_id.clone())
            };

            let matrix_room = self.matrix_room().clone();
            let handle =
                spawn_tokio!(async move { matrix_room.get_member_no_sync(&direct_user_id).await });

            match handle.await.expect("task was not aborted") {
                Ok(Some(matrix_member)) => {
                    direct_member.update_from_room_member(&matrix_member);
                }
                Ok(None) => {}
                Err(error) => {
                    error!("Could not get direct member: {error}");
                }
            }

            self.set_direct_member(Some(direct_member));
        }

        /// Initialize the live timeline of this room.
        fn init_live_timeline(&self) {
            let timeline = self
                .live_timeline
                .get_or_init(|| Timeline::new(&self.obj()));

            timeline.connect_read_change_trigger(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    spawn!(glib::Priority::DEFAULT_IDLE, async move {
                        imp.handle_read_change_trigger().await;
                    });
                }
            ));
        }

        /// The live timeline of this room.
        fn live_timeline(&self) -> &Timeline {
            self.live_timeline
                .get()
                .expect("live timeline is initialized")
        }

        /// Set the timestamp of the room's latest possibly unread event.
        pub(super) fn set_latest_activity(&self, latest_activity: u64) {
            if self.latest_activity.get() == latest_activity {
                return;
            }

            self.latest_activity.set(latest_activity);
            self.obj().notify_latest_activity();
        }

        /// Update whether this room is marked as unread.
        async fn update_is_marked_unread(&self) {
            let is_marked_unread = self.matrix_room().is_marked_unread();

            if self.is_marked_unread.get() == is_marked_unread {
                return;
            }

            self.is_marked_unread.set(is_marked_unread);
            self.handle_read_change_trigger().await;
            self.obj().notify_is_marked_unread();
        }

        /// Set whether all messages of this room are read.
        fn set_is_read(&self, is_read: bool) {
            if self.is_read.get() == is_read {
                return;
            }

            self.is_read.set(is_read);
            self.obj().notify_is_read();
        }

        /// Handle the trigger emitted when a read change might have occurred.
        async fn handle_read_change_trigger(&self) {
            let timeline = self.live_timeline();

            if self.is_marked_unread.get() {
                self.set_is_read(false);
            } else if let Some(has_unread) = timeline.has_unread_messages().await {
                self.set_is_read(!has_unread);
            }

            self.update_highlight();
        }

        /// Set how this room is highlighted.
        fn set_highlight(&self, highlight: HighlightFlags) {
            if self.highlight.get() == highlight {
                return;
            }

            self.highlight.set(highlight);
            self.obj().notify_highlight();
        }

        /// Update the highlight of the room from the current state.
        fn update_highlight(&self) {
            let mut highlight = HighlightFlags::empty();

            if matches!(self.category.get(), RoomCategory::Left) {
                // Consider that all left rooms are read.
                self.set_highlight(highlight);
                self.set_notification_count(0);
                return;
            }

            if self.is_read.get() {
                self.set_notification_count(0);
            } else {
                let counts = self.matrix_room().unread_notification_counts();

                if counts.highlight_count > 0 {
                    highlight = HighlightFlags::all();
                } else {
                    highlight = HighlightFlags::BOLD;
                }
                self.set_notification_count(counts.notification_count);
            }

            self.set_highlight(highlight);
        }

        /// Set the number of unread notifications of this room.
        fn set_notification_count(&self, count: u64) {
            if self.notification_count.get() == count {
                return;
            }

            self.notification_count.set(count);
            self.set_has_notifications(count > 0);
            self.obj().notify_notification_count();
        }

        /// Set whether this room has unread notifications.
        fn set_has_notifications(&self, has_notifications: bool) {
            if self.has_notifications.get() == has_notifications {
                return;
            }

            self.has_notifications.set(has_notifications);
            self.obj().notify_has_notifications();
        }

        /// Update whether the room is encrypted from the SDK.
        async fn update_is_encrypted(&self) {
            let matrix_room = self.matrix_room();
            let matrix_room_clone = matrix_room.clone();
            let handle =
                spawn_tokio!(async move { matrix_room_clone.latest_encryption_state().await });

            match handle.await.expect("task was not aborted") {
                Ok(state) => {
                    if state.is_encrypted() {
                        self.is_encrypted.set(true);
                        self.obj().notify_is_encrypted();
                    }
                }
                Err(error) => {
                    // It can be expected to not be allowed to access the encryption state if the
                    // user was never in the room, so do not add noise in the logs.
                    if matches!(matrix_room.state(), RoomState::Invited | RoomState::Knocked)
                        && error
                            .as_client_api_error()
                            .is_some_and(|e| e.status_code.is_client_error())
                    {
                        debug!("Could not load room encryption state: {error}");
                    } else {
                        error!("Could not load room encryption state: {error}");
                    }
                }
            }
        }

        /// Update whether guests are allowed.
        fn update_guests_allowed(&self) {
            let matrix_room = self.matrix_room();
            let guests_allowed = matrix_room.guest_access() == GuestAccess::CanJoin;

            if self.guests_allowed.get() == guests_allowed {
                return;
            }

            self.guests_allowed.set(guests_allowed);
            self.obj().notify_guests_allowed();
        }

        /// Update the visibility of the history.
        fn update_history_visibility(&self) {
            let matrix_room = self.matrix_room();
            let visibility = matrix_room.history_visibility_or_default().into();

            if self.history_visibility.get() == visibility {
                return;
            }

            self.history_visibility.set(visibility);
            self.obj().notify_history_visibility();
        }

        /// The version of this room.
        fn version(&self) -> String {
            self.matrix_room()
                .create_content()
                .map(|c| c.room_version.to_string())
                .unwrap_or_default()
        }

        /// If this is a Call room as defined by [MSC3417].
        ///
        /// [MSC3417]: <https://github.com/matrix-org/matrix-spec-proposals/pull/3417>
        fn is_call(&self) -> bool {
            self.matrix_room().is_call()
        }

        /// The rules for the version of this room.
        pub(super) fn rules(&self) -> RoomVersionRules {
            self.matrix_room()
                .clone_info()
                .room_version_rules_or_default()
        }

        /// Whether this room is federated.
        fn federated(&self) -> bool {
            self.matrix_room()
                .create_content()
                .is_some_and(|c| c.federate)
        }

        /// Start listening to typing events.
        fn set_up_typing(&self) {
            if self.typing_drop_guard.get().is_some() {
                // The event handler is already set up.
                return;
            }

            let matrix_room = self.matrix_room();
            if matrix_room.state() != RoomState::Joined {
                return;
            }

            let (typing_drop_guard, receiver) = matrix_room.subscribe_to_typing_notifications();
            let stream = BroadcastStream::new(receiver);

            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let fut = stream.for_each(move |typing_user_ids| {
                let obj_weak = obj_weak.clone();
                async move {
                    let Ok(typing_user_ids) = typing_user_ids else {
                        return;
                    };

                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            if let Some(obj) = obj_weak.upgrade() {
                                obj.imp().update_typing_list(typing_user_ids);
                            }
                        });
                    });
                }
            });
            spawn_tokio!(fut);

            self.typing_drop_guard
                .set(typing_drop_guard)
                .expect("typing drop guard is uninitialized");
        }

        /// Update the typing list with the given user IDs.
        fn update_typing_list(&self, typing_user_ids: Vec<OwnedUserId>) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let Some(members) = self.members.upgrade() else {
                // If we don't have a members list, the room is not shown so we don't need to
                // update the typing list.
                self.typing_list.update(vec![]);
                return;
            };

            let own_user_id = session.user_id();

            let members = typing_user_ids
                .into_iter()
                .filter(|user_id| user_id != own_user_id)
                .map(|user_id| members.get_or_create(user_id))
                .collect();

            self.typing_list.update(members);
        }

        /// Set the notifications setting for this room.
        fn set_notifications_setting(&self, setting: NotificationsRoomSetting) {
            if self.notifications_setting.get() == setting {
                return;
            }

            self.notifications_setting.set(setting);
            self.obj().notify_notifications_setting();
        }

        /// Set an ongoing verification in this room.
        fn set_verification(&self, verification: Option<IdentityVerification>) {
            if self.verification.obj().is_some() && verification.is_some() {
                // Just keep the same verification until it is dropped. Then we will look if
                // there is an ongoing verification in the room.
                return;
            }

            self.verification.disconnect_signals();

            let verification = verification.or_else(|| {
                // Look if there is an ongoing verification to replace it with.
                let room_id = self.matrix_room().room_id();
                self.session
                    .upgrade()
                    .map(|s| s.verification_list())
                    .and_then(|list| list.ongoing_room_verification(room_id))
            });

            if let Some(verification) = &verification {
                let state_handler = verification.connect_is_finished_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.set_verification(None);
                    }
                ));

                let dismiss_handler = verification.connect_dismiss(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.set_verification(None);
                    }
                ));

                self.verification
                    .set(verification, vec![state_handler, dismiss_handler]);
            }

            self.obj().notify_verification();
        }

        /// Handle changes in the ambiguity of members display names.
        pub(super) fn handle_ambiguity_changes<'a>(
            &self,
            changes: impl Iterator<Item = &'a AmbiguityChange>,
        ) {
            // Use a set to make sure we update members only once.
            let user_ids = changes
                .flat_map(AmbiguityChange::user_ids)
                .collect::<HashSet<_>>();

            if let Some(members) = self.members.upgrade() {
                for user_id in user_ids {
                    members.update_member(user_id.to_owned());
                }
            } else {
                let own_member = self.own_member();
                let own_user_id = own_member.user_id();

                if user_ids.contains(&**own_user_id) {
                    own_member.update();
                }
            }
        }

        /// Watch errors in the send queue to try to handle them.
        fn watch_send_queue(&self) {
            let matrix_room = self.matrix_room().clone();

            let room_weak = glib::SendWeakRef::from(self.obj().downgrade());
            spawn_tokio!(async move {
                let send_queue = matrix_room.send_queue();
                let subscriber = match send_queue.subscribe().await {
                    Ok((_, subscriber)) => BroadcastStream::new(subscriber),
                    Err(error) => {
                        warn!("Failed to listen to room send queue: {error}");
                        return;
                    }
                };

                subscriber
                    .for_each(move |update| {
                        let room_weak = room_weak.clone();
                        async move {
                            let Ok(RoomSendQueueUpdate::SendError {
                                error,
                                is_recoverable: true,
                                ..
                            }) = update
                            else {
                                return;
                            };

                            let ctx = glib::MainContext::default();
                            ctx.spawn(async move {
                                spawn!(async move {
                                    let Some(obj) = room_weak.upgrade() else {
                                        return;
                                    };
                                    let Some(session) = obj.session() else {
                                        return;
                                    };

                                    if session.is_offline() {
                                        // The queue will be restarted when the session is back
                                        // online.
                                        return;
                                    }

                                    let duration = match error.client_api_error_kind() {
                                        Some(ErrorKind::LimitExceeded(
                                            LimitExceededErrorData {
                                                retry_after: Some(retry_after),
                                                ..
                                            },
                                        )) => match retry_after {
                                            RetryAfter::Delay(duration) => Some(*duration),
                                            RetryAfter::DateTime(time) => {
                                                time.duration_since(SystemTime::now()).ok()
                                            }
                                        },
                                        _ => None,
                                    };
                                    let retry_after = duration
                                        .and_then(|d| d.as_secs().try_into().ok())
                                        .unwrap_or(DEFAULT_RETRY_AFTER);

                                    glib::timeout_add_seconds_local_once(retry_after, move || {
                                        let matrix_room = obj.matrix_room().clone();
                                        // Getting a room's send queue requires a tokio executor.
                                        spawn_tokio!(async move {
                                            matrix_room.send_queue().set_enabled(true);
                                        });
                                    });
                                });
                            });
                        }
                    })
                    .await;
            });
        }

        /// Change the category of this room.
        ///
        /// This makes the necessary to propagate the category to the
        /// homeserver.
        ///
        /// This can be used to trigger actions like join or leave, as well as
        /// changing the category in the sidebar.
        ///
        /// Note that rooms cannot change category once they are upgraded.
        pub(super) async fn change_category(
            &self,
            category: TargetRoomCategory,
        ) -> MatrixResult<()> {
            let previous_category = self.category.get();

            if previous_category == category {
                return Ok(());
            }

            if previous_category == RoomCategory::Outdated {
                warn!("Cannot change the category of an upgraded room");
                return Ok(());
            }

            self.set_category(category.into());

            let matrix_room = self.matrix_room().clone();
            let handle = spawn_tokio!(async move {
                let room_state = matrix_room.state();

                match category {
                    TargetRoomCategory::Favorite => {
                        if !matrix_room.is_favourite() {
                            // This method handles removing the low priority tag.
                            matrix_room.set_is_favourite(true, None).await?;
                        } else if matrix_room.is_low_priority() {
                            matrix_room.set_is_low_priority(false, None).await?;
                        }

                        if matches!(room_state, RoomState::Invited | RoomState::Left) {
                            matrix_room.join().await?;
                        }
                    }
                    TargetRoomCategory::Normal => {
                        if matrix_room.is_favourite() {
                            matrix_room.set_is_favourite(false, None).await?;
                        }
                        if matrix_room.is_low_priority() {
                            matrix_room.set_is_low_priority(false, None).await?;
                        }

                        if matches!(room_state, RoomState::Invited | RoomState::Left) {
                            matrix_room.join().await?;
                        }
                    }
                    TargetRoomCategory::LowPriority => {
                        if !matrix_room.is_low_priority() {
                            // This method handles removing the favourite tag.
                            matrix_room.set_is_low_priority(true, None).await?;
                        } else if matrix_room.is_favourite() {
                            matrix_room.set_is_favourite(false, None).await?;
                        }

                        if matches!(room_state, RoomState::Invited | RoomState::Left) {
                            matrix_room.join().await?;
                        }
                    }
                    TargetRoomCategory::Left => {
                        if matches!(
                            room_state,
                            RoomState::Knocked | RoomState::Invited | RoomState::Joined
                        ) {
                            matrix_room.leave().await?;
                        }
                    }
                }

                Result::<_, matrix_sdk::Error>::Ok(())
            });

            match handle.await.expect("task was not aborted") {
                Ok(()) => Ok(()),
                Err(error) => {
                    error!("Could not set the room category: {error}");

                    // Reset the category
                    Box::pin(self.update_category()).await;

                    Err(error)
                }
            }
        }
    }
}

glib::wrapper! {
    /// GObject representation of a Matrix room.
    ///
    /// Handles populating the Timeline.
    pub struct Room(ObjectSubclass<imp::Room>) @extends PillSource;
}

impl Room {
    /// Create a new `Room` for the given session, with the given room API.
    pub fn new(session: &Session, matrix_room: MatrixRoom, metainfo: Option<RoomMetainfo>) -> Self {
        let this = glib::Object::builder::<Self>()
            .property("session", session)
            .build();

        this.imp().init(matrix_room, metainfo);
        this
    }

    /// The room API of the SDK.
    pub(crate) fn matrix_room(&self) -> &MatrixRoom {
        self.imp().matrix_room()
    }

    /// The ID of this room.
    pub(crate) fn room_id(&self) -> &RoomId {
        self.imp().room_id()
    }

    /// Get a human-readable ID for this `Room`.
    ///
    /// This shows the display name and room ID to identify the room easily in
    /// logs.
    pub fn human_readable_id(&self) -> String {
        format!("{} ({})", self.display_name(), self.room_id())
    }

    /// The rules for the version of this room.
    pub(crate) fn rules(&self) -> RoomVersionRules {
        self.imp().rules()
    }

    /// Whether this room is joined.
    pub(crate) fn is_joined(&self) -> bool {
        self.own_member().membership() == Membership::Join
    }

    /// The ID of the predecessor of this room, if this room is an upgrade to a
    /// previous room.
    pub(crate) fn predecessor_id(&self) -> Option<&OwnedRoomId> {
        self.imp().predecessor_id.get()
    }

    /// The ID of the successor of this Room, if this room was upgraded.
    pub(crate) fn successor_id(&self) -> Option<&OwnedRoomId> {
        self.imp().successor_id.get()
    }

    /// The `matrix.to` URI representation for this room.
    pub(crate) async fn matrix_to_uri(&self) -> MatrixToUri {
        let matrix_room = self.matrix_room().clone();

        let handle = spawn_tokio!(async move { matrix_room.matrix_to_permalink().await });
        match handle.await.expect("task was not aborted") {
            Ok(permalink) => {
                return permalink;
            }
            Err(error) => {
                error!("Could not get room event permalink: {error}");
            }
        }

        // Fallback to using just the room ID, without routing.
        self.room_id().matrix_to_uri()
    }

    /// The `matrix.to` URI representation for the given event in this room.
    pub(crate) async fn matrix_to_event_uri(&self, event_id: OwnedEventId) -> MatrixToUri {
        let matrix_room = self.matrix_room().clone();

        let event_id_clone = event_id.clone();
        let handle =
            spawn_tokio!(
                async move { matrix_room.matrix_to_event_permalink(event_id_clone).await }
            );
        match handle.await.expect("task was not aborted") {
            Ok(permalink) => {
                return permalink;
            }
            Err(error) => {
                error!("Could not get room event permalink: {error}");
            }
        }

        // Fallback to using just the room ID, without routing.
        self.room_id().matrix_to_event_uri(event_id)
    }

    /// Constructs an `AtRoom` for this room.
    pub(crate) fn at_room(&self) -> AtRoom {
        AtRoom::new(self)
    }

    /// Get or create the list of members of this room.
    ///
    /// This creates the [`MemberList`] if no strong reference to it exists.
    pub(crate) fn get_or_create_members(&self) -> MemberList {
        let members = &self.imp().members;
        if let Some(list) = members.upgrade() {
            list
        } else {
            let list = MemberList::new(self);
            members.set(Some(&list));
            self.notify_members();
            list
        }
    }

    /// Change the category of this room.
    ///
    /// This makes the necessary to propagate the category to the homeserver.
    ///
    /// This can be used to trigger actions like join or leave, as well as
    /// changing the category in the sidebar.
    ///
    /// Note that rooms cannot change category once they are upgraded.
    pub(crate) async fn change_category(&self, category: TargetRoomCategory) -> MatrixResult<()> {
        self.imp().change_category(category).await
    }

    /// Toggle the `key` reaction on the given related event in this room.
    pub(crate) async fn toggle_reaction(&self, key: String, event: &Event) -> Result<(), ()> {
        // Use the timeline of the event, so that reactions also work on
        // events that are only in a thread timeline.
        let matrix_timeline = event.timeline().matrix_timeline();
        let identifier = event.identifier();

        let handle =
            spawn_tokio!(async move { matrix_timeline.toggle_reaction(&identifier, &key).await });

        if let Err(error) = handle.await.expect("task was not aborted") {
            error!("Could not toggle reaction: {error}");
            return Err(());
        }

        Ok(())
    }

    /// Send the given receipt.
    ///
    /// This will also unmark the room as unread.
    pub(crate) async fn send_receipt(
        &self,
        receipt_type: ApiReceiptType,
        position: ReceiptPosition,
    ) {
        let Some(session) = self.session() else {
            return;
        };
        let send_public_receipt = session.settings().public_read_receipts_enabled();

        let receipt_type = match receipt_type {
            ApiReceiptType::Read if !send_public_receipt => ApiReceiptType::ReadPrivate,
            t => t,
        };

        let matrix_timeline = self.live_timeline().matrix_timeline();
        let handle = spawn_tokio!(async move {
            match position {
                ReceiptPosition::End => matrix_timeline.mark_as_read(receipt_type).await,
                ReceiptPosition::Event(event_id) => {
                    matrix_timeline
                        .send_single_receipt(receipt_type, event_id)
                        .await
                }
            }
        });

        if let Err(error) = handle.await.expect("task was not aborted") {
            error!("Could not send read receipt: {error}");
        }
    }

    /// Mark the room as unread.
    pub(crate) async fn mark_as_unread(&self) {
        let matrix_room = self.matrix_room().clone();
        let handle = spawn_tokio!(async move { matrix_room.set_unread_flag(true).await });

        if let Err(error) = handle.await.expect("task was not aborted") {
            error!("Could not mark room as unread: {error}");
        }
    }

    /// Send a typing notification for this room, with the given typing state.
    pub(crate) fn send_typing_notification(&self, is_typing: bool) {
        let matrix_room = self.matrix_room();
        if matrix_room.state() != RoomState::Joined {
            return;
        }

        let matrix_room = matrix_room.clone();
        let handle = spawn_tokio!(async move { matrix_room.typing_notice(is_typing).await });

        spawn!(glib::Priority::DEFAULT_IDLE, async move {
            match handle.await.expect("task was not aborted") {
                Ok(()) => {}
                Err(error) => error!("Could not send typing notification: {error}"),
            }
        });
    }

    /// Redact the given events in this room because of the given reason.
    ///
    /// Returns `Ok(())` if all the redactions are successful, otherwise
    /// returns the list of events that could not be redacted.
    pub(crate) async fn redact<'a>(
        &self,
        events: &'a [OwnedEventId],
        reason: Option<String>,
    ) -> Result<(), Vec<&'a EventId>> {
        let matrix_room = self.matrix_room();
        if matrix_room.state() != RoomState::Joined {
            return Ok(());
        }

        let events_clone = events.to_owned();
        let matrix_room = matrix_room.clone();
        let handle = spawn_tokio!(async move {
            let mut failed_redactions = Vec::new();

            for (i, event_id) in events_clone.iter().enumerate() {
                match matrix_room.redact(event_id, reason.as_deref(), None).await {
                    Ok(_) => {}
                    Err(error) => {
                        error!("Could not redact event with ID {event_id}: {error}");
                        failed_redactions.push(i);
                    }
                }
            }

            failed_redactions
        });

        let failed_redactions = handle.await.expect("task was not aborted");
        let failed_redactions = failed_redactions
            .into_iter()
            .map(|i| &*events[i])
            .collect::<Vec<_>>();

        if failed_redactions.is_empty() {
            Ok(())
        } else {
            Err(failed_redactions)
        }
    }

    /// Report the given events in this room.
    ///
    /// The events are a list of `(event_id, reason)` tuples.
    ///
    /// Returns `Ok(())` if all the reports are sent successfully, otherwise
    /// returns the list of event IDs that could not be reported.
    pub(crate) async fn report_events<'a>(
        &self,
        events: &'a [(OwnedEventId, Option<String>)],
    ) -> Result<(), Vec<&'a EventId>> {
        let events_clone = events.to_owned();
        let matrix_room = self.matrix_room().clone();
        let handle = spawn_tokio!(async move {
            let futures = events_clone
                .into_iter()
                .map(|(event_id, reason)| matrix_room.report_content(event_id, reason));
            futures_util::future::join_all(futures).await
        });

        let mut failed = Vec::new();
        for (index, result) in handle
            .await
            .expect("task was not aborted")
            .iter()
            .enumerate()
        {
            match result {
                Ok(_) => {}
                Err(error) => {
                    error!(
                        "Could not report content with event ID {}: {error}",
                        events[index].0,
                    );
                    failed.push(&*events[index].0);
                }
            }
        }

        if failed.is_empty() {
            Ok(())
        } else {
            Err(failed)
        }
    }

    /// Invite the given users to this room.
    ///
    /// Returns `Ok(())` if all the invites are sent successfully, otherwise
    /// returns the list of users who could not be invited.
    pub(crate) async fn invite<'a>(
        &self,
        user_ids: &'a [OwnedUserId],
    ) -> Result<(), Vec<&'a UserId>> {
        let matrix_room = self.matrix_room();
        if matrix_room.state() != RoomState::Joined {
            error!("Can’t invite users, because this room isn’t a joined room");
            return Ok(());
        }

        let user_ids_clone = user_ids.to_owned();
        let matrix_room = matrix_room.clone();
        let handle = spawn_tokio!(async move {
            let invitations = user_ids_clone
                .iter()
                .map(|user_id| matrix_room.invite_user_by_id(user_id));
            futures_util::future::join_all(invitations).await
        });

        let mut failed_invites = Vec::new();
        for (index, result) in handle
            .await
            .expect("task was not aborted")
            .iter()
            .enumerate()
        {
            match result {
                Ok(()) => {}
                Err(error) => {
                    error!("Could not invite user with ID {}: {error}", user_ids[index],);
                    failed_invites.push(&*user_ids[index]);
                }
            }
        }

        if failed_invites.is_empty() {
            Ok(())
        } else {
            Err(failed_invites)
        }
    }

    /// Kick the given users from this room.
    ///
    /// The users are a list of `(user_id, reason)` tuples.
    ///
    /// Returns `Ok(())` if all the kicks are sent successfully, otherwise
    /// returns the list of users who could not be kicked.
    pub(crate) async fn kick<'a>(
        &self,
        users: &'a [(OwnedUserId, Option<String>)],
    ) -> Result<(), Vec<&'a UserId>> {
        let users_clone = users.to_owned();
        let matrix_room = self.matrix_room().clone();
        let handle = spawn_tokio!(async move {
            let futures = users_clone
                .iter()
                .map(|(user_id, reason)| matrix_room.kick_user(user_id, reason.as_deref()));
            futures_util::future::join_all(futures).await
        });

        let mut failed_kicks = Vec::new();
        for (index, result) in handle
            .await
            .expect("task was not aborted")
            .iter()
            .enumerate()
        {
            match result {
                Ok(()) => {}
                Err(error) => {
                    error!("Could not kick user with ID {}: {error}", users[index].0);
                    failed_kicks.push(&*users[index].0);
                }
            }
        }

        if failed_kicks.is_empty() {
            Ok(())
        } else {
            Err(failed_kicks)
        }
    }

    /// Ban the given users from this room.
    ///
    /// The users are a list of `(user_id, reason)` tuples.
    ///
    /// Returns `Ok(())` if all the bans are sent successfully, otherwise
    /// returns the list of users who could not be banned.
    pub(crate) async fn ban<'a>(
        &self,
        users: &'a [(OwnedUserId, Option<String>)],
    ) -> Result<(), Vec<&'a UserId>> {
        let users_clone = users.to_owned();
        let matrix_room = self.matrix_room().clone();
        let handle = spawn_tokio!(async move {
            let futures = users_clone
                .iter()
                .map(|(user_id, reason)| matrix_room.ban_user(user_id, reason.as_deref()));
            futures_util::future::join_all(futures).await
        });

        let mut failed_bans = Vec::new();
        for (index, result) in handle
            .await
            .expect("task was not aborted")
            .iter()
            .enumerate()
        {
            match result {
                Ok(()) => {}
                Err(error) => {
                    error!("Could not ban user with ID {}: {error}", users[index].0);
                    failed_bans.push(&*users[index].0);
                }
            }
        }

        if failed_bans.is_empty() {
            Ok(())
        } else {
            Err(failed_bans)
        }
    }

    /// Unban the given users from this room.
    ///
    /// The users are a list of `(user_id, reason)` tuples.
    ///
    /// Returns `Ok(())` if all the unbans are sent successfully, otherwise
    /// returns the list of users who could not be unbanned.
    pub(crate) async fn unban<'a>(
        &self,
        users: &'a [(OwnedUserId, Option<String>)],
    ) -> Result<(), Vec<&'a UserId>> {
        let users_clone = users.to_owned();
        let matrix_room = self.matrix_room().clone();
        let handle = spawn_tokio!(async move {
            let futures = users_clone
                .iter()
                .map(|(user_id, reason)| matrix_room.unban_user(user_id, reason.as_deref()));
            futures_util::future::join_all(futures).await
        });

        let mut failed_unbans = Vec::new();
        for (index, result) in handle
            .await
            .expect("task was not aborted")
            .iter()
            .enumerate()
        {
            match result {
                Ok(()) => {}
                Err(error) => {
                    error!("Could not unban user with ID {}: {error}", users[index].0);
                    failed_unbans.push(&*users[index].0);
                }
            }
        }

        if failed_unbans.is_empty() {
            Ok(())
        } else {
            Err(failed_unbans)
        }
    }

    /// Enable encryption for this room.
    pub(crate) async fn enable_encryption(&self) -> Result<(), ()> {
        if self.is_encrypted() {
            // Nothing to do.
            return Ok(());
        }

        let matrix_room = self.matrix_room().clone();
        let handle = spawn_tokio!(async move { matrix_room.enable_encryption().await });

        match handle.await.expect("task was not aborted") {
            Ok(()) => Ok(()),
            Err(error) => {
                error!("Could not enable room encryption: {error}");
                Err(())
            }
        }
    }

    /// Forget a room that is left.
    pub(crate) async fn forget(&self) -> MatrixResult<()> {
        if self.category() != RoomCategory::Left {
            warn!("Cannot forget a room that is not left");
            return Ok(());
        }

        let matrix_room = self.matrix_room().clone();
        let handle = spawn_tokio!(async move { matrix_room.forget().await });

        match handle.await.expect("task was not aborted") {
            Ok(()) => {
                self.emit_by_name::<()>("room-forgotten", &[]);
                Ok(())
            }
            Err(error) => {
                error!("Could not forget the room: {error}");
                Err(error)
            }
        }
    }

    /// Handle room member name ambiguity changes.
    pub(crate) fn handle_ambiguity_changes<'a>(
        &self,
        changes: impl Iterator<Item = &'a AmbiguityChange>,
    ) {
        self.imp().handle_ambiguity_changes(changes);
    }

    /// Update the latest activity of the room with the given events.
    ///
    /// The events must be in reverse chronological order.
    fn update_latest_activity<'a>(&self, events: impl Iterator<Item = &'a Event>) {
        let own_user_id = self.imp().own_member().user_id();
        let mut latest_activity = self.latest_activity();

        for event in events {
            if event.counts_as_activity(own_user_id) {
                latest_activity = latest_activity.max(event.origin_server_ts().get().into());
                break;
            }
        }

        self.imp().set_latest_activity(latest_activity);
    }

    /// Update the successor of this room.
    pub(crate) fn update_successor(&self) {
        self.imp().update_successor();
    }

    /// Connect to the signal emitted when the room was forgotten.
    pub(crate) fn connect_room_forgotten<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "room-forgotten",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}

/// Supported values for the history visibility.
#[derive(Debug, Default, Hash, Eq, PartialEq, Clone, Copy, glib::Enum)]
#[enum_type(name = "HistoryVisibilityValue")]
pub enum HistoryVisibilityValue {
    /// Anyone can read.
    WorldReadable,
    /// Members, since this was selected.
    #[default]
    Shared,
    /// Members, since they were invited.
    Invited,
    /// Members, since they joined.
    Joined,
    /// Unsupported value.
    Unsupported,
}

impl From<HistoryVisibility> for HistoryVisibilityValue {
    fn from(value: HistoryVisibility) -> Self {
        match value {
            HistoryVisibility::Invited => Self::Invited,
            HistoryVisibility::Joined => Self::Joined,
            HistoryVisibility::Shared => Self::Shared,
            HistoryVisibility::WorldReadable => Self::WorldReadable,
            _ => Self::Unsupported,
        }
    }
}

impl From<HistoryVisibilityValue> for HistoryVisibility {
    fn from(value: HistoryVisibilityValue) -> Self {
        match value {
            HistoryVisibilityValue::Invited => Self::Invited,
            HistoryVisibilityValue::Joined => Self::Joined,
            HistoryVisibilityValue::Shared => Self::Shared,
            HistoryVisibilityValue::WorldReadable => Self::WorldReadable,
            HistoryVisibilityValue::Unsupported => unimplemented!(),
        }
    }
}

/// The position of the receipt to send.
#[derive(Debug, Clone)]
pub(crate) enum ReceiptPosition {
    /// We are at the end of the timeline (bottom of the view).
    End,
    /// We are at the event with the given ID.
    Event(OwnedEventId),
}

/// Helper type to extract the current and previous memberships from a raw
/// `m.room.member` event.
#[derive(Deserialize)]
struct RoomMemberMembershipEvent {
    content: RoomMemberMembershipContent,
    unsigned: Option<RoomMemberMembershipUnsigned>,
}

/// Helper type to extract the membership of the `unsigned` object of an
/// `m.room.member` event.
#[derive(Deserialize)]
struct RoomMemberMembershipUnsigned {
    replaces_state: Option<OwnedEventId>,
    prev_content: Option<RoomMemberMembershipContent>,
}

/// Helper type to extract the membership of the `content` object of an
/// `m.room.member` event.
#[derive(Deserialize)]
struct RoomMemberMembershipContent {
    membership: MembershipState,
}
