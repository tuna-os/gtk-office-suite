//! Session lifecycle: preparation, homeserver reachability, token storage and
//! clean-up after logout.

use futures_util::StreamExt;
use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};
use matrix_sdk::SessionChange;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{debug, error, info};

use super::{SessionState, imp::Session};
use crate::{session_list::SessionInfoExt, spawn, spawn_tokio};

impl Session {
    /// The homeserver URL as a `GNetworkAddress`.
    pub(super) fn homeserver_address(&self) -> gio::NetworkAddress {
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
    pub(super) fn set_is_homeserver_reachable(&self, is_reachable: bool) {
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
    pub(super) fn watch_session_changes(&self) {
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
    /// Update the stored session tokens.
    pub(super) async fn store_tokens(&self) {
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
