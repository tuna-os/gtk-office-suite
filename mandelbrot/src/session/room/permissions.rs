use std::fmt;

use gettextrs::{gettext, pgettext};
use gtk::{
    glib,
    glib::{clone, closure_local},
    prelude::*,
    subclass::prelude::*,
};
use matrix_sdk::{RoomState, event_handler::EventHandlerDropGuard};
use ruma::{
    Int, OwnedUserId, UserId,
    events::{
        MessageLikeEventType, StateEventType, SyncStateEvent,
        room::power_levels::{
            NotificationPowerLevelType, PowerLevelAction, PowerLevelUserAction, RoomPowerLevels,
            RoomPowerLevelsEventContent, RoomPowerLevelsSource, UserPowerLevel,
        },
    },
    int,
    room_version_rules::AuthorizationRules,
};
use tracing::error;

use super::{Member, Membership, Room};
use crate::{prelude::*, spawn, spawn_tokio};

/// The maximum power level that can be set, according to the Matrix
/// specification.
///
/// This is the same value as `MAX_SAFE_INT` from the `js_int` crate.
pub const POWER_LEVEL_MAX: i64 = 0x001F_FFFF_FFFF_FFFF;
/// The minimum power level to have the role of Administrator, according to the
/// Matrix specification.
pub const POWER_LEVEL_ADMIN: i64 = 100;
/// The minimum power level to have the role of Moderator, according to the
/// Matrix specification.
pub const POWER_LEVEL_MOD: i64 = 50;

/// Role of a room member, like admin or moderator.
#[derive(Debug, Default, Hash, Eq, PartialEq, Clone, Copy, glib::Enum)]
#[enum_type(name = "MemberRole")]
pub enum MemberRole {
    /// A room member with the default power level.
    #[default]
    Default,
    /// A room member with a non-default power level, but lower than and a
    /// moderator.
    Custom,
    /// A moderator.
    Moderator,
    /// An administrator.
    Administrator,
    /// A creator.
    Creator,
    /// A room member that cannot send messages.
    Muted,
}

impl fmt::Display for MemberRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            // Translators: As in 'Default power level', meaning permissions.
            Self::Default => write!(f, "{}", gettext("Default")),
            // Translators: As in, 'Custom power level', meaning permissions.
            Self::Custom => write!(f, "{}", pgettext("power level", "Custom")),
            Self::Moderator => write!(f, "{}", gettext("Moderator")),
            Self::Administrator => write!(f, "{}", gettext("Admin")),
            Self::Creator => write!(f, "{}", gettext("Creator")),
            // Translators: As in 'Muted room member', a member that cannot send messages.
            Self::Muted => write!(f, "{}", gettext("Muted")),
        }
    }
}

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        sync::LazyLock,
    };

    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, glib::Properties)]
    #[properties(wrapper_type = super::Permissions)]
    pub struct Permissions {
        /// The room where these permissions apply.
        #[property(get)]
        pub(super) room: glib::WeakRef<Room>,
        /// The source of the power levels information.
        pub(super) power_levels: RefCell<RoomPowerLevels>,
        power_levels_drop_guard: OnceCell<EventHandlerDropGuard>,
        /// Whether our own member is joined.
        #[property(get)]
        is_joined: Cell<bool>,
        /// The power level of our own member.
        pub(super) own_power_level: Cell<UserPowerLevel>,
        /// The default power level for members.
        #[property(get)]
        default_power_level: Cell<i64>,
        /// The power level to mute members.
        #[property(get)]
        mute_power_level: Cell<i64>,
        /// Whether our own member can change the room's avatar.
        #[property(get)]
        can_change_avatar: Cell<bool>,
        /// Whether our own member can change the room's name.
        #[property(get)]
        can_change_name: Cell<bool>,
        /// Whether our own member can start or join a call.
        #[property(get)]
        can_start_call: Cell<bool>,
        /// Whether our own member can change the room's topic.
        #[property(get)]
        can_change_topic: Cell<bool>,
        /// Whether our own member can invite another user.
        #[property(get)]
        can_invite: Cell<bool>,
        /// Whether our own member can send a message.
        #[property(get)]
        can_send_message: Cell<bool>,
        /// Whether our own member can send a reaction.
        #[property(get)]
        can_send_reaction: Cell<bool>,
        /// Whether our own member can redact their own event.
        #[property(get)]
        can_redact_own: Cell<bool>,
        /// Whether our own member can redact the event of another user.
        #[property(get)]
        can_redact_other: Cell<bool>,
        /// Whether our own member can notify the whole room.
        #[property(get)]
        can_notify_room: Cell<bool>,
    }

    impl Default for Permissions {
        fn default() -> Self {
            Self {
                room: Default::default(),
                power_levels: RefCell::new(RoomPowerLevels::new(
                    RoomPowerLevelsSource::None,
                    &AuthorizationRules::V1,
                    None,
                )),
                power_levels_drop_guard: Default::default(),
                is_joined: Default::default(),
                own_power_level: Cell::new(UserPowerLevel::Int(int!(0))),
                default_power_level: Default::default(),
                mute_power_level: Default::default(),
                can_change_avatar: Default::default(),
                can_change_name: Default::default(),
                can_start_call: Default::default(),
                can_change_topic: Default::default(),
                can_invite: Default::default(),
                can_send_message: Default::default(),
                can_send_reaction: Default::default(),
                can_redact_own: Default::default(),
                can_redact_other: Default::default(),
                can_notify_room: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Permissions {
        const NAME: &'static str = "RoomPermissions";
        type Type = super::Permissions;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Permissions {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    Signal::builder("changed").build(),
                    Signal::builder("own-power-level-changed").build(),
                ]
            });
            SIGNALS.as_ref()
        }
    }

    impl Permissions {
        /// Initialize the room.
        pub(super) fn init_own_member(&self, own_member: &Member) {
            own_member.connect_membership_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_is_joined();
                }
            ));

            self.update_is_joined();
        }

        /// The room member for our own user.
        pub(super) fn own_member(&self) -> Option<Member> {
            self.room.upgrade().map(|r| r.own_member())
        }

        /// Initialize the power levels from the store.
        pub(super) async fn init_power_levels(&self) {
            let Some(room) = self.room.upgrade() else {
                return;
            };

            let matrix_room = room.matrix_room();

            // We will probably not be able to load the power levels if we were never in the
            // room, so skip this. We should get the power levels when we join the room.
            if !matches!(matrix_room.state(), RoomState::Invited | RoomState::Knocked) {
                self.update_power_levels().await;
            }

            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let handle = matrix_room.add_event_handler(
                move |_event: SyncStateEvent<RoomPowerLevelsEventContent>| {
                    let obj_weak = obj_weak.clone();
                    async move {
                        let ctx = glib::MainContext::default();
                        ctx.spawn(async move {
                            spawn!(async move {
                                if let Some(obj) = obj_weak.upgrade() {
                                    obj.imp().update_power_levels().await;
                                }
                            });
                        });
                    }
                },
            );

            let drop_guard = matrix_room.client().event_handler_drop_guard(handle);
            self.power_levels_drop_guard
                .set(drop_guard)
                .expect("power levels drop guard is uninitialized");
        }

        /// Update whether our own member is joined
        fn update_is_joined(&self) {
            let Some(own_member) = self.own_member() else {
                return;
            };

            let is_joined = own_member.membership() == Membership::Join;

            if self.is_joined.get() == is_joined {
                return;
            }

            self.is_joined.set(is_joined);
            self.permissions_changed();
        }

        /// Update the power levels with the data from the SDK's room.
        async fn update_power_levels(&self) {
            let Some(room) = self.room.upgrade() else {
                return;
            };

            let matrix_room = room.matrix_room().clone();
            let handle = spawn_tokio!(async move { matrix_room.power_levels().await });

            let power_levels = match handle.await.expect("task was not aborted") {
                Ok(power_levels) => power_levels,
                Err(error) => {
                    error!("Could not load room power levels: {error}");
                    return;
                }
            };

            self.power_levels.replace(power_levels.clone());
            self.permissions_changed();

            if let Some(members) = room.members() {
                members.update_power_levels(&power_levels);
            } else {
                let own_member = room.own_member();
                let own_user_id = own_member.user_id();
                own_member.set_power_level(power_levels.for_user(own_user_id));
            }
        }

        /// Trigger updates when the permissions changed.
        fn permissions_changed(&self) {
            self.update_own_power_level();
            self.update_default_power_level();
            self.update_mute_power_level();
            self.update_can_change_avatar();
            self.update_can_change_name();
            self.update_can_start_call();
            self.update_can_change_topic();
            self.update_can_invite();
            self.update_can_send_message();
            self.update_can_send_reaction();
            self.update_can_redact_own();
            self.update_can_redact_other();
            self.update_can_notify_room();
            self.obj().emit_by_name::<()>("changed", &[]);
        }

        /// Update the power level of our own member.
        fn update_own_power_level(&self) {
            let Some(room) = self.room.upgrade() else {
                return;
            };
            let own_member = room.own_member();

            let power_level = self.power_levels.borrow().for_user(own_member.user_id());

            if self.own_power_level.get() == power_level {
                return;
            }

            self.own_power_level.set(power_level);
            self.obj()
                .emit_by_name::<()>("own-power-level-changed", &[]);
        }

        /// Update the default power level for members.
        fn update_default_power_level(&self) {
            let power_level = self.power_levels.borrow().users_default.into();

            if self.default_power_level.get() == power_level {
                return;
            }

            self.default_power_level.set(power_level);
            self.obj().notify_default_power_level();
        }

        /// Update the power level to mute members.
        fn update_mute_power_level(&self) {
            // To mute user they must not have enough power to send messages.
            let power_levels = self.power_levels.borrow();
            let message_power_level = power_levels
                .events
                .get(&MessageLikeEventType::RoomMessage.into())
                .copied()
                .unwrap_or(power_levels.events_default);
            let power_level = (-1).min(message_power_level.into());

            if self.mute_power_level.get() == power_level {
                return;
            }

            self.mute_power_level.set(power_level);
            self.obj().notify_mute_power_level();
        }

        /// Whether our own member is allowed to do the given action.
        pub(super) fn is_allowed_to(&self, room_action: PowerLevelAction) -> bool {
            if !self.is_joined.get() {
                // We cannot do anything if the member is not joined.
                return false;
            }

            let Some(own_member) = self.own_member() else {
                return false;
            };

            self.power_levels
                .borrow()
                .user_can_do(own_member.user_id(), room_action)
        }

        /// Update whether our own member can change the room's avatar.
        fn update_can_change_avatar(&self) {
            let can_change_avatar =
                self.is_allowed_to(PowerLevelAction::SendState(StateEventType::RoomAvatar));

            if self.can_change_avatar.get() == can_change_avatar {
                return;
            }

            self.can_change_avatar.set(can_change_avatar);
            self.obj().notify_can_change_avatar();
        }

        /// Update whether our own member can change the room's name.
        fn update_can_change_name(&self) {
            let can_change_name =
                self.is_allowed_to(PowerLevelAction::SendState(StateEventType::RoomName));

            if self.can_change_name.get() == can_change_name {
                return;
            }

            self.can_change_name.set(can_change_name);
            self.obj().notify_can_change_name();
        }

        /// Update whether our own member can start or join a call.
        fn update_can_start_call(&self) {
            let can_start_call =
                self.is_allowed_to(PowerLevelAction::SendState(StateEventType::CallMember));

            if self.can_start_call.get() == can_start_call {
                return;
            }

            self.can_start_call.set(can_start_call);
            self.obj().notify_can_start_call();
        }

        /// Update whether our own member can change the room's topic.
        fn update_can_change_topic(&self) {
            let can_change_topic =
                self.is_allowed_to(PowerLevelAction::SendState(StateEventType::RoomTopic));

            if self.can_change_topic.get() == can_change_topic {
                return;
            }

            self.can_change_topic.set(can_change_topic);
            self.obj().notify_can_change_topic();
        }

        /// Update whether our own member can invite another user in the room.
        fn update_can_invite(&self) {
            let can_invite = self.is_allowed_to(PowerLevelAction::Invite);

            if self.can_invite.get() == can_invite {
                return;
            }

            self.can_invite.set(can_invite);
            self.obj().notify_can_invite();
        }

        /// Update whether our own member can send a message in the room.
        fn update_can_send_message(&self) {
            let can_send_message = self.is_allowed_to(PowerLevelAction::SendMessage(
                MessageLikeEventType::RoomMessage,
            ));

            if self.can_send_message.get() == can_send_message {
                return;
            }

            self.can_send_message.set(can_send_message);
            self.obj().notify_can_send_message();
        }

        /// Update whether our own member can send a reaction.
        fn update_can_send_reaction(&self) {
            let can_send_reaction = self.is_allowed_to(PowerLevelAction::SendMessage(
                MessageLikeEventType::Reaction,
            ));

            if self.can_send_reaction.get() == can_send_reaction {
                return;
            }

            self.can_send_reaction.set(can_send_reaction);
            self.obj().notify_can_send_reaction();
        }

        /// Update whether our own member can redact their own event.
        fn update_can_redact_own(&self) {
            let can_redact_own = self.is_allowed_to(PowerLevelAction::RedactOwn);

            if self.can_redact_own.get() == can_redact_own {
                return;
            }

            self.can_redact_own.set(can_redact_own);
            self.obj().notify_can_redact_own();
        }

        /// Update whether our own member can redact the event of another user.
        fn update_can_redact_other(&self) {
            let can_redact_other = self.is_allowed_to(PowerLevelAction::RedactOther);

            if self.can_redact_other.get() == can_redact_other {
                return;
            }

            self.can_redact_other.set(can_redact_other);
            self.obj().notify_can_redact_other();
        }

        /// Update whether our own member can notify the whole room.
        fn update_can_notify_room(&self) {
            let can_notify_room = self.is_allowed_to(PowerLevelAction::TriggerNotification(
                NotificationPowerLevelType::Room,
            ));

            if self.can_notify_room.get() == can_notify_room {
                return;
            }

            self.can_notify_room.set(can_notify_room);
            self.obj().notify_can_notify_room();
        }
    }
}

glib::wrapper! {
    /// The permissions of our own user in a room.
    pub struct Permissions(ObjectSubclass<imp::Permissions>);
}

impl Permissions {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set our own member.
    pub(super) async fn init(&self, room: &Room) {
        let imp = self.imp();

        imp.room.set(Some(room));
        imp.init_own_member(&room.own_member());
        imp.init_power_levels().await;
    }

    /// The source of the power levels information.
    pub(crate) fn power_levels(&self) -> RoomPowerLevels {
        self.imp().power_levels.borrow().clone()
    }

    /// The power level of our own member.
    pub(crate) fn own_power_level(&self) -> UserPowerLevel {
        self.imp().own_power_level.get()
    }

    /// The power level for the user with the given ID.
    pub(crate) fn user_power_level(&self, user_id: &UserId) -> UserPowerLevel {
        self.imp().power_levels.borrow().for_user(user_id)
    }

    /// The current [`MemberRole`] for the given power level.
    pub(crate) fn role(&self, power_level: UserPowerLevel) -> MemberRole {
        let UserPowerLevel::Int(power_level) = power_level else {
            return MemberRole::Creator;
        };

        let power_level = i64::from(power_level);

        if power_level >= POWER_LEVEL_ADMIN {
            MemberRole::Administrator
        } else if power_level >= POWER_LEVEL_MOD {
            MemberRole::Moderator
        } else if power_level == self.default_power_level() {
            MemberRole::Default
        } else if power_level < self.default_power_level() && power_level <= self.mute_power_level()
        {
            // Only set role as muted for members below default, to avoid visual noise in
            // rooms where muted is the default.
            MemberRole::Muted
        } else {
            MemberRole::Custom
        }
    }

    /// Whether our own member is allowed to do the given action.
    pub(crate) fn is_allowed_to(&self, room_action: PowerLevelAction) -> bool {
        self.imp().is_allowed_to(room_action)
    }

    /// Whether our own user can do the given action on the user with the given
    /// ID.
    pub(crate) fn can_do_to_user(&self, user_id: &UserId, action: PowerLevelUserAction) -> bool {
        let imp = self.imp();

        if !self.is_joined() {
            // We cannot do anything if the member is not joined.
            return false;
        }

        let Some(own_member) = imp.own_member() else {
            return false;
        };
        let own_user_id = own_member.user_id();

        let power_levels = imp.power_levels.borrow();

        if own_user_id == user_id {
            // The only action we can do for our own user is change the power level, if it's
            // not a creator.
            return action == PowerLevelUserAction::ChangePowerLevel
                && power_levels.user_can_change_user_power_level(own_user_id, own_user_id);
        }

        power_levels.user_can_do_to_user(own_user_id, user_id, action)
    }

    /// Whether our user can set the given power level for another user.
    pub(crate) fn can_set_user_power_level_to(&self, power_level: i64) -> bool {
        self.is_allowed_to(PowerLevelAction::SendState(StateEventType::RoomPowerLevels))
            && self.own_power_level() >= Int::new_saturating(power_level)
    }

    /// Set the power level of the room member with the given user ID.
    pub(crate) async fn set_user_power_level(
        &self,
        user_id: OwnedUserId,
        power_level: Int,
    ) -> Result<(), ()> {
        let Some(room) = self.room() else {
            return Err(());
        };

        let matrix_room = room.matrix_room().clone();
        let handle = spawn_tokio!(async move {
            matrix_room
                .update_power_levels(vec![(&user_id, power_level)])
                .await
        });

        match handle.await.expect("task was not aborted") {
            Ok(_) => Ok(()),
            Err(error) => {
                error!("Could not set user power level: {error}");
                Err(())
            }
        }
    }

    /// Set the power levels.
    pub(crate) async fn set_power_levels(&self, power_levels: RoomPowerLevels) -> Result<(), ()> {
        let Some(room) = self.room() else {
            return Err(());
        };

        let event = RoomPowerLevelsEventContent::try_from(power_levels).map_err(|error| {
            error!("Could not set power levels: {error}");
        })?;

        let matrix_room = room.matrix_room().clone();
        let handle = spawn_tokio!(async move { matrix_room.send_state_event(event).await });

        match handle.await.expect("task was not aborted") {
            Ok(_) => Ok(()),
            Err(error) => {
                error!("Could not set power levels: {error}");
                Err(())
            }
        }
    }

    /// Whether the user with the given ID is allowed to do the given action.
    pub(crate) fn user_is_allowed_to(
        &self,
        user_id: &UserId,
        room_action: PowerLevelAction,
    ) -> bool {
        self.imp()
            .power_levels
            .borrow()
            .user_can_do(user_id, room_action)
    }

    /// Connect to the signal emitted when the permissions changed.
    pub(crate) fn connect_changed<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "changed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }

    /// Connect to the signal emitted when the power level of our own member
    /// changed.
    pub(crate) fn connect_own_power_level_changed<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "own-power-level-changed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}

impl Default for Permissions {
    fn default() -> Self {
        Self::new()
    }
}
