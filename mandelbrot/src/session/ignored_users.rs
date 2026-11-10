use futures_util::StreamExt;
use gtk::{
    gio,
    glib::{self, clone},
    prelude::*,
    subclass::prelude::*,
};
use indexmap::IndexSet;
use ruma::{OwnedUserId, events::ignored_user_list::IgnoredUserListEventContent};
use tracing::{debug, error, warn};

use super::Session;
use crate::{spawn, spawn_tokio};

mod imp {
    use std::cell::RefCell;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::IgnoredUsers)]
    pub struct IgnoredUsers {
        /// The current session.
        #[property(get, set = Self::set_session, explicit_notify, nullable)]
        pub session: glib::WeakRef<Session>,
        /// The content of the ignored user list event.
        pub list: RefCell<IndexSet<OwnedUserId>>,
        abort_handle: RefCell<Option<tokio::task::AbortHandle>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for IgnoredUsers {
        const NAME: &'static str = "IgnoredUsers";
        type Type = super::IgnoredUsers;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for IgnoredUsers {
        fn dispose(&self) {
            if let Some(abort_handle) = self.abort_handle.take() {
                abort_handle.abort();
            }
        }
    }

    impl ListModelImpl for IgnoredUsers {
        fn item_type(&self) -> glib::Type {
            gtk::StringObject::static_type()
        }

        fn n_items(&self) -> u32 {
            self.list.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.list
                .borrow()
                .get_index(position as usize)
                .map(|user_id| gtk::StringObject::new(user_id.as_str()).upcast())
        }
    }

    impl IgnoredUsers {
        /// Set the current session.
        fn set_session(&self, session: Option<&Session>) {
            if self.session.upgrade().as_ref() == session {
                return;
            }

            self.session.set(session);

            self.init();
            self.obj().notify_session();
        }

        /// Listen to changes of the ignored users list.
        fn init(&self) {
            if let Some(abort_handle) = self.abort_handle.take() {
                abort_handle.abort();
            }

            let Some(session) = self.session.upgrade() else {
                return;
            };
            let obj = self.obj();

            let obj_weak = glib::SendWeakRef::from(obj.downgrade());
            let subscriber = session.client().subscribe_to_ignore_user_list_changes();
            let fut = subscriber.for_each(move |_| {
                let obj_weak = obj_weak.clone();
                async move {
                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            if let Some(obj) = obj_weak.upgrade() {
                                obj.imp().load_list().await;
                            }
                        });
                    });
                }
            });

            let abort_handle = spawn_tokio!(fut).abort_handle();
            self.abort_handle.replace(Some(abort_handle));

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.load_list().await;
                }
            ));
        }

        /// Load the list from the store and update it.
        async fn load_list(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let client = session.client();
            let handle = spawn_tokio!(async move {
                client
                    .account()
                    .account_data::<IgnoredUserListEventContent>()
                    .await
            });

            let raw = match handle.await.unwrap() {
                Ok(Some(raw)) => raw,
                Ok(None) => {
                    debug!("Got no ignored users list");
                    self.update_list(IndexSet::new());
                    return;
                }
                Err(error) => {
                    error!("Could not get ignored users list: {error}");
                    return;
                }
            };

            match raw.deserialize() {
                Ok(content) => self.update_list(content.ignored_users.into_keys().collect()),
                Err(error) => {
                    error!("Could not deserialize ignored users list: {error}");
                }
            }
        }

        /// Update the list with the given new list.
        fn update_list(&self, new_list: IndexSet<OwnedUserId>) {
            if *self.list.borrow() == new_list {
                return;
            }

            let old_len = self.n_items();
            let new_len = new_list.len() as u32;

            let mut pos = 0;
            {
                let old_list = self.list.borrow();

                for old_item in old_list.iter() {
                    let Some(new_item) = new_list.get_index(pos as usize) else {
                        break;
                    };

                    if old_item != new_item {
                        break;
                    }

                    pos += 1;
                }
            }

            if old_len == new_len && pos == new_len {
                // Nothing changed.
                return;
            }

            self.list.replace(new_list);

            self.obj().items_changed(
                pos,
                old_len.saturating_sub(pos),
                new_len.saturating_sub(pos),
            );
        }
    }
}

glib::wrapper! {
    /// The list of ignored users of a `Session`.
    pub struct IgnoredUsers(ObjectSubclass<imp::IgnoredUsers>)
        @implements gio::ListModel;
}

impl IgnoredUsers {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Whether this list contains the given user ID.
    pub fn contains(&self, user_id: &OwnedUserId) -> bool {
        self.imp().list.borrow().contains(user_id)
    }

    /// Add the user with the given ID to the list.
    pub async fn add(&self, user_id: &OwnedUserId) -> Result<(), ()> {
        let Some(session) = self.session() else {
            return Err(());
        };

        if self.contains(user_id) {
            warn!(
                "Trying to add `{user_id}` to the ignored users but they are already in the list, ignoring"
            );
            return Ok(());
        }

        let client = session.client();
        let user_id_clone = user_id.clone();
        let handle =
            spawn_tokio!(async move { client.account().ignore_user(&user_id_clone).await });

        match handle.await.unwrap() {
            Ok(()) => {
                let (pos, added) = self.imp().list.borrow_mut().insert_full(user_id.clone());

                if added {
                    self.items_changed(pos as u32, 0, 1);
                }
                Ok(())
            }
            Err(error) => {
                error!("Could not add `{user_id}` to the ignored users: {error}");
                Err(())
            }
        }
    }

    /// Remove the user with the given ID from the list.
    pub async fn remove(&self, user_id: &OwnedUserId) -> Result<(), ()> {
        let Some(session) = self.session() else {
            return Err(());
        };

        if !self.contains(user_id) {
            warn!(
                "Trying to remove `{user_id}` from the ignored users but they are not in the list, ignoring"
            );
            return Ok(());
        }

        let client = session.client();
        let user_id_clone = user_id.clone();
        let handle =
            spawn_tokio!(async move { client.account().unignore_user(&user_id_clone).await });

        match handle.await.unwrap() {
            Ok(()) => {
                let removed = self.imp().list.borrow_mut().shift_remove_full(user_id);

                if let Some((pos, _)) = removed {
                    self.items_changed(pos as u32, 1, 0);
                }
                Ok(())
            }
            Err(error) => {
                error!("Could not remove `{user_id}` from the ignored users: {error}");
                Err(())
            }
        }
    }
}

impl Default for IgnoredUsers {
    fn default() -> Self {
        Self::new()
    }
}
