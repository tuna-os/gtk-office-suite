//! Synchronization orchestration for a Matrix session.

use std::{sync::Arc, time::Duration};

use futures_util::{StreamExt, pin_mut};
use gtk::{
    glib::{self, clone},
    subclass::prelude::*,
};
use matrix_sdk::{config::SyncSettings, sync::SyncResponse};
use matrix_sdk_ui::{
    eyeball_im::VectorDiff,
    room_list_service::{RoomListLoadingState, filters::new_filter_all},
    sync_service::{State as SyncServiceState, SyncService},
};
use ruma::{
    OwnedRoomId,
    api::{
        FeatureFlag,
        client::filter::{FilterDefinition, RoomFilter},
    },
    assign,
};
use tokio::time::sleep;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{debug, error, info};

use super::{
    MISSED_SYNC_DELAYS, MISSED_SYNC_OFFLINE_COUNT, SessionState, SyncMode, SyncServiceWrapper,
    imp::Session, is_sliding_sync_unsupported_error,
};
use crate::{session_list::SessionInfoExt, spawn, spawn_tokio, utils::TokioDrop};

impl Session {
    /// Start syncing the Matrix client.
    ///
    /// The first call detects whether the homeserver supports simplified
    /// sliding sync (MSC4186) and then selects the sync method for the
    /// rest of the session's lifetime, unless sliding sync fails with an
    /// unsupported error, in which case we fall back to classic sync.
    pub(super) fn sync(&self) {
        if self.state.get() < SessionState::InitialSync || !self.is_homeserver_reachable.get() {
            return;
        }

        match self.sync_mode.get() {
            SyncMode::Unknown => self.detect_sync_mode(),
            SyncMode::SlidingSync => self.sliding_sync(),
            SyncMode::Classic => self.classic_sync(),
        }
    }

    /// Detect whether the homeserver supports simplified sliding sync and
    /// start the proper sync method.
    fn detect_sync_mode(&self) {
        let client = self.client().clone();

        spawn!(clone!(
            #[weak(rename_to = imp)]
            self,
            async move {
                let handle = spawn_tokio!(async move { client.supported_versions().await });

                match handle.await.expect("task was not aborted") {
                    Ok(supported) => {
                        let mode = if supported.features.contains(&FeatureFlag::Msc4186) {
                            info!(
                                session = imp.obj().session_id(),
                                "Homeserver supports simplified sliding sync (MSC4186), \
                                 using sliding sync"
                            );
                            SyncMode::SlidingSync
                        } else {
                            info!(
                                session = imp.obj().session_id(),
                                "Homeserver does not support simplified sliding sync \
                                 (MSC4186), using classic sync"
                            );
                            SyncMode::Classic
                        };

                        imp.sync_mode.set(mode);
                        imp.sync();
                    }
                    Err(error) => {
                        error!(
                            session = imp.obj().session_id(),
                            "Could not detect simplified sliding sync support: {error}"
                        );
                        imp.handle_missed_sync();
                    }
                }
            }
        ));
    }

    /// Handle a failed synchronization attempt.
    ///
    /// Updates the offline state and schedules a new synchronization
    /// attempt after a delay.
    fn handle_missed_sync(&self) {
        let missed_sync_count = self.missed_sync_count.get();

        // If there are too many failed attempts, mark the session as offline.
        if missed_sync_count == MISSED_SYNC_OFFLINE_COUNT {
            self.set_offline(true);
        }

        // Increase the count of missed syncs, if we have not reached the maximum value.
        if missed_sync_count < 4 {
            self.missed_sync_count.set(missed_sync_count + 1);
        }

        // Wait a little before trying again.
        let delay = MISSED_SYNC_DELAYS[missed_sync_count];
        glib::timeout_add_seconds_local_once(
            delay as u32,
            clone!(
                #[weak(rename_to = imp)]
                self,
                move || {
                    imp.sync();
                }
            ),
        );
    }

    /// Start or restart the sync service using simplified sliding sync.
    fn sliding_sync(&self) {
        if let Some(sync_service) = &*self.sync_service.borrow() {
            // The service was already created, make sure it is running.
            let sync_service = sync_service.0.clone();
            spawn_tokio!(async move {
                sync_service.start().await;
            });
            return;
        }

        let client = self.client().clone();

        spawn!(clone!(
            #[weak(rename_to = imp)]
            self,
            async move {
                let handle =
                    spawn_tokio!(async move { SyncService::builder(client).build().await });

                match handle.await.expect("task was not aborted") {
                    Ok(sync_service) => {
                        let sync_service = Arc::new(sync_service);
                        imp.sync_service
                            .replace(Some(TokioDrop::new(SyncServiceWrapper(
                                sync_service.clone(),
                            ))));

                        imp.watch_sync_service_state(&sync_service);
                        imp.watch_room_list_entries(&sync_service);
                        imp.watch_room_updates();

                        spawn_tokio!(async move {
                            sync_service.start().await;
                        });
                    }
                    Err(error) => {
                        error!(
                            session = imp.obj().session_id(),
                            "Could not create sync service, falling back to classic sync: \
                             {error}"
                        );
                        imp.sync_mode.set(SyncMode::Classic);
                        imp.sync();
                    }
                }
            }
        ));
    }

    /// Watch the state of the sync service.
    fn watch_sync_service_state(&self, sync_service: &SyncService) {
        let state_stream = sync_service.state();
        let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());

        let fut = state_stream.for_each(move |state| {
            let obj_weak = obj_weak.clone();
            async move {
                let ctx = glib::MainContext::default();
                ctx.spawn(async move {
                    spawn!(async move {
                        if let Some(obj) = obj_weak.upgrade() {
                            obj.imp().handle_sync_service_state(&state);
                        }
                    });
                });
            }
        });

        let handle = spawn_tokio!(fut).abort_handle();
        self.sync_service_state_handle.replace(Some(handle));
    }

    /// Handle a state change of the sync service.
    fn handle_sync_service_state(&self, state: &SyncServiceState) {
        let obj = self.obj();
        let session_id = obj.session_id();

        match state {
            SyncServiceState::Running => {
                debug!(session = session_id, "Sync service is running");
                self.set_offline(false);
                self.missed_sync_count.set(0);
            }
            SyncServiceState::Error(error) => {
                error!(session = session_id, "Sync service error: {error}");

                if is_sliding_sync_unsupported_error(error) {
                    info!(
                        session = session_id,
                        "Homeserver rejected simplified sliding sync, falling back to \
                         classic sync"
                    );
                    self.stop_sliding_sync();
                    self.sync_mode.set(SyncMode::Classic);
                    self.sync();
                    return;
                }

                self.handle_missed_sync();
            }
            SyncServiceState::Idle | SyncServiceState::Terminated | SyncServiceState::Offline => {
                // Graceful states, nothing to do.
            }
        }
    }

    /// Watch the entries of the sync service's room list.
    ///
    /// This is the source of the room membership of the session's
    /// `RoomList` when sliding sync is used. It also watches the loading
    /// state of the SDK's room list to detect the end of the first
    /// synchronization.
    fn watch_room_list_entries(&self, sync_service: &SyncService) {
        let room_list_service = sync_service.room_list_service();
        let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());

        let fut = async move {
            let all_rooms = match room_list_service.all_rooms().await {
                Ok(all_rooms) => all_rooms,
                Err(error) => {
                    error!("Could not get the sync service's room list: {error}");
                    return;
                }
            };

            // Watch the loading state to detect the end of the first sync.
            let mut loading_state_stream = all_rooms.loading_state();
            let obj_weak_clone = obj_weak.clone();
            let loading_state_fut = async move {
                while let Some(state) = loading_state_stream.next().await {
                    if matches!(state, RoomListLoadingState::Loaded { .. }) {
                        let ctx = glib::MainContext::default();
                        ctx.spawn(async move {
                            spawn!(async move {
                                if let Some(obj) = obj_weak_clone.upgrade() {
                                    obj.imp().handle_first_sync_done();
                                }
                            });
                        });
                        break;
                    }
                }
            };

            let entries_fut = async {
                let (entries_stream, entries_controller) =
                    all_rooms.entries_with_dynamic_adapters(usize::MAX);
                // We want all the rooms, the filtering and sorting is done
                // by the models on top of the session's `RoomList`.
                entries_controller.set_filter(Box::new(new_filter_all(Vec::new())));

                pin_mut!(entries_stream);
                while let Some(diff_list) = entries_stream.next().await {
                    let diff_list = diff_list
                        .into_iter()
                        .map(|diff| diff.map(|item| item.into_inner().room_id().to_owned()))
                        .collect::<Vec<VectorDiff<OwnedRoomId>>>();

                    let obj_weak = obj_weak.clone();
                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            if let Some(obj) = obj_weak.upgrade() {
                                obj.imp().room_list().handle_sliding_sync_entries(diff_list);
                            }
                        });
                    });
                }
            };

            futures_util::future::join(loading_state_fut, entries_fut).await;
        };

        let handle = spawn_tokio!(fut).abort_handle();
        self.room_list_entries_handle.replace(Some(handle));
    }

    /// Watch the room updates received via sync.
    ///
    /// With sliding sync, this is only used to forward the ambiguity
    /// changes of the members to the rooms, the room membership is handled
    /// via the sliding sync entries.
    fn watch_room_updates(&self) {
        let receiver = self.client().subscribe_to_all_room_updates();
        let stream = BroadcastStream::new(receiver);
        let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());

        let fut = stream.for_each(move |updates| {
            let obj_weak = obj_weak.clone();
            async move {
                let Ok(updates) = updates else {
                    return;
                };

                let ctx = glib::MainContext::default();
                ctx.spawn(async move {
                    spawn!(async move {
                        if let Some(obj) = obj_weak.upgrade() {
                            obj.imp().room_list().handle_ambiguity_changes(&updates);
                        }
                    });
                });
            }
        });

        let handle = spawn_tokio!(fut).abort_handle();
        self.room_updates_handle.replace(Some(handle));
    }

    /// Handle the end of the first synchronization with sliding sync.
    fn handle_first_sync_done(&self) {
        debug!(
            session = self.obj().session_id(),
            "First sliding sync completed"
        );

        if self.state.get() < SessionState::Ready {
            self.set_state(SessionState::Ready);
            self.init_notifications();
        }

        self.set_offline(false);
        self.missed_sync_count.set(0);
    }

    /// Stop the sync service and the tasks watching it, if any.
    pub(super) fn stop_sliding_sync(&self) {
        for handle in [
            self.sync_service_state_handle.take(),
            self.room_list_entries_handle.take(),
            self.room_updates_handle.take(),
        ]
        .into_iter()
        .flatten()
        {
            handle.abort();
        }

        if let Some(sync_service) = self.sync_service.take() {
            let sync_service = sync_service.0.clone();
            spawn_tokio!(async move {
                sync_service.stop().await;
            });
        }
    }

    /// Start syncing the Matrix client with the classic `/sync` endpoint.
    fn classic_sync(&self) {
        let client = self.client().clone();
        let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());

        let handle = spawn_tokio!(async move {
            // Make sure that the event cache is subscribed to sync responses to benefit
            // from it.
            if let Err(error) = client.event_cache().subscribe() {
                error!("Could not subscribe event cache to sync responses: {error}");
            }

            // TODO: only create the filter once and reuse it in the future
            let filter = assign!(FilterDefinition::default(), {
                room: assign!(RoomFilter::with_lazy_loading(), {
                    include_leave: true,
                }),
            });

            let sync_settings = SyncSettings::new()
                .timeout(Duration::from_secs(30))
                .ignore_timeout_on_first_sync(true)
                .filter(filter.into());

            let mut sync_stream = Box::pin(client.sync_stream(sync_settings).await);
            while let Some(response) = sync_stream.next().await {
                let obj_weak = obj_weak.clone();
                let ctx = glib::MainContext::default();
                let delay = ctx
                    .spawn(async move {
                        spawn!(async move {
                            if let Some(obj) = obj_weak.upgrade() {
                                obj.imp().handle_sync_response(response)
                            } else {
                                None
                            }
                        })
                        .await
                        .expect("task was not aborted")
                    })
                    .await
                    .expect("task was not aborted");

                if let Some(delay) = delay {
                    sleep(delay).await;
                }
            }
        })
        .abort_handle();

        self.sync_handle.replace(Some(handle));
    }

    /// Handle the response received via sync.
    ///
    /// Returns the delay to wait for before making the next sync, if
    /// necessary.
    fn handle_sync_response(
        &self,
        response: Result<SyncResponse, matrix_sdk::Error>,
    ) -> Option<Duration> {
        let obj = self.obj();
        let session_id = obj.session_id();
        debug!(session = session_id, "Received sync response");

        match response {
            Ok(response) => {
                self.room_list().handle_room_updates(response.rooms);

                if self.state.get() < SessionState::Ready {
                    self.set_state(SessionState::Ready);
                    self.init_notifications();
                }

                self.set_offline(false);
                self.missed_sync_count.set(0);

                None
            }
            Err(error) => {
                let missed_sync_count = self.missed_sync_count.get();

                // If there are too many failed attempts, mark the session as offline.
                if missed_sync_count == MISSED_SYNC_OFFLINE_COUNT {
                    self.set_offline(true);
                }

                // Increase the count of missed syncs, if we have not reached the maximum value.
                if missed_sync_count < 4 {
                    self.missed_sync_count.set(missed_sync_count + 1);
                }

                error!(session = session_id, "Could not perform sync: {error}");

                // Sleep a little between attempts.
                let delay = MISSED_SYNC_DELAYS[missed_sync_count];
                Some(Duration::from_secs(delay))
            }
        }
    }
}
