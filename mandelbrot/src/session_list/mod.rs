use std::{cmp::Ordering, ffi::OsString};

use gettextrs::gettext;
use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};
use indexmap::map::IndexMap;
use tracing::{error, info};

mod failed_session;
mod new_session;
mod session_info;
mod session_list_settings;

pub(crate) use self::{
    failed_session::*, new_session::*, session_info::*, session_list_settings::*,
};
use crate::{
    prelude::*,
    secret::{Secret, StoredSession},
    session::{Session, SessionState},
    spawn, spawn_tokio,
    utils::{DataType, LoadingState},
};

mod imp {
    use std::{
        cell::{Cell, RefCell},
        marker::PhantomData,
    };

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::SessionList)]
    pub struct SessionList {
        /// The map of session ID to session.
        pub(super) list: RefCell<IndexMap<String, SessionInfo>>,
        /// The loading state of the list.
        #[property(get, builder(LoadingState::default()))]
        state: Cell<LoadingState>,
        /// The error message, if state is set to `LoadingState::Error`.
        #[property(get, nullable)]
        error: RefCell<Option<String>>,
        /// The settings of the sessions.
        #[property(get)]
        settings: SessionListSettings,
        /// Whether this list is empty.
        #[property(get = Self::is_empty)]
        is_empty: PhantomData<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SessionList {
        const NAME: &'static str = "SessionList";
        type Type = super::SessionList;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for SessionList {}

    impl ListModelImpl for SessionList {
        fn item_type(&self) -> glib::Type {
            SessionInfo::static_type()
        }

        fn n_items(&self) -> u32 {
            self.list.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.list
                .borrow()
                .get_index(position as usize)
                .map(|(_, v)| v.clone().upcast())
        }
    }

    impl SessionList {
        /// Whether this list is empty.
        fn is_empty(&self) -> bool {
            self.list.borrow().is_empty()
        }

        /// Set the loading state of this list.
        fn set_state(&self, state: LoadingState) {
            if self.state.get() == state {
                return;
            }

            self.state.set(state);
            self.obj().notify_state();
        }

        /// Set the error message.
        fn set_error(&self, message: String) {
            self.error.replace(Some(message));

            self.obj().notify_error();
            self.set_state(LoadingState::Error);
        }

        /// Insert the given session into the list.
        ///
        /// If a session with the same ID already exists, it is replaced.
        ///
        /// Returns the index of the session.
        pub(super) fn insert(&self, session: impl IsA<SessionInfo>) -> usize {
            let session = session.upcast();

            if let Some(session) = session.downcast_ref::<Session>() {
                session.connect_logged_out(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |session| imp.remove(session.session_id())
                ));
            }

            let was_empty = self.is_empty();

            let (index, replaced) = self
                .list
                .borrow_mut()
                .insert_full(session.session_id(), session);

            let removed = replaced.is_some().into();

            let obj = self.obj();
            obj.items_changed(index as u32, removed, 1);

            if was_empty {
                obj.notify_is_empty();
            }

            index
        }

        /// Remove the session with the given ID from the list.
        fn remove(&self, session_id: &str) {
            let removed = self.list.borrow_mut().shift_remove_full(session_id);

            if let Some((position, ..)) = removed {
                let obj = self.obj();
                obj.items_changed(position as u32, 1, 0);

                if self.is_empty() {
                    obj.notify_is_empty();
                }
            }
        }

        /// Restore the logged-in sessions.
        pub(super) async fn restore_sessions(&self) {
            if self.state.get() >= LoadingState::Loading {
                return;
            }

            self.set_state(LoadingState::Loading);

            let mut sessions = match Secret::restore_sessions().await {
                Ok(sessions) => sessions,
                Err(error) => {
                    let message = format!(
                        "{}\n\n{}",
                        gettext("Could not restore previous sessions"),
                        error.to_user_facing(),
                    );

                    self.set_error(message);
                    return;
                }
            };

            self.settings.load();
            let session_ids = self.settings.session_ids();

            // Keep the order from the settings.
            sessions.sort_by(|a, b| {
                let pos_a = session_ids.get_index_of(&a.id);
                let pos_b = session_ids.get_index_of(&b.id);

                match (pos_a, pos_b) {
                    (Some(pos_a), Some(pos_b)) => pos_a.cmp(&pos_b),
                    // Keep unknown sessions at the end.
                    (Some(_), None) => Ordering::Greater,
                    (None, Some(_)) => Ordering::Less,
                    _ => Ordering::Equal,
                }
            });

            // Get the directories present in the data path to only restore sessions with
            // data on the system. This is necessary for users sharing their secrets between
            // devices.
            let mut directories = match self.data_directories(sessions.len()).await {
                Ok(directories) => directories,
                Err(error) => {
                    error!("Could not access data directory: {error}");
                    let message = format!(
                        "{}\n\n{}",
                        gettext("Could not restore previous sessions"),
                        gettext("An unexpected error happened while accessing the data directory"),
                    );

                    self.set_error(message);
                    return;
                }
            };

            for stored_session in sessions {
                if let Some(pos) = directories
                    .iter()
                    .position(|dir_name| dir_name == stored_session.id.as_str())
                {
                    directories.swap_remove(pos);
                    info!(
                        "Restoring previous session {} for user {}",
                        stored_session.id, stored_session.user_id,
                    );
                    self.insert(NewSession::new(&stored_session));

                    spawn!(
                        glib::Priority::DEFAULT_IDLE,
                        clone!(
                            #[weak(rename_to = obj)]
                            self,
                            async move {
                                obj.restore_stored_session(&stored_session).await;
                            }
                        )
                    );
                } else {
                    info!(
                        "Ignoring session {} for user {}: no data directory",
                        stored_session.id, stored_session.user_id,
                    );
                }
            }

            self.set_state(LoadingState::Ready);
        }

        /// The list of directories in the data directory.
        async fn data_directories(&self, capacity: usize) -> std::io::Result<Vec<OsString>> {
            let data_path = DataType::Persistent.dir_path();

            if !data_path.try_exists()? {
                return Ok(Vec::new());
            }

            spawn_tokio!(async move {
                let mut read_dir = tokio::fs::read_dir(data_path).await?;
                let mut directories = Vec::with_capacity(capacity);

                loop {
                    let Some(entry) = read_dir.next_entry().await? else {
                        // We are at the end of the list.
                        break;
                    };

                    if !entry.file_type().await?.is_dir() {
                        // We are only interested in directories.
                        continue;
                    }

                    directories.push(entry.file_name());
                }

                std::io::Result::Ok(directories)
            })
            .await
            .expect("task was not aborted")
        }

        /// Restore a stored session.
        async fn restore_stored_session(&self, session_info: &StoredSession) {
            let settings = self.settings.get_or_create(&session_info.id);
            match Session::new(session_info.clone(), settings).await {
                Ok(session) => {
                    session.prepare().await;
                    self.insert(session);
                }
                Err(error) => {
                    error!("Could not restore previous session: {error}");
                    self.insert(FailedSession::new(session_info, error));
                }
            }
        }
    }
}

glib::wrapper! {
    /// List of all logged in sessions.
    pub struct SessionList(ObjectSubclass<imp::SessionList>)
        @implements gio::ListModel;
}

impl SessionList {
    /// Create a new empty `SessionList`.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Whether at least one session is ready.
    pub(crate) fn has_session_ready(&self) -> bool {
        self.imp()
            .list
            .borrow()
            .values()
            .filter_map(|s| s.downcast_ref::<Session>())
            .any(|s| s.state() == SessionState::Ready)
    }

    /// The session with the given ID, if any.
    pub(crate) fn get(&self, session_id: &str) -> Option<SessionInfo> {
        self.imp().list.borrow().get(session_id).cloned()
    }

    /// The index of the session with the given ID, if any.
    pub(crate) fn index(&self, session_id: &str) -> Option<usize> {
        self.imp().list.borrow().get_index_of(session_id)
    }

    /// The first session in the list, if any.
    pub(crate) fn first(&self) -> Option<SessionInfo> {
        self.imp().list.borrow().first().map(|(_, v)| v.clone())
    }

    /// Insert the given session into the list.
    ///
    /// If a session with the same ID already exists, it is replaced.
    ///
    /// Returns the index of the session.
    pub(crate) fn insert(&self, session: impl IsA<SessionInfo>) -> usize {
        self.imp().insert(session)
    }

    /// Restore the logged-in sessions.
    pub(crate) async fn restore_sessions(&self) {
        self.imp().restore_sessions().await;
    }
}

impl Default for SessionList {
    fn default() -> Self {
        Self::new()
    }
}
