use gettextrs::gettext;
use gtk::{
    glib,
    glib::{clone, closure_local},
    prelude::*,
    subclass::prelude::*,
};
use ruma::{
    OwnedRoomId,
    events::room::join_rules::{
        AllowRule, JoinRule as MatrixJoinRule, Restricted, RoomJoinRulesEventContent,
    },
};
use tracing::error;

use super::{Membership, Room};
use crate::{components::PillSource, gettext_f, spawn_tokio, utils::BoundObject};

/// Simplified join rules.
#[derive(Debug, Default, Hash, Eq, PartialEq, Clone, Copy, glib::Enum)]
#[enum_type(name = "JoinRuleValue")]
pub enum JoinRuleValue {
    /// Only invited users can join.
    #[default]
    Invite,
    /// Anyone can join.
    Public,
    /// Members of a room can join.
    RoomMembership,
    /// The rule is unsupported.
    Unsupported,
}

impl JoinRuleValue {
    /// Whether we support editing this join rule.
    pub(crate) fn can_be_edited(self) -> bool {
        matches!(self, Self::Invite | Self::Public)
    }
}

impl From<&MatrixJoinRule> for JoinRuleValue {
    fn from(value: &MatrixJoinRule) -> Self {
        match value {
            MatrixJoinRule::Invite | MatrixJoinRule::Knock => Self::Invite,
            MatrixJoinRule::Restricted(restricted)
            | MatrixJoinRule::KnockRestricted(restricted) => {
                if has_restricted_membership_room(restricted) {
                    Self::RoomMembership
                } else {
                    Self::Unsupported
                }
            }
            MatrixJoinRule::Public => Self::Public,
            _ => Self::Unsupported,
        }
    }
}

mod imp {
    use std::{
        cell::{Cell, RefCell},
        sync::LazyLock,
    };

    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::JoinRule)]
    pub struct JoinRule {
        /// The room where this join rule apply.
        #[property(get)]
        room: glib::WeakRef<Room>,
        /// The current join rule from the SDK.
        matrix_join_rule: RefCell<Option<MatrixJoinRule>>,
        /// The value of the join rule.
        #[property(get, builder(JoinRuleValue::default()))]
        value: Cell<JoinRuleValue>,
        /// Whether users can knock.
        #[property(get)]
        can_knock: Cell<bool>,
        /// The string to use to display this join rule.
        ///
        /// This string can contain markup.
        #[property(get)]
        display_name: RefCell<String>,
        /// The room we need to be a member of to match this join rule, if any.
        ///
        /// This can be a `Room` or a `RemoteRoom`.
        // TODO: Support multiple rooms.
        #[property(get)]
        membership_room: BoundObject<PillSource>,
        /// Whether our own user can join this room on their own.
        #[property(get)]
        we_can_join: Cell<bool>,
        /// Whether anyone can join this room on their own.
        #[property(get)]
        anyone_can_join: Cell<bool>,
        own_membership_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for JoinRule {
        const NAME: &'static str = "RoomJoinRule";
        type Type = super::JoinRule;
    }

    #[glib::derived_properties]
    impl ObjectImpl for JoinRule {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("changed").build()]);
            SIGNALS.as_ref()
        }

        fn dispose(&self) {
            if let Some(room) = self.room.upgrade()
                && let Some(handler) = self.own_membership_handler.take()
            {
                room.own_member().disconnect(handler);
            }
        }
    }

    impl JoinRule {
        /// Set the room where this join rule applies.
        pub(super) fn set_room(&self, room: &Room) {
            self.room.set(Some(room));

            let own_membership_handler = room.own_member().connect_membership_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_we_can_join();
                }
            ));
            self.own_membership_handler
                .replace(Some(own_membership_handler));
        }

        /// The current join rule from the SDK.
        pub(super) fn matrix_join_rule(&self) -> Option<MatrixJoinRule> {
            self.matrix_join_rule.borrow().clone()
        }

        /// Update the join rule.
        pub(super) fn update_join_rule(&self, join_rule: Option<&MatrixJoinRule>) {
            if self.matrix_join_rule.borrow().as_ref() == join_rule {
                return;
            }

            self.matrix_join_rule.replace(join_rule.cloned());

            self.update_value();
            self.update_can_knock();
            self.update_membership_room();
            self.update_display_name();

            self.update_we_can_join();
            self.update_anyone_can_join();

            self.obj().emit_by_name::<()>("changed", &[]);
        }

        /// Update the value of the join rule.
        fn update_value(&self) {
            let value = self
                .matrix_join_rule
                .borrow()
                .as_ref()
                .map(Into::into)
                .unwrap_or_default();

            if self.value.get() == value {
                return;
            }

            self.value.set(value);
            self.obj().notify_value();
        }

        /// Update whether users can knock.
        fn update_can_knock(&self) {
            let can_knock = self.matrix_join_rule.borrow().as_ref().is_some_and(|r| {
                matches!(
                    r,
                    MatrixJoinRule::Knock | MatrixJoinRule::KnockRestricted(_)
                )
            });

            if self.can_knock.get() == can_knock {
                return;
            }

            self.can_knock.set(can_knock);
            self.obj().notify_can_knock();
        }

        /// Set the room we need to be a member of to match this join rule.
        fn update_membership_room(&self) {
            let room_id = self
                .matrix_join_rule
                .borrow()
                .as_ref()
                .and_then(|r| match r {
                    MatrixJoinRule::Restricted(restricted)
                    | MatrixJoinRule::KnockRestricted(restricted) => {
                        restricted_membership_room(restricted)
                    }
                    _ => None,
                });

            if self
                .membership_room
                .obj()
                .map(|d| d.identifier())
                .as_deref()
                == room_id.as_ref().map(|id| id.as_str())
            {
                return;
            }

            self.membership_room.disconnect_signals();

            if let Some(room_id) = room_id {
                let Some(session) = self.room.upgrade().and_then(|r| r.session()) else {
                    return;
                };

                let room: PillSource = if let Some(room) = session.room_list().get(&room_id) {
                    room.upcast()
                } else {
                    session.remote_cache().room(room_id.into()).upcast()
                };

                let display_name_handler = room.connect_display_name_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_display_name();
                    }
                ));

                self.membership_room.set(room, vec![display_name_handler]);
            }

            self.obj().notify_membership_room();
        }

        /// Update the display name of the join rule.
        fn update_display_name(&self) {
            let value = self.value.get();
            let can_knock = self.can_knock.get();

            let name = match value {
                JoinRuleValue::Invite => {
                    if can_knock {
                        gettext("Only invited users, and users can request an invite")
                    } else {
                        gettext("Only invited users")
                    }
                }
                JoinRuleValue::RoomMembership => {
                    let room_name = self
                        .membership_room
                        .obj()
                        .map(|r| r.display_name())
                        .unwrap_or_default();

                    if can_knock {
                        gettext_f(
                            // Translators: Do NOT translate the content between '{' and '}',
                            // this is a variable name.
                            "Members of {room}, and users can request an invite",
                            &[("room", &format!("<b>{room_name}</b>"))],
                        )
                    } else {
                        gettext_f(
                            // Translators: Do NOT translate the content between '{' and '}',
                            // this is a variable name.
                            "Members of {room}",
                            &[("room", &format!("<b>{room_name}</b>"))],
                        )
                    }
                }
                JoinRuleValue::Public => gettext("Any registered user"),
                JoinRuleValue::Unsupported => gettext("Unsupported rule"),
            };

            if *self.display_name.borrow() == name {
                return;
            }

            self.display_name.replace(name);
            self.obj().notify_display_name();
        }

        /// Update whether our own user can join this room on their own.
        fn update_we_can_join(&self) {
            let we_can_join = self.we_can_join();

            if self.we_can_join.get() == we_can_join {
                return;
            }

            self.we_can_join.set(we_can_join);
            self.obj().notify_we_can_join();
        }

        /// Whether our own user can join this room on their own.
        fn we_can_join(&self) -> bool {
            let Some(matrix_join_rule) = self.matrix_join_rule() else {
                return false;
            };
            let Some(room) = self.room.upgrade() else {
                return false;
            };

            if room.own_member().membership() == Membership::Ban {
                return false;
            }

            match matrix_join_rule {
                MatrixJoinRule::Public => true,
                MatrixJoinRule::Restricted(rules) | MatrixJoinRule::KnockRestricted(rules) => rules
                    .allow
                    .into_iter()
                    .any(|rule| we_pass_restricted_allow_rule(&room, rule)),
                _ => false,
            }
        }

        /// Update whether our own user can join this room on their own.
        fn update_anyone_can_join(&self) {
            let anyone_can_join = self
                .matrix_join_rule
                .borrow()
                .as_ref()
                .is_some_and(|r| *r == MatrixJoinRule::Public);

            if self.anyone_can_join.get() == anyone_can_join {
                return;
            }

            self.anyone_can_join.set(anyone_can_join);
            self.obj().notify_anyone_can_join();
        }
    }
}

glib::wrapper! {
    /// The join rule of a room.
    pub struct JoinRule(ObjectSubclass<imp::JoinRule>);
}

impl JoinRule {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Initialize the join rule with the room where it applies.
    pub(super) fn init(&self, room: &Room) {
        self.imp().set_room(room);
    }

    /// Update the join rule with the given value from the SDK.
    pub(super) fn update(&self, join_rule: Option<&MatrixJoinRule>) {
        self.imp().update_join_rule(join_rule);
    }

    /// Get the current join rule from the SDK.
    pub(crate) fn matrix_join_rule(&self) -> Option<MatrixJoinRule> {
        self.imp().matrix_join_rule()
    }

    /// Change the join rule.
    pub(crate) async fn set_matrix_join_rule(&self, rule: MatrixJoinRule) -> Result<(), ()> {
        let Some(room) = self.room() else {
            return Err(());
        };

        let content = RoomJoinRulesEventContent::new(rule);

        let matrix_room = room.matrix_room().clone();
        let handle = spawn_tokio!(async move { matrix_room.send_state_event(content).await });

        match handle.await.expect("task was not aborted") {
            Ok(_) => Ok(()),
            Err(error) => {
                error!("Could not change join rule: {error}");
                Err(())
            }
        }
    }

    /// Connect to the signal emitted when the join rule changed.
    pub(crate) fn connect_changed<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "changed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}

impl Default for JoinRule {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether the given restricted rule allows a room membership.
fn has_restricted_membership_room(restricted: &Restricted) -> bool {
    restricted
        .allow
        .iter()
        .any(|a| matches!(a, AllowRule::RoomMembership(_)))
}

/// The ID of the first room, if the given restricted rule allows a room
/// membership.
fn restricted_membership_room(restricted: &Restricted) -> Option<OwnedRoomId> {
    restricted.allow.iter().find_map(|a| match a {
        AllowRule::RoomMembership(m) => Some(m.room_id.clone()),
        _ => None,
    })
}

/// Whether our account passes the given restricted allow rule.
fn we_pass_restricted_allow_rule(room: &Room, rule: AllowRule) -> bool {
    match rule {
        AllowRule::RoomMembership(room_membership) => room.session().is_some_and(|s| {
            s.room_list()
                .get_by_identifier((&*room_membership.room_id).into())
                .is_some_and(|room| room.is_joined())
        }),
        _ => false,
    }
}
