use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};
use matrix_sdk::{
    Client as MatrixClient, Room as MatrixRoom, encryption::verification::VerificationRequest,
};
use ruma::{
    RoomId,
    events::{
        key::verification::request::ToDeviceKeyVerificationRequestEvent,
        room::message::{MessageType, OriginalSyncRoomMessageEvent},
    },
};
use tracing::{debug, error};

use super::{VerificationKey, VerificationState, load_supported_verification_methods};
use crate::{
    session::{IdentityVerification, Member, Membership, Session, User},
    spawn, spawn_tokio,
};

mod imp {
    use std::{cell::RefCell, sync::LazyLock};

    use glib::subclass::Signal;
    use indexmap::IndexMap;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::VerificationList)]
    pub struct VerificationList {
        /// The ongoing verification requests.
        pub(super) list: RefCell<IndexMap<VerificationKey, IdentityVerification>>,
        /// The current session.
        #[property(get, construct_only)]
        session: glib::WeakRef<Session>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VerificationList {
        const NAME: &'static str = "VerificationList";
        type Type = super::VerificationList;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for VerificationList {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("secret-received").build()]);
            SIGNALS.as_ref()
        }
    }

    impl ListModelImpl for VerificationList {
        fn item_type(&self) -> glib::Type {
            IdentityVerification::static_type()
        }

        fn n_items(&self) -> u32 {
            self.list.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.list
                .borrow()
                .get_index(position as usize)
                .map(|(_, item)| item.clone().upcast())
        }
    }

    impl VerificationList {
        /// Add a verification received via a to-device event.
        pub(super) async fn add_to_device_request(&self, request: VerificationRequest) {
            if request.is_done() || request.is_cancelled() || request.is_passive() {
                // Ignore requests that are already finished.
                return;
            }

            let Some(session) = self.session.upgrade() else {
                return;
            };

            let verification = IdentityVerification::new(request, &session.user(), None).await;
            self.add(verification.clone());

            if verification.state() == VerificationState::Requested {
                session
                    .notifications()
                    .show_to_device_identity_verification(&verification)
                    .await;
            }
        }

        /// Add a verification received via an in-room event.
        pub(super) async fn add_in_room_request(
            &self,
            request: VerificationRequest,
            room_id: &RoomId,
        ) {
            if request.is_done() || request.is_cancelled() || request.is_passive() {
                // Ignore requests that are already finished.
                return;
            }

            let Some(session) = self.session.upgrade() else {
                return;
            };
            let Some(room) = session.room_list().get(room_id) else {
                error!(
                    "Room for verification request `({}, {})` not found",
                    request.other_user_id(),
                    request.flow_id()
                );
                return;
            };

            if matches!(
                room.own_member().membership(),
                Membership::Leave | Membership::Ban
            ) {
                // Ignore requests where the user is not in the room anymore.
                return;
            }

            let other_user_id = request.other_user_id().to_owned();
            let member = room.members().map_or_else(
                || Member::new(&room, other_user_id.clone()),
                |l| l.get_or_create(other_user_id.clone()),
            );

            // Ensure the member is up-to-date.
            let matrix_room = room.matrix_room().clone();
            let handle =
                spawn_tokio!(async move { matrix_room.get_member_no_sync(&other_user_id).await });
            match handle.await.expect("task was not aborted") {
                Ok(Some(matrix_member)) => member.update_from_room_member(&matrix_member),
                Ok(None) => {
                    error!(
                        "Room member for verification request `({}, {})` not found",
                        request.other_user_id(),
                        request.flow_id()
                    );
                    return;
                }
                Err(error) => {
                    error!(
                        "Could not get room member for verification request `({}, {})`: {error}",
                        request.other_user_id(),
                        request.flow_id()
                    );
                    return;
                }
            }

            let verification =
                IdentityVerification::new(request, member.upcast_ref(), Some(&room)).await;

            room.set_verification(Some(&verification));

            self.add(verification.clone());

            if verification.state() == VerificationState::Requested {
                session
                    .notifications()
                    .show_in_room_identity_verification(&verification)
                    .await;
            }
        }

        /// Add the given verification to the list.
        pub(super) fn add(&self, verification: IdentityVerification) {
            let key = verification.key();

            // Don't add request that already exists.
            if self.list.borrow().contains_key(&key) {
                return;
            }

            let obj = self.obj();
            verification.connect_remove_from_list(clone!(
                #[weak]
                obj,
                move |verification| {
                    obj.remove(&verification.key());
                }
            ));

            let (pos, _) = self.list.borrow_mut().insert_full(key, verification);

            obj.items_changed(pos as u32, 0, 1);
        }
    }
}

glib::wrapper! {
    /// The list of ongoing verification requests.
    pub struct VerificationList(ObjectSubclass<imp::VerificationList>)
        @implements gio::ListModel;
}

impl VerificationList {
    /// Construct a new `VerificationList` with the given session.
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }

    /// Initialize this list to listen to new verification requests.
    pub(crate) fn init(&self) {
        let Some(session) = self.session() else {
            return;
        };

        let client = session.client();
        let obj_weak = glib::SendWeakRef::from(self.downgrade());

        let obj_weak_clone = obj_weak.clone();
        client.add_event_handler(
            move |ev: ToDeviceKeyVerificationRequestEvent, client: MatrixClient| {
                let obj_weak = obj_weak_clone.clone();
                async move {
                    let Some(request) = client
                        .encryption()
                        .get_verification_request(&ev.sender, &ev.content.transaction_id)
                        .await
                    else {
                        // This might be normal if the request has already timed out.
                        debug!(
                            "To-device verification request `({}, {})` not found in the SDK",
                            ev.sender, ev.content.transaction_id
                        );
                        return;
                    };

                    if !request.is_self_verification() {
                        // We only support in-room verifications for other users.
                        debug!(
                            "To-device verification request `({}, {})` for other users is not supported",
                            ev.sender, ev.content.transaction_id
                        );
                        return;
                    }

                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            if let Some(obj) = obj_weak.upgrade() {
                                obj.imp().add_to_device_request(request).await;
                            }
                        });
                    });
                }
            },
        );

        client.add_event_handler(
            move |ev: OriginalSyncRoomMessageEvent, room: MatrixRoom, client: MatrixClient| {
                let obj_weak = obj_weak.clone();
                async move {
                    let MessageType::VerificationRequest(_) = &ev.content.msgtype else {
                        return;
                    };
                    let Some(request) = client
                        .encryption()
                        .get_verification_request(&ev.sender, &ev.event_id)
                        .await
                    else {
                        // This might be normal if the request has already timed out.
                        debug!(
                            "To-device verification request `({}, {})` not found in the SDK",
                            ev.sender, ev.event_id
                        );
                        return;
                    };
                    let room_id = room.room_id().to_owned();

                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            if let Some(obj) = obj_weak.upgrade() {
                                obj.imp().add_in_room_request(request, &room_id).await;
                            }
                        });
                    });
                }
            },
        );
    }

    /// Remove the verification with the given key.
    pub(crate) fn remove(&self, key: &VerificationKey) {
        let Some((pos, ..)) = self.imp().list.borrow_mut().shift_remove_full(key) else {
            return;
        };

        self.items_changed(pos as u32, 1, 0);

        if let Some(session) = self.session() {
            session.notifications().withdraw_identity_verification(key);
        }
    }

    /// Get the verification with the given key.
    pub(crate) fn get(&self, key: &VerificationKey) -> Option<IdentityVerification> {
        self.imp().list.borrow().get(key).cloned()
    }

    // Returns the ongoing session verification, if any.
    pub(crate) fn ongoing_session_verification(&self) -> Option<IdentityVerification> {
        let list = self.imp().list.borrow();
        list.values()
            .find(|v| v.is_self_verification() && !v.is_finished())
            .cloned()
    }

    // Returns the ongoing verification in the given room, if any.
    pub(crate) fn ongoing_room_verification(
        &self,
        room_id: &RoomId,
    ) -> Option<IdentityVerification> {
        let list = self.imp().list.borrow();
        list.values()
            .find(|v| v.room().is_some_and(|room| room.room_id() == room_id) && !v.is_finished())
            .cloned()
    }

    /// Create and send a new verification request.
    ///
    /// If `user` is `None`, a new session verification is started for our own
    /// user and sent to other devices.
    pub(crate) async fn create(&self, user: Option<User>) -> Result<IdentityVerification, ()> {
        let Some(session) = self.session() else {
            error!("Could not create identity verification: failed to upgrade session");
            return Err(());
        };

        let user = user.unwrap_or_else(|| session.user());

        let supported_methods = load_supported_verification_methods().await;

        let Some(identity) = user.ensure_crypto_identity().await else {
            error!("Could not create identity verification: cryptographic identity not found");
            return Err(());
        };

        let handle = spawn_tokio!(async move {
            identity
                .request_verification_with_methods(supported_methods)
                .await
        });

        match handle.await.expect("task was not aborted") {
            Ok(request) => {
                let room = if let Some(room_id) = request.room_id() {
                    let Some(room) = session.room_list().get(room_id) else {
                        error!(
                            "Room for verification request `({}, {})` not found",
                            request.other_user_id(),
                            request.flow_id()
                        );
                        return Err(());
                    };
                    Some(room)
                } else {
                    None
                };

                let verification = IdentityVerification::new(request, &user, room.as_ref()).await;
                self.imp().add(verification.clone());

                Ok(verification)
            }
            Err(error) => {
                error!("Could not create identity verification: {error}");
                Err(())
            }
        }
    }
}
