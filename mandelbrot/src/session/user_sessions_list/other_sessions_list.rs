use gtk::{gio, glib, prelude::*, subclass::prelude::*};
use ruma::OwnedDeviceId;

use super::{UserSession, UserSessionData};
use crate::session::Session;

mod imp {
    use std::{cell::RefCell, collections::HashSet};

    use indexmap::IndexMap;

    use super::*;

    #[derive(Debug, Default)]
    pub struct OtherSessionsList {
        /// The map of other sessions.
        pub(super) map: RefCell<IndexMap<OwnedDeviceId, UserSession>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for OtherSessionsList {
        const NAME: &'static str = "OtherSessionsList";
        type Type = super::OtherSessionsList;
        type Interfaces = (gio::ListModel,);
    }

    impl ObjectImpl for OtherSessionsList {}

    impl ListModelImpl for OtherSessionsList {
        fn item_type(&self) -> glib::Type {
            UserSession::static_type()
        }

        fn n_items(&self) -> u32 {
            self.map.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.map
                .borrow()
                .get_index(position as usize)
                .map(|(_user_id, member)| member.clone().upcast())
        }
    }

    impl OtherSessionsList {
        /// Update this list to match the given list of data.
        pub(super) fn update(&self, session: &Session, data_list: Vec<UserSessionData>) {
            let n_items = self.n_items();

            // Optimization if the new list is empty.
            if data_list.is_empty() {
                if n_items != 0 {
                    self.map.borrow_mut().clear();
                    self.obj().items_changed(0, n_items, 0);
                }

                return;
            }

            let (added, removed) = {
                let mut map_ref = self.map.borrow_mut();
                let mut old_device_ids = map_ref.keys().cloned().collect::<HashSet<_>>();
                let mut added = 0;

                for data in data_list {
                    old_device_ids.remove(data.device_id());

                    let session = map_ref
                        .entry(data.device_id().to_owned())
                        .or_insert_with_key(|device_id| {
                            added += 1;
                            UserSession::new(session, device_id.clone())
                        });

                    session.set_data(data);
                }

                // If there are old device IDs left, it means that some sessions were
                // disconnected.
                let mut removed = Vec::with_capacity(old_device_ids.len());
                for device_id in old_device_ids {
                    let Some((pos, _, session)) = map_ref.shift_remove_full(&device_id) else {
                        continue;
                    };

                    // We need to drop the reference to the map before notifying about the removal,
                    // to avoid issues with possible side effects. So we keep a list of the removals
                    // for now.
                    removed.push((pos, session));
                }

                (added, removed)
            };

            // Now that the reference to the map is dropped, notify about the changes.
            let obj = self.obj();

            if added > 0 {
                obj.items_changed(n_items, 0, added);
            }

            for (pos, session) in removed {
                obj.items_changed(pos as u32, 1, 0);
                session.emit_disconnected();
            }
        }

        /// Find the user session with the given device ID, if any.
        pub(super) fn get(&self, device_id: &OwnedDeviceId) -> Option<UserSession> {
            self.map.borrow().get(device_id).cloned()
        }
    }
}

glib::wrapper! {
    /// List of active user sessions for a user, except the current one.
    pub struct OtherSessionsList(ObjectSubclass<imp::OtherSessionsList>)
        @implements gio::ListModel;
}

impl OtherSessionsList {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Update this list to match the given list of data.
    pub(super) fn update(&self, session: &Session, data_list: Vec<UserSessionData>) {
        self.imp().update(session, data_list);
    }

    /// Find the user session with the given device ID, if any.
    pub(super) fn get(&self, device_id: &OwnedDeviceId) -> Option<UserSession> {
        self.imp().get(device_id)
    }
}

impl Default for OtherSessionsList {
    fn default() -> Self {
        Self::new()
    }
}
