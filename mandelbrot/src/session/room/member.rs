use gtk::{
    glib,
    glib::{clone, closure_local},
    prelude::*,
    subclass::prelude::*,
};
use matrix_sdk::room::RoomMember;
use ruma::{
    OwnedEventId, OwnedUserId,
    events::room::{
        member::MembershipState,
        power_levels::{NotificationPowerLevelType, PowerLevelAction, UserPowerLevel},
    },
    int,
};
use tracing::{debug, error};

use super::{MemberRole, Room};
use crate::{components::PillSource, prelude::*, session::User, spawn, spawn_tokio};

/// The possible states of membership of a user in a room.
#[derive(Debug, Default, Eq, PartialEq, Clone, Copy, glib::Enum)]
#[enum_type(name = "Membership")]
pub enum Membership {
    /// The user left the room, or was never in the room.
    #[default]
    Leave,
    /// The user is currently in the room.
    Join,
    /// The user was invited to the room.
    Invite,
    /// The user was banned from the room.
    Ban,
    /// The user knocked on the room.
    Knock,
    /// The user is in an unsupported membership state.
    Unsupported,
}

impl From<&MembershipState> for Membership {
    fn from(state: &MembershipState) -> Self {
        match state {
            MembershipState::Leave => Membership::Leave,
            MembershipState::Join => Membership::Join,
            MembershipState::Invite => Membership::Invite,
            MembershipState::Ban => Membership::Ban,
            MembershipState::Knock => Membership::Knock,
            _ => Membership::Unsupported,
        }
    }
}

impl From<MembershipState> for Membership {
    fn from(state: MembershipState) -> Self {
        Membership::from(&state)
    }
}

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        marker::PhantomData,
        sync::LazyLock,
    };

    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, glib::Properties)]
    #[properties(wrapper_type = super::Member)]
    pub struct Member {
        /// The room of the member.
        #[property(get, set = Self::set_room, construct_only)]
        room: OnceCell<Room>,
        /// The power level of the member.
        pub(super) power_level: Cell<UserPowerLevel>,
        /// The power level of the member, as an `i64`.
        ///
        /// Should only be used for sorting.
        ///
        /// `i64::MAX` is used to represent an infinite power level, since it
        /// cannot be reached with the Matrix specification.
        #[property(get = Self::power_level_i64)]
        power_level_i64: PhantomData<i64>,
        /// The role of the member.
        #[property(get, builder(MemberRole::default()))]
        role: Cell<MemberRole>,
        /// This membership state of the member.
        #[property(get, builder(Membership::default()))]
        membership: Cell<Membership>,
        /// The timestamp of the latest activity of this member.
        #[property(get, set = Self::set_latest_activity, explicit_notify)]
        latest_activity: Cell<u64>,
        power_level_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    }

    impl Default for Member {
        fn default() -> Self {
            Self {
                room: Default::default(),
                power_level: Cell::new(UserPowerLevel::Int(int!(0))),
                power_level_i64: Default::default(),
                role: Default::default(),
                membership: Default::default(),
                latest_activity: Default::default(),
                power_level_handlers: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Member {
        const NAME: &'static str = "Member";
        type Type = super::Member;
        type ParentType = User;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Member {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("power-level-changed").build()]);
            SIGNALS.as_ref()
        }

        fn dispose(&self) {
            if let Some(room) = self.room.get() {
                for handler in self.power_level_handlers.take() {
                    room.permissions().disconnect(handler);
                }
            }
        }
    }

    impl PillSourceImpl for Member {
        fn identifier(&self) -> String {
            self.obj().upcast_ref::<User>().user_id_string()
        }
    }

    impl Member {
        /// Set the room of the member.
        fn set_room(&self, room: Room) {
            let room = self.room.get_or_init(|| room);

            let default_pl_handler = room
                .permissions()
                .connect_default_power_level_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_role();
                    }
                ));
            let mute_pl_handler = room.permissions().connect_mute_power_level_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_role();
                }
            ));
            self.power_level_handlers
                .replace(vec![default_pl_handler, mute_pl_handler]);
        }

        /// Set the power level of the member.
        pub(super) fn set_power_level(&self, power_level: UserPowerLevel) {
            if self.power_level.get() == power_level {
                return;
            }

            self.power_level.set(power_level);
            self.update_role();

            let obj = self.obj();
            obj.emit_by_name::<()>("power-level-changed", &[]);
            obj.notify_power_level_i64();
        }

        /// The power level of the member, as an `i64`.
        fn power_level_i64(&self) -> i64 {
            if let UserPowerLevel::Int(power_level) = self.power_level.get() {
                power_level.into()
            } else {
                // Represent the infinite power level with a value out of range for a power
                // level.
                i64::MAX
            }
        }

        /// Update the role of the member.
        fn update_role(&self) {
            let role = self
                .room
                .get()
                .expect("room is initialized")
                .permissions()
                .role(self.power_level.get());

            if self.role.get() == role {
                return;
            }

            self.role.set(role);
            self.obj().notify_role();
        }

        /// Set this membership state of the member.
        pub(super) fn set_membership(&self, membership: Membership) {
            if self.membership.get() == membership {
                return;
            }

            self.membership.replace(membership);
            self.obj().notify_membership();
        }

        /// Set the timestamp of the latest activity of this member.
        fn set_latest_activity(&self, activity: u64) {
            if self.latest_activity.get() >= activity {
                return;
            }

            self.latest_activity.set(activity);
            self.obj().notify_latest_activity();
        }
    }
}

glib::wrapper! {
    /// A `User` in the context of a given room.
    pub struct Member(ObjectSubclass<imp::Member>) @extends PillSource, User;
}

impl Member {
    pub fn new(room: &Room, user_id: OwnedUserId) -> Self {
        let session = room.session();
        let obj = glib::Object::builder::<Self>()
            .property("session", &session)
            .property("room", room)
            .build();

        obj.upcast_ref::<User>().imp().set_user_id(user_id);
        obj
    }

    /// The power level of the member.
    pub(crate) fn power_level(&self) -> UserPowerLevel {
        self.imp().power_level.get()
    }

    /// Set the power level of the member.
    pub(super) fn set_power_level(&self, power_level: UserPowerLevel) {
        self.imp().set_power_level(power_level);
    }

    /// Update this member with the data from the given SDK's member.
    pub(crate) fn update_from_room_member(&self, member: &RoomMember) {
        if member.user_id() != self.user_id() {
            error!("Tried Member update from RoomMember with wrong user ID.");
            return;
        }

        self.set_name(member.display_name().map(ToOwned::to_owned));
        self.set_is_name_ambiguous(member.name_ambiguous());
        self.avatar_data()
            .image()
            .expect("image is set")
            .set_uri_and_info(member.avatar_url().map(ToOwned::to_owned), None);
        self.set_power_level(member.power_level());
        self.imp().set_membership(member.membership().into());
    }

    /// Update this member with data from the SDK.
    pub(crate) fn update(&self) {
        spawn!(clone!(
            #[weak(rename_to = obj)]
            self,
            async move {
                obj.update_inner().await;
            }
        ));
    }

    async fn update_inner(&self) {
        let room = self.room();

        let matrix_room = room.matrix_room().clone();
        let user_id = self.user_id().clone();
        let handle = spawn_tokio!(async move { matrix_room.get_member_no_sync(&user_id).await });

        match handle.await.expect("task was not aborted") {
            Ok(Some(member)) => self.update_from_room_member(&member),
            Ok(None) => {
                debug!("Room member {} not found", self.user_id());
            }
            Err(error) => {
                error!("Could not load room member {}: {error}", self.user_id());
            }
        }
    }

    /// The IDs of the events sent by this member that can be redacted.
    pub(crate) fn redactable_events(&self) -> Vec<OwnedEventId> {
        self.room()
            .live_timeline()
            .redactable_events_for(self.user_id())
    }

    /// Whether this room member can notify the whole room.
    pub(crate) fn can_notify_room(&self) -> bool {
        self.room().permissions().user_is_allowed_to(
            self.user_id(),
            PowerLevelAction::TriggerNotification(NotificationPowerLevelType::Room),
        )
    }

    /// The string to use to search for this member.
    pub(crate) fn search_string(&self) -> String {
        format!("{} {} {}", self.display_name(), self.user_id(), self.role())
    }

    /// Connect to the signal emitted when the power level of the member
    /// changed.
    pub(crate) fn connect_power_level_changed<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "power-level-changed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}
