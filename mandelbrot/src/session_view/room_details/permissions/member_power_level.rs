use adw::subclass::prelude::*;
use gtk::{
    glib,
    glib::{clone, closure_local},
    prelude::*,
};
use ruma::{
    Int, OwnedUserId,
    events::room::power_levels::{PowerLevelUserAction, UserPowerLevel},
    int,
};
use tracing::error;

use crate::{
    prelude::*,
    session::{MemberRole, Permissions, User},
    utils::BoundObjectWeakRef,
};

mod imp {
    use std::{
        cell::{Cell, OnceCell},
        marker::PhantomData,
        sync::LazyLock,
    };

    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, glib::Properties)]
    #[properties(wrapper_type = super::MemberPowerLevel)]
    pub struct MemberPowerLevel {
        /// The permissions to watch.
        #[property(get, set = Self::set_permissions, construct_only)]
        permissions: BoundObjectWeakRef<Permissions>,
        /// The room member or remote user.
        #[property(get, construct_only)]
        user: OnceCell<User>,
        /// The wanted power level of the member.
        ///
        /// Initially, it should be the same as the member's, but can change
        /// independently.
        pub(super) power_level: Cell<UserPowerLevel>,
        /// The wanted power level of the member, as an `i64`.
        ///
        /// Should only be used for sorting.
        ///
        /// `i64::MAX` is used to represent an infinite power level, since it
        /// cannot be reached with the Matrix specification.
        #[property(get = Self::power_level_i64)]
        power_level_i64: PhantomData<i64>,
        /// The wanted role of the member.
        #[property(get, builder(MemberRole::default()))]
        role: Cell<MemberRole>,
        /// Whether this member's power level can be edited.
        #[property(get)]
        editable: Cell<bool>,
    }

    impl Default for MemberPowerLevel {
        fn default() -> Self {
            Self {
                permissions: Default::default(),
                user: Default::default(),
                power_level: Cell::new(UserPowerLevel::Int(int!(0))),
                power_level_i64: Default::default(),
                role: Default::default(),
                editable: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MemberPowerLevel {
        const NAME: &'static str = "RoomDetailsPermissionsMemberPowerLevel";
        type Type = super::MemberPowerLevel;
    }

    #[glib::derived_properties]
    impl ObjectImpl for MemberPowerLevel {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("power-level-changed").build()]);
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.update_power_level();
            self.update_role();
            self.update_editable();
        }
    }

    impl MemberPowerLevel {
        /// Set the room member.
        fn set_permissions(&self, permissions: &Permissions) {
            let changed_handler = permissions.connect_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_power_level();
                    imp.update_role();
                    imp.update_editable();
                }
            ));
            self.permissions.set(permissions, vec![changed_handler]);
        }

        /// Update the wanted power level of the member.
        fn update_power_level(&self) {
            let Some(user) = self.user.get() else {
                return;
            };
            let Some(permissions) = self.permissions.obj() else {
                return;
            };

            self.set_power_level(permissions.user_power_level(user.user_id()));
        }

        /// Set the wanted power level of the member.
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

        /// The wanted power level of the member, as an `i64`.
        fn power_level_i64(&self) -> i64 {
            if let UserPowerLevel::Int(power_level) = self.power_level.get() {
                power_level.into()
            } else {
                // Represent the infinite power level with a value out of range for a power
                // level.
                i64::MAX
            }
        }

        /// Update the wanted role of the member.
        fn update_role(&self) {
            let Some(permissions) = self.permissions.obj() else {
                return;
            };

            let role = permissions.role(self.power_level.get());

            if self.role.get() == role {
                return;
            }

            self.role.set(role);
            self.obj().notify_role();
        }

        /// Update whether this member's power level can be edited.
        fn update_editable(&self) {
            let Some(user) = self.user.get() else {
                return;
            };
            let Some(permissions) = self.permissions.obj() else {
                return;
            };

            let editable =
                permissions.can_do_to_user(user.user_id(), PowerLevelUserAction::ChangePowerLevel);

            if self.editable.get() == editable {
                return;
            }

            self.editable.set(editable);
            self.obj().notify_editable();
        }
    }
}

glib::wrapper! {
    /// A room member with a cached wanted power level.
    pub struct MemberPowerLevel(ObjectSubclass<imp::MemberPowerLevel>);
}

impl MemberPowerLevel {
    /// Constructs a new `MemberPowerLevel` with the given user and permissions.
    pub fn new(user: &impl IsA<User>, permissions: &Permissions) -> Self {
        glib::Object::builder()
            .property("user", user)
            .property("permissions", permissions)
            .build()
    }

    /// The wanted power level of the member.
    pub(crate) fn power_level(&self) -> UserPowerLevel {
        self.imp().power_level.get()
    }

    /// Set the wanted power level of the member.
    pub(crate) fn set_power_level(&self, power_level: UserPowerLevel) {
        self.imp().set_power_level(power_level);
    }

    /// Get the parts of this member, to use in the power levels event.
    ///
    /// Returns `None` if the permissions could not be upgraded, or if the power
    /// level is the users default.
    pub(crate) fn to_parts(&self) -> Option<(OwnedUserId, Int)> {
        let permissions = self.permissions()?;

        let UserPowerLevel::Int(power_level) = self.power_level() else {
            error!("Cannot set user power level to infinite");
            return None;
        };

        let users_default = permissions.default_power_level();

        if i64::from(power_level) == users_default {
            return None;
        }

        Some((self.user().user_id().clone(), power_level))
    }

    /// The string to use to search for this member.
    pub(crate) fn search_string(&self) -> String {
        let user = self.user();
        format!("{} {} {}", user.display_name(), user.user_id(), self.role())
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
