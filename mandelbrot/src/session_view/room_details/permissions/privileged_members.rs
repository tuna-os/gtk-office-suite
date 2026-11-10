use std::collections::BTreeMap;

use adw::subclass::prelude::*;
use gtk::{gio, glib, glib::clone, prelude::*};
use indexmap::IndexMap;
use ruma::{Int, OwnedUserId};

use super::MemberPowerLevel;
use crate::{
    session::{Permissions, User},
    utils::BoundObjectWeakRef,
};

mod imp {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::PrivilegedMembers)]
    pub struct PrivilegedMembers {
        /// The list of members.
        pub(super) list: RefCell<IndexMap<OwnedUserId, MemberPowerLevel>>,
        /// The permissions to watch.
        #[property(get, set = Self::set_permissions, construct_only)]
        permissions: BoundObjectWeakRef<Permissions>,
        /// Whether this list has changed.
        #[property(get)]
        changed: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PrivilegedMembers {
        const NAME: &'static str = "RoomDetailsPermissionsPrivilegedMembers";
        type Type = super::PrivilegedMembers;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for PrivilegedMembers {}

    impl ListModelImpl for PrivilegedMembers {
        fn item_type(&self) -> glib::Type {
            MemberPowerLevel::static_type()
        }

        fn n_items(&self) -> u32 {
            self.list.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.list
                .borrow()
                .get_index(position as usize)
                .map(|(_, member)| member.clone().upcast())
        }
    }

    impl PrivilegedMembers {
        /// Set the permissions to watch.
        fn set_permissions(&self, permissions: &Permissions) {
            let changed_handler = permissions.connect_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update();
                }
            ));
            self.permissions.set(permissions, vec![changed_handler]);

            self.update();
        }

        /// Update this list.
        fn update(&self) {
            let Some(permissions) = self.permissions.obj() else {
                return;
            };
            let Some(room) = permissions.room() else {
                return;
            };
            let Some(session) = room.session() else {
                return;
            };

            let members = room.get_or_create_members();
            let mut users = permissions.power_levels().users;

            let mut removed_users = Vec::new();
            {
                for user_id in self.list.borrow().keys() {
                    if !users.contains_key(user_id) {
                        removed_users.push(user_id.clone());
                        continue;
                    }

                    // Remove known members so only new members are left.
                    users.remove(user_id);
                }
            }

            for user_id in removed_users {
                self.remove_member(&user_id);
            }

            // Only new members are remaining.
            let mut new_handlers = Vec::with_capacity(users.len());
            let new_members = users.into_keys().map(|user_id| {
                let user = members
                    .get(&user_id)
                    .and_upcast::<User>()
                    .unwrap_or_else(|| {
                        // Fallback to the remote cache if the user is not in the room anymore.
                        session.remote_cache().user(user_id.clone()).upcast()
                    });
                let member = MemberPowerLevel::new(&user, &permissions);

                let handler = member.connect_power_level_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_changed();
                    }
                ));
                new_handlers.push(handler);

                (user_id, member)
            });

            self.add_members(new_members);
        }

        /// Remove the member with the given user ID from the list.
        fn remove_member(&self, user_id: &OwnedUserId) {
            let Some((pos, ..)) = self.list.borrow_mut().shift_remove_full(user_id) else {
                return;
            };

            self.obj().items_changed(pos as u32, 1, 0);
        }

        /// Add the given members to the list.
        pub(super) fn add_members(
            &self,
            members: impl ExactSizeIterator<Item = (OwnedUserId, MemberPowerLevel)>,
        ) {
            let pos = self.n_items();
            let added = members.len() as u32;

            self.list.borrow_mut().extend(members);

            self.update_changed();
            self.obj().items_changed(pos, 0, added);
        }

        /// Update whether the list has changed.
        fn update_changed(&self) {
            let changed = self.compute_changed();

            if self.changed.get() == changed {
                return;
            }

            self.changed.set(changed);
            self.obj().notify_changed();
        }

        /// Compute whether the list has changed.
        fn compute_changed(&self) -> bool {
            let Some(permissions) = self.permissions.obj() else {
                return false;
            };

            let users = permissions.power_levels().users;
            let list = self.list.borrow();

            if users.len() != list.len() {
                return true;
            }

            for (user_id, member) in list.iter() {
                let Some(pl) = users.get(user_id) else {
                    // This is a new member.
                    return true;
                };

                if member.power_level() != *pl {
                    return true;
                }
            }

            false
        }
    }
}

glib::wrapper! {
    /// The list of members with custom power levels in a room.
    pub struct PrivilegedMembers(ObjectSubclass<imp::PrivilegedMembers>)
        @implements gio::ListModel;
}

impl PrivilegedMembers {
    /// Constructs a new `PrivilegedMembers` with the given permissions.
    pub fn new(permissions: &Permissions) -> Self {
        glib::Object::builder()
            .property("permissions", permissions)
            .build()
    }

    /// Add the given members to the list.
    pub(crate) fn add_members(
        &self,
        members: impl ExactSizeIterator<Item = (OwnedUserId, MemberPowerLevel)>,
    ) {
        let imp = self.imp();

        let mut new_members = Vec::with_capacity(members.len());

        {
            let list = imp.list.borrow();
            for (user_id, new_member) in members {
                if let Some(member) = list.get(&user_id) {
                    member.set_power_level(new_member.power_level());
                } else {
                    new_members.push((user_id, new_member));
                }
            }
        }

        if !new_members.is_empty() {
            self.imp().add_members(new_members.into_iter());
        }
    }

    /// Collect the list of members.
    pub(crate) fn collect(&self) -> BTreeMap<OwnedUserId, Int> {
        self.imp()
            .list
            .borrow()
            .values()
            .filter_map(MemberPowerLevel::to_parts)
            .collect()
    }
}
