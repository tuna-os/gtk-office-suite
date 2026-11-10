use futures_util::StreamExt;
use gtk::{glib, glib::clone, prelude::*, subclass::prelude::*};
use matrix_sdk::encryption::{
    VerificationState as SdkVerificationState, recovery::RecoveryState as SdkRecoveryState,
};
use tokio::task::AbortHandle;
use tracing::{debug, error, warn};

use super::Session;
use crate::{prelude::*, spawn, spawn_tokio};

/// The state of the crypto identity.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, glib::Enum)]
#[enum_type(name = "CryptoIdentityState")]
pub enum CryptoIdentityState {
    /// The state is not known yet.
    #[default]
    Unknown,
    /// The crypto identity does not exist.
    ///
    /// It means that cross-signing is not set up.
    Missing,
    /// There are no other verified sessions.
    LastManStanding,
    /// There are other verified sessions.
    OtherSessions,
}

/// The state of the verification of the session.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, glib::Enum)]
#[enum_type(name = "SessionVerificationState")]
pub enum SessionVerificationState {
    /// The state is not known yet.
    #[default]
    Unknown,
    /// The session is verified.
    Verified,
    /// The session is not verified.
    Unverified,
}

impl From<SdkVerificationState> for SessionVerificationState {
    fn from(value: SdkVerificationState) -> Self {
        match value {
            SdkVerificationState::Unknown => Self::Unknown,
            SdkVerificationState::Verified => Self::Verified,
            SdkVerificationState::Unverified => Self::Unverified,
        }
    }
}

/// The state of the recovery.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, glib::Enum)]
#[enum_type(name = "RecoveryState")]
pub enum RecoveryState {
    /// The state is not known yet.
    #[default]
    Unknown,
    /// Recovery is disabled.
    Disabled,
    /// Recovery is enabled and we have all the keys.
    Enabled,
    /// Recovery is enabled and we are missing some keys.
    Incomplete,
}

impl From<SdkRecoveryState> for RecoveryState {
    fn from(value: SdkRecoveryState) -> Self {
        match value {
            SdkRecoveryState::Unknown => Self::Unknown,
            SdkRecoveryState::Disabled => Self::Disabled,
            SdkRecoveryState::Enabled => Self::Enabled,
            SdkRecoveryState::Incomplete => Self::Incomplete,
        }
    }
}

mod imp {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::SessionSecurity)]
    pub struct SessionSecurity {
        /// The current session.
        #[property(get, set = Self::set_session, explicit_notify, nullable)]
        session: glib::WeakRef<Session>,
        /// The state of the crypto identity for the current session.
        #[property(get, builder(CryptoIdentityState::default()))]
        crypto_identity_state: Cell<CryptoIdentityState>,
        /// The state of the verification for the current session.
        #[property(get, builder(SessionVerificationState::default()))]
        verification_state: Cell<SessionVerificationState>,
        /// The state of recovery for the current session.
        #[property(get, builder(RecoveryState::default()))]
        recovery_state: Cell<RecoveryState>,
        /// Whether all the cross-signing keys are available.
        #[property(get)]
        cross_signing_keys_available: Cell<bool>,
        /// Whether the room keys backup is enabled.
        #[property(get)]
        backup_enabled: Cell<bool>,
        /// Whether the room keys backup exists on the homeserver.
        #[property(get)]
        backup_exists_on_server: Cell<bool>,
        abort_handles: RefCell<Vec<AbortHandle>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SessionSecurity {
        const NAME: &'static str = "SessionSecurity";
        type Type = super::SessionSecurity;
    }

    #[glib::derived_properties]
    impl ObjectImpl for SessionSecurity {
        fn dispose(&self) {
            for handle in self.abort_handles.take() {
                handle.abort();
            }
        }
    }

    impl SessionSecurity {
        /// Set the current session.
        fn set_session(&self, session: Option<&Session>) {
            if self.session.upgrade().as_ref() == session {
                return;
            }

            self.session.set(session);
            self.obj().notify_session();

            self.watch_verification_state();
            self.watch_recovery_state();

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.watch_crypto_identity_state().await;
                }
            ));
        }

        /// Set the crypto identity state of the current session.
        pub(super) fn set_crypto_identity_state(&self, state: CryptoIdentityState) {
            if self.crypto_identity_state.get() == state {
                return;
            }

            self.crypto_identity_state.set(state);
            self.obj().notify_crypto_identity_state();
        }

        /// Set the verification state of the current session.
        pub(super) fn set_verification_state(&self, state: SessionVerificationState) {
            if self.verification_state.get() == state {
                return;
            }

            self.verification_state.set(state);
            self.obj().notify_verification_state();
        }

        /// Set the recovery state of the current session.
        pub(super) fn set_recovery_state(&self, state: RecoveryState) {
            if self.recovery_state.get() == state {
                return;
            }

            self.recovery_state.set(state);
            self.obj().notify_recovery_state();
        }

        /// Set whether all the cross-signing keys are available.
        pub(super) fn set_cross_signing_keys_available(&self, available: bool) {
            if self.cross_signing_keys_available.get() == available {
                return;
            }

            self.cross_signing_keys_available.set(available);
            self.obj().notify_cross_signing_keys_available();
        }

        /// Set whether the room keys backup is enabled.
        pub(super) fn set_backup_enabled(&self, enabled: bool) {
            if self.backup_enabled.get() == enabled {
                return;
            }

            self.backup_enabled.set(enabled);
            self.obj().notify_backup_enabled();
        }

        /// Set whether the room keys backup exists on the homeserver.
        pub(super) fn set_backup_exists_on_server(&self, exists: bool) {
            if self.backup_exists_on_server.get() == exists {
                return;
            }

            self.backup_exists_on_server.set(exists);
            self.obj().notify_backup_exists_on_server();
        }

        /// Listen to crypto identity changes.
        async fn watch_crypto_identity_state(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let client = session.client();
            let encryption = client.encryption();

            let encryption_clone = encryption.clone();
            let handle =
                spawn_tokio!(async move { encryption_clone.user_identities_stream().await });
            let identities_stream = match handle.await.unwrap() {
                Ok(stream) => stream,
                Err(error) => {
                    error!("Could not get user identities stream: {error}");
                    // All method calls here have the same error, so we can return early.
                    return;
                }
            };

            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let fut = identities_stream.for_each(move |updates| {
                let obj_weak = obj_weak.clone();

                async move {
                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            let Some(obj) = obj_weak.upgrade() else {
                                return;
                            };
                            let Some(session) = obj.session() else {
                                return;
                            };

                            let own_user_id = session.user_id();
                            if updates.new.contains_key(own_user_id)
                                || updates.changed.contains_key(own_user_id)
                            {
                                obj.imp().load_crypto_identity_state().await;
                            }
                        });
                    });
                }
            });
            let identities_abort_handle = spawn_tokio!(fut).abort_handle();

            let handle = spawn_tokio!(async move { encryption.devices_stream().await });
            let devices_stream = match handle.await.unwrap() {
                Ok(stream) => stream,
                Err(error) => {
                    error!("Could not get devices stream: {error}");
                    // All method calls here have the same error, so we can return early.
                    return;
                }
            };

            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let fut = devices_stream.for_each(move |updates| {
                let obj_weak = obj_weak.clone();

                async move {
                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            let Some(obj) = obj_weak.upgrade() else {
                                return;
                            };
                            let Some(session) = obj.session() else {
                                return;
                            };

                            let own_user_id = session.user_id();
                            if updates.new.contains_key(own_user_id)
                                || updates.changed.contains_key(own_user_id)
                            {
                                obj.imp().load_crypto_identity_state().await;
                            }
                        });
                    });
                }
            });
            let devices_abort_handle = spawn_tokio!(fut).abort_handle();

            self.abort_handles
                .borrow_mut()
                .extend([identities_abort_handle, devices_abort_handle]);

            self.load_crypto_identity_state().await;
        }

        /// Load the crypto identity state.
        async fn load_crypto_identity_state(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let client = session.client();

            let client_clone = client.clone();
            let user_identity_handle = spawn_tokio!(async move {
                let user_id = client_clone.user_id().unwrap();
                client_clone.encryption().get_user_identity(user_id).await
            });

            let has_identity = match user_identity_handle.await.unwrap() {
                Ok(Some(_)) => true,
                Ok(None) => {
                    debug!("No crypto user identity found");
                    false
                }
                Err(error) => {
                    error!("Could not get crypto user identity: {error}");
                    false
                }
            };

            if !has_identity {
                self.set_crypto_identity_state(CryptoIdentityState::Missing);
                return;
            }

            let devices_handle = spawn_tokio!(async move {
                let user_id = client.user_id().unwrap();
                client.encryption().get_user_devices(user_id).await
            });

            let own_device = session.device_id();
            let has_other_sessions = match devices_handle.await.unwrap() {
                Ok(devices) => devices
                    .devices()
                    .any(|d| d.device_id() != own_device && d.is_cross_signed_by_owner()),
                Err(error) => {
                    error!("Could not get user devices: {error}");
                    // If there are actually no other devices, the user can still
                    // reset the crypto identity.
                    true
                }
            };

            let state = if has_other_sessions {
                CryptoIdentityState::OtherSessions
            } else {
                CryptoIdentityState::LastManStanding
            };

            self.set_crypto_identity_state(state);
        }

        /// Listen to verification state changes.
        fn watch_verification_state(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let client = session.client();
            let mut stream = client.encryption().verification_state();
            // Get the current value right away.
            stream.reset();

            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let fut = stream.for_each(move |state| {
                let obj_weak = obj_weak.clone();

                async move {
                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            if let Some(obj) = obj_weak.upgrade() {
                                obj.imp().set_verification_state(state.into());
                            }
                        });
                    });
                }
            });
            let verification_abort_handle = spawn_tokio!(fut).abort_handle();

            self.abort_handles
                .borrow_mut()
                .push(verification_abort_handle);
        }

        /// Listen to recovery state changes.
        fn watch_recovery_state(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let client = session.client();

            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let stream = client.encryption().recovery().state_stream();

            let fut = stream.for_each(move |state| {
                let obj_weak = obj_weak.clone();

                async move {
                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            if let Some(obj) = obj_weak.upgrade() {
                                obj.imp().update_recovery_state(state.into()).await;
                            }
                        });
                    });
                }
            });

            let abort_handle = spawn_tokio!(fut).abort_handle();
            self.abort_handles.borrow_mut().push(abort_handle);
        }

        /// Update the session for the given recovery state.
        async fn update_recovery_state(&self, state: RecoveryState) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let (cross_signing_keys_available, backup_enabled, backup_exists_on_server) = if matches!(
                state,
                RecoveryState::Enabled
            ) {
                (true, true, true)
            } else {
                let encryption = session.client().encryption();
                let backups = encryption.backups();

                let handle = spawn_tokio!(async move { encryption.cross_signing_status().await });
                let cross_signing_keys_available =
                    handle.await.unwrap().is_some_and(|s| s.is_complete());

                let handle = spawn_tokio!(async move {
                    if backups.are_enabled().await {
                        (true, true)
                    } else {
                        let backup_exists_on_server = match backups.exists_on_server().await {
                            Ok(exists) => exists,
                            Err(error) => {
                                warn!(
                                    "Could not request whether recovery backup exists on homeserver: {error}"
                                );
                                // If the request failed, we have to try to delete the backup to
                                // avoid unsolvable errors.
                                true
                            }
                        };
                        (false, backup_exists_on_server)
                    }
                });
                let (backup_enabled, backup_exists_on_server) = handle.await.unwrap();

                (
                    cross_signing_keys_available,
                    backup_enabled,
                    backup_exists_on_server,
                )
            };

            self.set_cross_signing_keys_available(cross_signing_keys_available);
            self.set_backup_enabled(backup_enabled);
            self.set_backup_exists_on_server(backup_exists_on_server);

            self.set_recovery_state(state);
        }
    }
}

glib::wrapper! {
    /// Information about the security of a Matrix session.
    pub struct SessionSecurity(ObjectSubclass<imp::SessionSecurity>);
}

impl SessionSecurity {
    /// Construct a new empty `SessionSecurity`.
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for SessionSecurity {
    fn default() -> Self {
        Self::new()
    }
}
