use futures_util::StreamExt;
use gtk::{glib, glib::clone, prelude::*, subclass::prelude::*};
use matrix_sdk::encryption::identities::UserDevices;
use ruma::{OwnedDeviceId, OwnedUserId};
use tokio::task::AbortHandle;
use tracing::error;

mod other_sessions_list;
mod user_session;

use self::user_session::UserSessionData;
pub use self::{other_sessions_list::OtherSessionsList, user_session::UserSession};
use super::Session;
use crate::{prelude::*, spawn, spawn_tokio, utils::LoadingState};

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        collections::HashMap,
        marker::PhantomData,
    };

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::UserSessionsList)]
    pub struct UserSessionsList {
        /// The current session.
        #[property(get)]
        session: glib::WeakRef<Session>,
        /// The ID of the user the sessions belong to.
        user_id: OnceCell<OwnedUserId>,
        /// The other user sessions.
        #[property(get)]
        other_sessions: OtherSessionsList,
        /// The current user session.
        #[property(get)]
        current_session: RefCell<Option<UserSession>>,
        /// The loading state of the list.
        #[property(get, builder(LoadingState::default()))]
        loading_state: Cell<LoadingState>,
        /// Whether the list is empty.
        #[property(get = Self::is_empty)]
        is_empty: PhantomData<bool>,
        sessions_watch_abort_handle: RefCell<Option<AbortHandle>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for UserSessionsList {
        const NAME: &'static str = "UserSessionsList";
        type Type = super::UserSessionsList;
    }

    #[glib::derived_properties]
    impl ObjectImpl for UserSessionsList {
        fn dispose(&self) {
            if let Some(abort_handle) = self.sessions_watch_abort_handle.take() {
                abort_handle.abort();
            }
        }
    }

    impl UserSessionsList {
        /// Initialize this list with the given session and user ID.
        pub(super) fn init(&self, session: &Session, user_id: OwnedUserId) {
            self.session.set(Some(session));
            let user_id = self.user_id.get_or_init(|| user_id);

            // We know that we have at least this session for our own user.
            if session.user_id() == user_id {
                let current_session = UserSession::new(session, session.device_id().clone());
                self.current_session.replace(Some(current_session));
            }

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.load().await;
                }
            ));
            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.watch_sessions().await;
                }
            ));
        }

        /// The ID of the user the sessions belong to.
        fn user_id(&self) -> &OwnedUserId {
            self.user_id.get().expect("user ID is initialized")
        }

        /// Listen to changes in the user sessions.
        async fn watch_sessions(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let client = session.client();
            let stream = match client.encryption().devices_stream().await {
                Ok(stream) => stream,
                Err(error) => {
                    error!("Could not access the user sessions stream: {error}");
                    return;
                }
            };

            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let user_id = self.user_id().clone();
            let fut = stream.for_each(move |updates| {
                let user_id = user_id.clone();
                let obj_weak = obj_weak.clone();

                async move {
                    // If a device update is received for an account different than the one
                    // for which the settings are currently opened, we don't want to reload the user
                    // sessions, to save bandwidth.
                    // However, when a device is disconnected, an empty device update is received.
                    // In this case, we do not know which account had a device disconnection, so we
                    // want to reload the sessions just in case.
                    if !updates.new.contains_key(&user_id)
                        && !updates.changed.contains_key(&user_id)
                        && (!updates.new.is_empty() || !updates.changed.is_empty())
                    {
                        return;
                    }

                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            if let Some(obj) = obj_weak.upgrade() {
                                obj.imp().load().await;
                            }
                        });
                    });
                }
            });

            let abort_handle = spawn_tokio!(fut).abort_handle();
            self.sessions_watch_abort_handle.replace(Some(abort_handle));
        }

        /// Load the list of user sessions.
        pub(super) async fn load(&self) {
            if self.loading_state.get() == LoadingState::Loading {
                // Do not load the list twice at the same time.
                return;
            }

            let Some(session) = self.session.upgrade() else {
                return;
            };

            self.set_loading_state(LoadingState::Loading);

            let user_id = self.user_id().clone();
            let client = session.client();
            let handle = spawn_tokio!(async move {
                // Load the crypto sessions, to know whether the device is encrypted or not.
                let crypto_sessions = match client.encryption().get_user_devices(&user_id).await {
                    Ok(crypto_sessions) => Some(crypto_sessions),
                    Err(error) => {
                        error!("Could not get crypto sessions for user {user_id}: {error}");
                        None
                    }
                };

                let is_own_user = client.user_id().unwrap() == user_id;

                let mut api_sessions = None;
                if is_own_user {
                    // Load the session information, to get the display name and last seen info.
                    match client.devices().await {
                        Ok(response) => {
                            api_sessions = Some(response.devices);
                        }
                        Err(error) => {
                            error!("Could not get sessions list for user {user_id}: {error}");
                        }
                    }
                }

                (api_sessions, crypto_sessions)
            });

            let (api_sessions, crypto_sessions) = handle.await.unwrap();

            if api_sessions.is_none() && crypto_sessions.is_none() {
                self.set_loading_state(LoadingState::Error);
                return;
            }

            // Convert API sessions to a map.
            let mut api_sessions = api_sessions
                .into_iter()
                .flatten()
                .map(|d| (d.device_id.clone(), d))
                .collect::<HashMap<_, _>>();

            let is_own_user = session.user_id() == self.user_id();
            let own_device_id = session.device_id();
            let mut current_session_data = None;

            // If we have the API sessions, use their length to reserve the capacity because
            // it is cheaper. Otherwise we need to count the list of crypto sessions.
            let api_sessions_len = api_sessions.len();
            let capacity = if api_sessions_len > 0 {
                api_sessions_len
            } else {
                crypto_sessions.iter().flat_map(UserDevices::keys).count()
            };
            let mut other_sessions_data = Vec::with_capacity(capacity);

            // First, handle the list of devices with a cryptographic identity, i.e. devices
            // that support encryption.
            for crypto in crypto_sessions.iter().flat_map(UserDevices::devices) {
                let data = if let Some(api) = api_sessions.remove(crypto.device_id()) {
                    UserSessionData::Both { api, crypto }
                } else {
                    UserSessionData::Crypto(crypto)
                };

                if is_own_user && data.device_id() == own_device_id {
                    current_session_data = Some(data);
                } else {
                    other_sessions_data.push(data);
                }
            }

            // If there are remaining devices through the device information API, they do
            // not support encryption.
            for api in api_sessions.into_values() {
                let data = UserSessionData::DevicesApi(api);

                if is_own_user && data.device_id() == own_device_id {
                    current_session_data = Some(data);
                } else {
                    other_sessions_data.push(data);
                }
            }

            if let Some((session, data)) = current_session_data
                .and_then(|data| self.current_session.borrow().clone().zip(Some(data)))
            {
                session.set_data(data);
            }

            let was_empty = self.is_empty();

            self.other_sessions.update(&session, other_sessions_data);

            if self.is_empty() != was_empty {
                self.obj().notify_is_empty();
            }

            self.set_loading_state(LoadingState::Ready);
        }

        /// Find the user session with the given device ID, if any.
        pub(super) fn get(&self, device_id: &OwnedDeviceId) -> Option<UserSession> {
            if let Some(current_session) = self.current_session.borrow().as_ref()
                && current_session.device_id() == device_id
            {
                return Some(current_session.clone());
            }

            self.other_sessions.get(device_id)
        }

        /// Set the loading state of the list.
        fn set_loading_state(&self, loading_state: LoadingState) {
            if self.loading_state.get() == loading_state {
                return;
            }

            self.loading_state.set(loading_state);
            self.obj().notify_loading_state();
        }

        /// Whether the list is empty.
        fn is_empty(&self) -> bool {
            self.current_session.borrow().is_none() && self.other_sessions.n_items() == 0
        }
    }
}

glib::wrapper! {
    /// List of active user sessions for a user.
    pub struct UserSessionsList(ObjectSubclass<imp::UserSessionsList>);
}

impl UserSessionsList {
    /// Construct a new empty `UserSessionsList`.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Initialize this list with the given session and user ID.
    pub(crate) fn init(&self, session: &Session, user_id: OwnedUserId) {
        self.imp().init(session, user_id);
    }

    /// Load the list of user sessions.
    pub(crate) async fn load(&self) {
        self.imp().load().await;
    }

    /// Find the user session with the given device ID, if any.
    pub(crate) fn get(&self, device_id: &OwnedDeviceId) -> Option<UserSession> {
        self.imp().get(device_id)
    }
}

impl Default for UserSessionsList {
    fn default() -> Self {
        Self::new()
    }
}
