use std::sync::Arc;

use futures_util::{StreamExt, lock::Mutex};
use gettextrs::gettext;
use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};
use matrix_sdk::{Client, SessionChange, media::MediaRetentionPolicy};
use matrix_sdk_ui::{
    room_list_service,
    sync_service::{self, SyncService},
};
use ruma::api::{
    client::{
        profile::{AvatarUrl, DisplayName},
        search::search_events::v3::UserProfile,
    },
    error::ErrorKind,
};
use tokio::task::AbortHandle;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{debug, error, info};

// TODO: Remove the allow when the call UI is mounted in the room view and
// uses the join/leave API.
#[allow(dead_code)]
mod call;
mod global_account_data;
mod ignored_users;
mod notifications;
mod remote;
mod room;
mod room_list;
mod security;
mod session_settings;
mod sidebar_data;
mod space_hierarchy;
mod sync;
mod user;
mod user_sessions_list;
mod verification;

pub(crate) use self::{
    call::*, global_account_data::*, ignored_users::*, notifications::*, remote::*, room::*,
    room_list::*, security::*, session_settings::*, sidebar_data::*, space_hierarchy::*, user::*,
    user_sessions_list::*, verification::*,
};
use crate::{
    Application,
    components::AvatarData,
    prelude::*,
    secret::StoredSession,
    session_list::{SessionInfo, SessionInfoImpl},
    spawn, spawn_tokio,
    utils::{
        TokioDrop,
        matrix::{self, ClientSetupError},
    },
};

/// The database key for persisting the session's profile.
const SESSION_PROFILE_KEY: &str = "session_profile";
/// The number of consecutive missed synchronizations before the session is
/// marked as offline.
///
/// Note that this is set to `2`, but the count begins at `0` so this would
/// match the third missed synchronization.
const MISSED_SYNC_OFFLINE_COUNT: usize = 2;
/// The delays in seconds to wait for when a sync fails, depending on the number
/// of missed attempts.
const MISSED_SYNC_DELAYS: &[u64] = &[1, 5, 10, 20, 30];

/// The state of the session.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, glib::Enum)]
#[repr(i32)]
#[enum_type(name = "SessionState")]
pub enum SessionState {
    LoggedOut = -1,
    #[default]
    Init = 0,
    InitialSync = 1,
    Ready = 2,
}

/// The method used to synchronize this session with the homeserver.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum SyncMode {
    /// Support for simplified sliding sync was not determined yet.
    #[default]
    Unknown,
    /// Simplified sliding sync (MSC4186), via the SDK's `SyncService`.
    SlidingSync,
    /// Classic sync, via the `/sync` endpoint.
    Classic,
}

/// Wrapper around [`SyncService`] that implements `Debug`.
struct SyncServiceWrapper(Arc<SyncService>);

impl std::fmt::Debug for SyncServiceWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncService").finish_non_exhaustive()
    }
}

/// Whether the given sync service error means that the homeserver does not
/// support simplified sliding sync.
fn is_sliding_sync_unsupported_error(error: &sync_service::Error) -> bool {
    let (sync_service::Error::RoomList(room_list_service::Error::SlidingSync(sdk_error))
    | sync_service::Error::EncryptionSync(
        matrix_sdk_ui::encryption_sync_service::Error::SlidingSync(sdk_error),
    )) = error
    else {
        return false;
    };

    if matches!(
        sdk_error.client_api_error_kind(),
        Some(ErrorKind::Unrecognized)
    ) {
        return true;
    }

    // Some homeservers or reverse proxies reply with a plain 404 instead.
    sdk_error
        .as_client_api_error()
        .is_some_and(|error| error.status_code.as_u16() == 404)
}

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::Session)]
    pub struct Session {
        /// The Matrix client for this session.
        client: OnceCell<TokioDrop<Client>>,
        /// The list model of the sidebar.
        #[property(get = Self::sidebar_list_model)]
        sidebar_list_model: OnceCell<SidebarListModel>,
        /// The user of this session.
        #[property(get = Self::user)]
        user: OnceCell<User>,
        /// The current state of the session.
        #[property(get, builder(SessionState::default()))]
        pub(super) state: Cell<SessionState>,
        /// Whether this session has a connection to the homeserver.
        #[property(get)]
        pub(super) is_homeserver_reachable: Cell<bool>,
        /// Whether this session is synchronized with the homeserver.
        #[property(get)]
        is_offline: Cell<bool>,
        /// The current settings for this session.
        #[property(get, construct_only)]
        settings: OnceCell<SessionSettings>,
        /// The settings in the global account data for this session.
        #[property(get = Self::global_account_data_owned)]
        global_account_data: OnceCell<GlobalAccountData>,
        /// The notifications API for this session.
        #[property(get)]
        notifications: Notifications,
        /// The native calls API for this session.
        #[property(get)]
        call_manager: CallManager,
        /// The ignored users API for this session.
        #[property(get)]
        ignored_users: IgnoredUsers,
        /// The list of sessions for this session's user.
        #[property(get)]
        user_sessions: UserSessionsList,
        /// Information about security for this session.
        #[property(get)]
        security: SessionSecurity,
        /// The cache for remote data.
        remote_cache: OnceCell<RemoteCache>,
        session_changes_handle: RefCell<Option<AbortHandle>>,
        pub(super) sync_handle: RefCell<Option<AbortHandle>>,
        /// The method used to synchronize this session with the homeserver.
        pub(super) sync_mode: Cell<SyncMode>,
        /// The sync service, when simplified sliding sync is used.
        pub(super) sync_service: RefCell<Option<TokioDrop<SyncServiceWrapper>>>,
        pub(super) sync_service_state_handle: RefCell<Option<AbortHandle>>,
        pub(super) room_list_entries_handle: RefCell<Option<AbortHandle>>,
        pub(super) room_updates_handle: RefCell<Option<AbortHandle>>,
        network_monitor_handler_id: RefCell<Option<glib::SignalHandlerId>>,
        homeserver_reachable_lock: Mutex<()>,
        homeserver_reachable_source: RefCell<Option<glib::SourceId>>,
        /// The number of missed synchronizations in a row.
        ///
        /// Capped at `MISSED_SYNC_DELAYS.len() - 1`.
        pub(super) missed_sync_count: Cell<usize>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Session {
        const NAME: &'static str = "Session";
        type Type = super::Session;
        type ParentType = SessionInfo;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Session {
        fn dispose(&self) {
            // Needs to be disconnected or else it may restart the sync
            if let Some(handler_id) = self.network_monitor_handler_id.take() {
                gio::NetworkMonitor::default().disconnect(handler_id);
            }

            if let Some(source) = self.homeserver_reachable_source.take() {
                source.remove();
            }

            if let Some(handle) = self.session_changes_handle.take() {
                handle.abort();
            }

            if let Some(handle) = self.sync_handle.take() {
                handle.abort();
            }

            self.stop_sliding_sync();
        }
    }

    impl SessionInfoImpl for Session {
        fn avatar_data(&self) -> AvatarData {
            self.user().avatar_data().clone()
        }
    }

    impl Session {
        /// Set the Matrix client for this session.
        pub(super) fn set_client(&self, client: Client) {
            self.client
                .set(TokioDrop::new(client))
                .expect("client should be uninitialized");

            let obj = self.obj();

            self.ignored_users.set_session(Some(obj.clone()));
            self.notifications.set_session(Some(obj.clone()));
            self.call_manager.set_session(Some(obj.clone()));
            self.user_sessions.init(&obj, obj.user_id().clone());

            let monitor = gio::NetworkMonitor::default();
            let handler_id = monitor.connect_network_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _| {
                    spawn!(async move {
                        imp.update_homeserver_reachable().await;
                    });
                }
            ));
            self.network_monitor_handler_id.replace(Some(handler_id));
        }

        /// The Matrix client for this session.
        pub(super) fn client(&self) -> &Client {
            self.client.get().expect("client should be initialized")
        }

        /// The list model of the sidebar.
        fn sidebar_list_model(&self) -> SidebarListModel {
            self.sidebar_list_model
                .get_or_init(|| {
                    let obj = self.obj();
                    let item_list =
                        SidebarItemList::new(&RoomList::new(&obj), &VerificationList::new(&obj));
                    SidebarListModel::new(&item_list)
                })
                .clone()
        }

        /// The room list of this session.
        pub(super) fn room_list(&self) -> RoomList {
            self.sidebar_list_model().item_list().room_list()
        }

        /// The verification list of this session.
        pub(super) fn verification_list(&self) -> VerificationList {
            self.sidebar_list_model().item_list().verification_list()
        }

        /// The user of the session.
        fn user(&self) -> User {
            self.user
                .get_or_init(|| {
                    let obj = self.obj();
                    User::new(&obj, obj.info().user_id.clone())
                })
                .clone()
        }

        /// Set the current state of the session.
        pub(super) fn set_state(&self, state: SessionState) {
            let old_state = self.state.get();

            if old_state == SessionState::LoggedOut || old_state == state {
                // The session should be dismissed when it has been logged out, so
                // we do not accept anymore state changes.
                return;
            }

            self.state.set(state);
            self.obj().notify_state();
        }

        /// The homeserver URL as a `GNetworkAddress`.
        fn homeserver_address(&self) -> gio::NetworkAddress {
            let obj = self.obj();
            let homeserver = obj.homeserver();
            let default_port = if homeserver.scheme() == "http" {
                80
            } else {
                443
            };

            gio::NetworkAddress::parse_uri(homeserver.as_str(), default_port)
                .expect("url is parsed successfully")
        }

        /// Check whether the homeserver is reachable.
        pub(super) async fn update_homeserver_reachable(&self) {
            // If there is a timeout, remove it, we will add a new one later if needed.
            if let Some(source) = self.homeserver_reachable_source.take() {
                source.remove();
            }
            let Some(_guard) = self.homeserver_reachable_lock.try_lock() else {
                // There is an ongoing check.
                return;
            };

            let monitor = gio::NetworkMonitor::default();
            let is_network_available = monitor.is_network_available();

            let is_homeserver_reachable = if is_network_available {
                // Check if we can reach the homeserver.
                let address = self.homeserver_address();

                match monitor.can_reach_future(&address).await {
                    Ok(()) => true,
                    Err(error) => {
                        error!(
                            session = self.obj().session_id(),
                            "Homeserver is not reachable: {error}"
                        );
                        false
                    }
                }
            } else {
                false
            };

            self.set_is_homeserver_reachable(is_homeserver_reachable);

            if is_network_available && !is_homeserver_reachable {
                // Check again later if the homeserver is reachable.
                let source = glib::timeout_add_seconds_local_once(
                    10,
                    clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move || {
                            imp.homeserver_reachable_source.take();

                            spawn!(async move {
                                imp.update_homeserver_reachable().await;
                            });
                        }
                    ),
                );
                self.homeserver_reachable_source.replace(Some(source));
            }
        }

        /// Set whether the homeserver is reachable.
        fn set_is_homeserver_reachable(&self, is_reachable: bool) {
            if self.is_homeserver_reachable.get() == is_reachable {
                return;
            }
            let obj = self.obj();

            self.is_homeserver_reachable.set(is_reachable);

            if let Some(handle) = self.sync_handle.take() {
                handle.abort();
            }

            if is_reachable {
                info!(session = obj.session_id(), "Homeserver is reachable");

                // Restart the sync loop.
                self.sync();
            } else {
                // Pause the sync service, it will be restarted when the
                // homeserver is reachable again.
                if let Some(sync_service) = &*self.sync_service.borrow() {
                    let sync_service = sync_service.0.clone();
                    spawn_tokio!(async move {
                        sync_service.stop().await;
                    });
                }

                self.set_offline(true);
            }

            obj.notify_is_homeserver_reachable();
        }

        /// Set whether this session is synchronized with the homeserver.
        pub(super) fn set_offline(&self, is_offline: bool) {
            if self.is_offline.get() == is_offline {
                return;
            }

            if !is_offline {
                // Restart the send queues, in case they were stopped.
                let client = self.client().clone();
                spawn_tokio!(async move {
                    client.send_queue().set_enabled(true).await;
                });
            }

            self.is_offline.set(is_offline);
            self.obj().notify_is_offline();
        }

        /// The settings stored in the global account data for this session.
        fn global_account_data(&self) -> &GlobalAccountData {
            self.global_account_data
                .get_or_init(|| GlobalAccountData::new(&self.obj()))
        }

        /// The owned settings stored in the global account data for this
        /// session.
        fn global_account_data_owned(&self) -> GlobalAccountData {
            self.global_account_data().clone()
        }

        /// The cache for remote data.
        pub(super) fn remote_cache(&self) -> &RemoteCache {
            self.remote_cache
                .get_or_init(|| RemoteCache::new(self.obj().clone()))
        }

        /// Finish initialization of this session.
        pub(super) async fn prepare(&self) {
            spawn!(
                glib::Priority::LOW,
                clone!(
                    #[weak(rename_to = imp)]
                    self,
                    async move {
                        // First, load the profile from the cache, it will be quicker.
                        imp.init_user_profile().await;
                        // Then, check if the profile changed.
                        imp.update_user_profile().await;
                    }
                )
            );

            self.global_account_data();
            self.watch_session_changes();
            self.update_homeserver_reachable().await;

            self.room_list().load().await;
            self.verification_list().init();
            self.security.set_session(Some(&*self.obj()));

            let client = self.client().clone();
            spawn_tokio!(async move {
                client
                    .send_queue()
                    .respawn_tasks_for_rooms_with_unsent_requests()
                    .await;
            });

            self.set_state(SessionState::InitialSync);
            self.sync();

            debug!(
                session = self.obj().session_id(),
                "A new session was prepared"
            );
        }

        /// Watch the changes of the session, like being logged out or the
        /// tokens being refreshed.
        fn watch_session_changes(&self) {
            let receiver = self.client().subscribe_to_session_changes();
            let stream = BroadcastStream::new(receiver);

            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let fut = stream.for_each(move |change| {
                let obj_weak = obj_weak.clone();
                async move {
                    let Ok(change) = change else {
                        return;
                    };

                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            if let Some(obj) = obj_weak.upgrade() {
                                match change {
                                    SessionChange::UnknownToken { .. } => {
                                        info!(
                                            session = obj.session_id(),
                                            "The access token is invalid, cleaning up the session…"
                                        );
                                        obj.imp().clean_up().await;
                                    }
                                    SessionChange::TokensRefreshed => {
                                        obj.imp().store_tokens().await;
                                    }
                                }
                            }
                        });
                    });
                }
            });

            let handle = spawn_tokio!(fut).abort_handle();
            self.session_changes_handle.replace(Some(handle));
        }

        /// Load the cached profile of the user of this session.
        async fn init_user_profile(&self) {
            let client = self.client().clone();
            let handle = spawn_tokio!(async move {
                client
                    .state_store()
                    .get_custom_value(SESSION_PROFILE_KEY.as_bytes())
                    .await
            });

            let profile = match handle.await.expect("task was not aborted") {
                Ok(Some(bytes)) => match serde_json::from_slice::<UserProfile>(&bytes) {
                    Ok(profile) => profile,
                    Err(error) => {
                        error!(
                            session = self.obj().session_id(),
                            "Could not deserialize session profile: {error}"
                        );
                        return;
                    }
                },
                Ok(None) => return,
                Err(error) => {
                    error!(
                        session = self.obj().session_id(),
                        "Could not load cached session profile: {error}"
                    );
                    return;
                }
            };

            let user = self.user();
            user.set_name(profile.displayname);
            user.set_avatar_url(profile.avatar_url);
        }

        /// Update the profile of this session’s user.
        ///
        /// Fetches the updated profile and updates the local data.
        async fn update_user_profile(&self) {
            let client = self.client().clone();
            let client_clone = client.clone();
            let handle =
                spawn_tokio!(async move { client_clone.account().fetch_user_profile().await });

            let profile = match handle
                .await
                .expect("task was not aborted")
                .and_then(|response| {
                    let mut profile = UserProfile::new();
                    profile.displayname = response.get_static::<DisplayName>()?;
                    profile.avatar_url = response.get_static::<AvatarUrl>()?;

                    Ok(profile)
                }) {
                Ok(profile) => profile,
                Err(error) => {
                    error!(
                        session = self.obj().session_id(),
                        "Could not fetch session profile: {error}"
                    );
                    return;
                }
            };

            let user = self.user();

            if Some(user.display_name()) == profile.displayname
                && user
                    .avatar_data()
                    .image()
                    .is_some_and(|i| i.uri() == profile.avatar_url)
            {
                // Nothing to update.
                return;
            }

            // Serialize first for caching to avoid a clone.
            let value = serde_json::to_vec(&profile);

            // Update the profile for the UI.
            user.set_name(profile.displayname);
            user.set_avatar_url(profile.avatar_url);

            // Update the cache.
            let value = match value {
                Ok(value) => value,
                Err(error) => {
                    error!(
                        session = self.obj().session_id(),
                        "Could not serialize session profile: {error}"
                    );
                    return;
                }
            };

            let handle = spawn_tokio!(async move {
                client
                    .state_store()
                    .set_custom_value(SESSION_PROFILE_KEY.as_bytes(), value)
                    .await
            });

            if let Err(error) = handle.await.expect("task was not aborted") {
                error!(
                    session = self.obj().session_id(),
                    "Could not cache session profile: {error}"
                );
            }
        }

        /// Start listening to notifications.
        pub(super) fn init_notifications(&self) {
            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let client = self.client().clone();

            spawn_tokio!(async move {
                client
                    .register_notification_handler(move |notification, room, _| {
                        let obj_weak = obj_weak.clone();
                        async move {
                            let ctx = glib::MainContext::default();
                            ctx.spawn(async move {
                                spawn!(async move {
                                    if let Some(obj) = obj_weak.upgrade() {
                                        obj.notifications().show_push(notification, room).await;
                                    }
                                });
                            });
                        }
                    })
                    .await;
            });
        }

        /// Update the stored session tokens.
        async fn store_tokens(&self) {
            let Some(session_tokens) = self.client().session_tokens() else {
                return;
            };

            debug!(
                session = self.obj().session_id(),
                "Storing updated session tokens…"
            );
            self.obj().info().store_tokens(session_tokens).await;
        }

        /// Clean up this session after it was logged out.
        ///
        /// This should only be called if the session has been logged out
        /// without calling `Session::log_out`.
        pub(super) async fn clean_up(&self) {
            let obj = self.obj();
            self.set_state(SessionState::LoggedOut);

            if let Some(handle) = self.sync_handle.take() {
                handle.abort();
            }

            self.stop_sliding_sync();

            if let Some(settings) = self.settings.get() {
                settings.delete();
            }

            obj.info().clone().delete().await;

            self.notifications.clear();

            debug!(
                session = obj.session_id(),
                "The logged out session was cleaned up"
            );
        }
    }
}

glib::wrapper! {
    /// A Matrix user session.
    pub struct Session(ObjectSubclass<imp::Session>)
        @extends SessionInfo;
}

impl Session {
    /// Construct an existing session.
    pub(crate) async fn new(
        stored_session: StoredSession,
        settings: SessionSettings,
    ) -> Result<Self, ClientSetupError> {
        let tokens = stored_session
            .load_tokens()
            .await
            .ok_or(ClientSetupError::NoSessionTokens)?;

        let stored_session_clone = stored_session.clone();
        let client = spawn_tokio!(async move {
            let client = matrix::client_with_stored_session(stored_session_clone, tokens).await?;

            // Make sure that we use the proper retention policy.
            let media = client.media();
            let used_media_retention_policy = media.media_retention_policy().await?;
            let wanted_media_retention_policy = MediaRetentionPolicy::default();

            if used_media_retention_policy != wanted_media_retention_policy {
                media
                    .set_media_retention_policy(wanted_media_retention_policy)
                    .await?;
            }

            Ok::<_, ClientSetupError>(client)
        })
        .await
        .expect("task was not aborted")?;

        let obj = glib::Object::builder::<Self>()
            .property("info", stored_session)
            .property("settings", settings)
            .build();
        obj.imp().set_client(client);

        Ok(obj)
    }

    /// Create a new session from the session of the given Matrix client.
    pub(crate) async fn create(client: &Client) -> Result<Self, ClientSetupError> {
        let stored_session = StoredSession::new(client).await?;
        let settings = Application::default()
            .session_list()
            .settings()
            .get_or_create(&stored_session.id);

        Self::new(stored_session, settings).await
    }

    /// Finish initialization of this session.
    pub(crate) async fn prepare(&self) {
        self.imp().prepare().await;
    }

    /// The room list of this session.
    pub(crate) fn room_list(&self) -> RoomList {
        self.imp().room_list()
    }

    /// The verification list of this session.
    pub(crate) fn verification_list(&self) -> VerificationList {
        self.imp().verification_list()
    }

    /// The Matrix client.
    pub(crate) fn client(&self) -> Client {
        self.imp().client().clone()
    }

    /// The cache for remote data.
    pub(crate) fn remote_cache(&self) -> &RemoteCache {
        self.imp().remote_cache()
    }

    /// Log out of this session.
    pub(crate) async fn log_out(&self) -> Result<(), String> {
        debug!(
            session = self.session_id(),
            "The session is about to be logged out"
        );

        let client = self.client();
        let handle = spawn_tokio!(async move { client.logout().await });

        match handle.await.expect("task was not aborted") {
            Ok(()) => {
                self.imp().clean_up().await;
                Ok(())
            }
            Err(error) => {
                error!(
                    session = self.session_id(),
                    "Could not log the session out: {error}"
                );
                Err(gettext("Could not log the session out"))
            }
        }
    }

    /// Clean up this session after it was logged out.
    ///
    /// This should only be called if the session has been logged out without
    /// calling `Session::log_out`.
    pub(crate) async fn clean_up(&self) {
        self.imp().clean_up().await;
    }

    /// Connect to the signal emitted when this session is logged out.
    pub(crate) fn connect_logged_out<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_state_notify(move |obj| {
            if obj.state() == SessionState::LoggedOut {
                f(obj);
            }
        })
    }

    /// Connect to the signal emitted when this session is ready.
    pub(crate) fn connect_ready<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_state_notify(move |obj| {
            if obj.state() == SessionState::Ready {
                f(obj);
            }
        })
    }
}
