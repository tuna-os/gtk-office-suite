use gtk::{glib, prelude::*, subclass::prelude::*};
use matrix_sdk_ui::spaces::SpaceRoom;
use ruma::{
    OwnedRoomAliasId, OwnedRoomId, OwnedServerName,
    room::{JoinRuleSummary, RoomType},
};

use crate::{
    components::{AvatarImage, AvatarUriSource, PillSource},
    prelude::*,
    session::{RoomListRoomInfo, Session},
    utils::string::linkify,
};

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::SpaceHierarchyChild)]
    pub struct SpaceHierarchyChild {
        /// The current session.
        #[property(get, set = Self::set_session, construct_only)]
        session: glib::WeakRef<Session>,
        /// The ID of this room.
        room_id: OnceCell<OwnedRoomId>,
        /// The servers that should know this room.
        via: RefCell<Vec<OwnedServerName>>,
        /// The canonical alias of this room.
        canonical_alias: RefCell<Option<OwnedRoomAliasId>>,
        /// The canonical alias of this room, as a string.
        #[property(get = Self::alias_string)]
        alias_string: std::marker::PhantomData<Option<String>>,
        /// The topic of this room, with detected links.
        #[property(get)]
        topic_linkified: RefCell<Option<String>>,
        /// The number of joined members in this room.
        #[property(get)]
        joined_members_count: Cell<u64>,
        /// Whether this room is a space.
        #[property(get)]
        is_space: Cell<bool>,
        /// The number of children of this room, if it is a space.
        #[property(get)]
        children_count: Cell<u64>,
        /// Whether this room is suggested by the space administrators.
        #[property(get)]
        is_suggested: Cell<bool>,
        /// Whether we can knock on this room.
        #[property(get)]
        can_knock: Cell<bool>,
        /// The information about this room in the room list.
        #[property(get)]
        room_list_info: RoomListRoomInfo,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SpaceHierarchyChild {
        const NAME: &'static str = "SpaceHierarchyChild";
        type Type = super::SpaceHierarchyChild;
        type ParentType = PillSource;
    }

    #[glib::derived_properties]
    impl ObjectImpl for SpaceHierarchyChild {}

    impl PillSourceImpl for SpaceHierarchyChild {
        fn identifier(&self) -> String {
            self.room_id().to_string()
        }
    }

    impl SpaceHierarchyChild {
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

        /// The ID of this room.
        pub(super) fn room_id(&self) -> &OwnedRoomId {
            self.room_id.get().expect("room ID should be initialized")
        }

        /// The servers that should know this room.
        pub(super) fn via(&self) -> Vec<OwnedServerName> {
            self.via.borrow().clone()
        }

        /// The canonical alias of this room, as a string.
        fn alias_string(&self) -> Option<String> {
            self.canonical_alias
                .borrow()
                .as_ref()
                .map(ToString::to_string)
        }

        /// Update this room with the given data from the space hierarchy.
        pub(super) fn update_with(&self, data: &SpaceRoom) {
            let obj = self.obj();

            let room_id = self
                .room_id
                .get_or_init(|| data.room_id.clone())
                .clone()
                .into();
            let identifiers = data
                .canonical_alias
                .clone()
                .map(Into::into)
                .into_iter()
                .chain(Some(room_id))
                .collect();
            self.room_list_info.set_identifiers(identifiers);

            obj.set_display_name(data.display_name.clone());

            if *self.canonical_alias.borrow() != data.canonical_alias {
                self.canonical_alias.replace(data.canonical_alias.clone());
                obj.notify_alias_string();
            }

            self.set_topic(data.topic.clone());

            if self.joined_members_count.get() != data.num_joined_members {
                self.joined_members_count.set(data.num_joined_members);
                obj.notify_joined_members_count();
            }

            let is_space = matches!(data.room_type, Some(RoomType::Space));
            if self.is_space.get() != is_space {
                self.is_space.set(is_space);
                obj.notify_is_space();
            }

            if self.children_count.get() != data.children_count {
                self.children_count.set(data.children_count);
                obj.notify_children_count();
            }

            if self.is_suggested.get() != data.suggested {
                self.is_suggested.set(data.suggested);
                obj.notify_is_suggested();
            }

            let can_knock = matches!(
                data.join_rule,
                Some(JoinRuleSummary::Knock | JoinRuleSummary::KnockRestricted(_))
            );
            if self.can_knock.get() != can_knock {
                self.can_knock.set(can_knock);
                obj.notify_can_knock();
            }

            self.via.replace(data.via.clone());

            if let Some(image) = obj.avatar_data().image() {
                image.set_uri_and_info(data.avatar_url.clone(), None);
            }
        }

        /// Set the topic of this room.
        fn set_topic(&self, topic: Option<String>) {
            let topic =
                topic.filter(|s| !s.is_empty() && s.find(|c: char| !c.is_whitespace()).is_some());

            let topic_linkified = topic.map(|t| {
                // Detect links.
                let mut s = linkify(&t);
                // Remove trailing spaces.
                s.truncate_end_whitespaces();
                s
            });

            if *self.topic_linkified.borrow() == topic_linkified {
                return;
            }

            self.topic_linkified.replace(topic_linkified);
            self.obj().notify_topic_linkified();
        }
    }
}

glib::wrapper! {
    /// A room or space in the hierarchy of a space.
    pub struct SpaceHierarchyChild(ObjectSubclass<imp::SpaceHierarchyChild>)
        @extends PillSource;
}

impl SpaceHierarchyChild {
    /// Construct a new `SpaceHierarchyChild` with the given session and data.
    pub(crate) fn new(session: &Session, data: &SpaceRoom) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("session", session)
            .build();
        obj.imp().update_with(data);
        obj
    }

    /// The ID of this room.
    pub(crate) fn room_id(&self) -> &OwnedRoomId {
        self.imp().room_id()
    }

    /// The servers that should know this room.
    pub(crate) fn via(&self) -> Vec<OwnedServerName> {
        self.imp().via()
    }

    /// Update this room with the given data from the space hierarchy.
    pub(crate) fn update_with(&self, data: &SpaceRoom) {
        self.imp().update_with(data);
    }
}
