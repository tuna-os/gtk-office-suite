use std::collections::HashMap;

use gtk::{
    gio, glib,
    glib::{clone, closure},
    prelude::*,
    subclass::prelude::*,
};
use indexmap::IndexMap;
use matrix_sdk::RoomMemberships;
use ruma::{OwnedUserId, UserId, events::room::power_levels::RoomPowerLevels};
use tracing::error;

use super::{Event, Member, Membership, Room};
use crate::{prelude::*, spawn, spawn_tokio, utils::LoadingState};

mod imp {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::MemberList)]
    pub struct MemberList {
        /// The list of known members.
        pub(super) members: RefCell<IndexMap<OwnedUserId, Member>>,
        /// The room these members belong to.
        #[property(get, set = Self::set_room, construct_only)]
        room: glib::WeakRef<Room>,
        /// The lists of members filtered by membership.
        membership_lists: RefCell<HashMap<MembershipListKind, gio::ListModel>>,
        /// The loading state of the list.
        #[property(get, builder(LoadingState::default()))]
        state: Cell<LoadingState>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MemberList {
        const NAME: &'static str = "MemberList";
        type Type = super::MemberList;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for MemberList {}

    impl ListModelImpl for MemberList {
        fn item_type(&self) -> glib::Type {
            Member::static_type()
        }

        fn n_items(&self) -> u32 {
            self.members.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.members
                .borrow()
                .get_index(position as usize)
                .map(|(_user_id, member)| member.clone().upcast())
        }
    }

    impl MemberList {
        /// Set the room these members belong to.
        fn set_room(&self, room: &Room) {
            {
                let mut members = self.members.borrow_mut();
                let own_member = room.own_member();
                members.insert(own_member.user_id().clone(), own_member);

                if let Some(member) = room.direct_member() {
                    members.insert(member.user_id().clone(), member);
                }
            }

            self.room.set(Some(room));
            self.obj().notify_room();

            spawn!(
                glib::Priority::LOW,
                clone!(
                    #[weak(rename_to = imp)]
                    self,
                    async move {
                        imp.load().await;
                    }
                )
            );
        }

        /// Get the list filtered by membership for the given kind.
        pub(super) fn membership_list(&self, kind: MembershipListKind) -> gio::ListModel {
            if let Some(list) = self.membership_lists.borrow().get(&kind) {
                return list.clone();
            }

            // Construct the list if it doesn't exist.
            let list = kind.filtered_list_model(self.obj().upcast_ref());
            self.membership_lists
                .borrow_mut()
                .insert(kind, list.clone());
            list
        }

        /// Set whether this list is being loaded.
        pub(super) fn set_state(&self, state: LoadingState) {
            if self.state.get() == state {
                return;
            }

            self.state.set(state);
            self.obj().notify_state();
        }

        /// Load this list.
        pub(super) async fn load(&self) {
            let Some(room) = self.room.upgrade() else {
                return;
            };
            if matches!(
                self.state.get(),
                LoadingState::Loading | LoadingState::Ready
            ) {
                return;
            }

            self.set_state(LoadingState::Loading);

            let matrix_room = room.matrix_room();

            // First load what we have locally.
            let matrix_room_clone = matrix_room.clone();
            let handle = spawn_tokio!(async move {
                let mut memberships = RoomMemberships::all();
                memberships.remove(RoomMemberships::LEAVE);

                matrix_room_clone.members_no_sync(memberships).await
            });

            match handle.await.expect("task was not aborted") {
                Ok(members) => {
                    self.update_from_room_members(&members);

                    if matrix_room.are_members_synced() {
                        // Nothing more to do, we can stop here.
                        self.set_state(LoadingState::Ready);
                        return;
                    }
                }
                Err(error) => {
                    error!("Could not load room members from store: {error}");
                }
            }

            // We do not have everything locally, request the rest from the server.
            let matrix_room = matrix_room.clone();
            let handle = spawn_tokio!(async move {
                let mut memberships = RoomMemberships::all();
                memberships.remove(RoomMemberships::LEAVE);

                matrix_room.members(memberships).await
            });

            // FIXME: We should retry to load the room members if the request failed
            match handle.await.expect("task was not aborted") {
                Ok(members) => {
                    // Add all members needed to display room events.
                    self.update_from_room_members(&members);
                    self.set_state(LoadingState::Ready);
                }
                Err(error) => {
                    self.set_state(LoadingState::Error);
                    error!(%error, "Could not load room members from server");
                }
            }
        }

        /// Updates members with the given SDK room member structs.
        ///
        /// If some of the new members do not correspond to existing members,
        /// they are created.
        fn update_from_room_members(&self, new_members: &[matrix_sdk::room::RoomMember]) {
            let Some(room) = self.room.upgrade() else {
                return;
            };

            let mut members = self.members.borrow_mut();
            let prev_len = members.len();
            for member in new_members {
                members
                    .entry(member.user_id().to_owned())
                    .or_insert_with_key(|user_id| Member::new(&room, user_id.clone()));
            }
            let num_members_added = members.len().saturating_sub(prev_len);

            // We cannot have the mut borrow active when members are updated or
            // items_changed is emitted because that will probably cause reads of
            // the members field.
            std::mem::drop(members);

            {
                for room_member in new_members {
                    let member = self.members.borrow().get(room_member.user_id()).cloned();
                    if let Some(member) = member {
                        member.update_from_room_member(room_member);
                    }
                }

                // Restore the members activity according to the known live timeline events.
                for item in room.live_timeline().items().iter::<glib::Object>().rev() {
                    let Ok(item) = item else {
                        // The iterator is broken, stop.
                        break;
                    };
                    let Ok(event) = item.downcast::<Event>() else {
                        continue;
                    };
                    if !event.counts_as_unread() {
                        continue;
                    }

                    let member = self.members.borrow().get(&event.sender_id()).cloned();
                    if let Some(member) = member {
                        member.set_latest_activity(u64::from(event.origin_server_ts().get()));
                    }
                }
            }

            if num_members_added > 0 {
                // IndexMap preserves insertion order, so all the new items will be at the end.
                self.obj()
                    .items_changed(prev_len as u32, 0, num_members_added as u32);
            }
        }
    }
}

glib::wrapper! {
    /// List of all Members in a room. Implements ListModel.
    ///
    /// Members are sorted in "insertion order", not anything useful.
    pub struct MemberList(ObjectSubclass<imp::MemberList>)
        @implements gio::ListModel;
}

impl MemberList {
    pub fn new(room: &Room) -> Self {
        glib::Object::builder::<Self>()
            .property("room", room)
            .build()
    }

    /// Reload this list.
    pub(crate) fn reload(&self) {
        let imp = self.imp();
        imp.set_state(LoadingState::Initial);

        spawn!(clone!(
            #[weak]
            imp,
            async move {
                imp.load().await;
            }
        ));
    }

    /// Returns the member with the given ID, if it exists in the list.
    pub(crate) fn get(&self, user_id: &UserId) -> Option<Member> {
        self.imp().members.borrow().get(user_id).cloned()
    }

    /// Returns the member with the given ID.
    ///
    /// Creates a new member first if there is no member with the given ID.
    pub(crate) fn get_or_create(&self, user_id: OwnedUserId) -> Member {
        let mut members = self.imp().members.borrow_mut();
        let mut was_member_added = false;
        let prev_len = members.len();
        let member = members
            .entry(user_id)
            .or_insert_with_key(|user_id| {
                was_member_added = true;
                Member::new(&self.room().expect("room exists"), user_id.clone())
            })
            .clone();

        // We can't have the borrow active when items_changed is emitted because that
        // will probably cause reads of the members field.
        std::mem::drop(members);
        if was_member_added {
            // IndexMap preserves insertion order so the new member will be at the end.
            self.items_changed(prev_len as u32, 0, 1);
        }

        member
    }

    /// Get the list filtered by membership for the given kind.
    pub(crate) fn membership_list(&self, kind: MembershipListKind) -> gio::ListModel {
        self.imp().membership_list(kind)
    }

    /// Update a room member with the SDK's data.
    ///
    /// Creates a new member first if there is no member matching the given
    /// event.
    pub(super) fn update_member(&self, user_id: OwnedUserId) {
        self.get_or_create(user_id).update();
    }

    /// Updates the room members' power level.
    pub(super) fn update_power_levels(&self, power_levels: &RoomPowerLevels) {
        // We need to go through the whole list because we don't know who was
        // added/removed.
        for (user_id, member) in &*self.imp().members.borrow() {
            member.set_power_level(power_levels.for_user(user_id));
        }
    }
}

/// The kind of membership used to filter a list of room members.
///
/// This is a subset of [`Membership`].
#[derive(Debug, Default, Hash, Eq, PartialEq, Clone, Copy, glib::Enum, glib::Variant)]
#[enum_type(name = "MembershipListKind")]
pub enum MembershipListKind {
    /// The user is currently in the room.
    #[default]
    Join,
    /// The user was invited to the room.
    Invite,
    /// The user was banned from the room.
    Ban,
    /// The user knocked on the room.
    Knock,
}

impl MembershipListKind {
    /// Build a `GListModel` that filters the given list model containing
    /// [`Member`]s with this kind, and add it to the given map.
    fn filtered_list_model(self, members: &gio::ListModel) -> gio::ListModel {
        let membership = Membership::from(self);
        let membership_eq_expr = Member::this_expression("membership").chain_closure::<bool>(
            closure!(|_: Option<glib::Object>, this_membership: Membership| {
                this_membership == membership
            }),
        );

        gtk::FilterListModel::builder()
            .model(members)
            .filter(&gtk::BoolFilter::new(Some(&membership_eq_expr)))
            .watch_items(true)
            .build()
            .upcast()
    }

    /// The tag to use for pages that present this kind.
    pub(crate) const fn tag(self) -> &'static str {
        match self {
            Self::Join => "join",
            Self::Invite => "invite",
            Self::Ban => "ban",
            Self::Knock => "knock",
        }
    }

    /// The name of the icon that represents this kind.
    pub(crate) const fn icon_name(self) -> &'static str {
        match self {
            Self::Join | Self::Knock => "users-symbolic",
            Self::Invite => "user-add-symbolic",
            Self::Ban => "safety-symbolic",
        }
    }
}

impl From<MembershipListKind> for Membership {
    fn from(value: MembershipListKind) -> Self {
        match value {
            MembershipListKind::Join => Self::Join,
            MembershipListKind::Invite => Self::Invite,
            MembershipListKind::Ban => Self::Ban,
            MembershipListKind::Knock => Self::Knock,
        }
    }
}
