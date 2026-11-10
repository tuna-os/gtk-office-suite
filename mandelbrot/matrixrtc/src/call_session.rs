// SPDX-License-Identifier: GPL-3.0-or-later

//! The callable MatrixRTC engine: ties the membership manager, the
//! encryption manager and the to-device key transport together into one
//! session object, mirroring `matrix-js-sdk`'s
//! `MatrixRTCSession.joinRTCSession` semantics.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use ruma::{OwnedDeviceId, OwnedUserId};
use serde_json::{Value as JsonValue, json};
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::{
    call_membership::{CallMembership, MemberStateEvent},
    client::{RtcClientApi, ToDeviceEvent},
    encryption_manager::{EncryptionConfig, KeyRingEntry, RtcEncryptionManager},
    key_transport::{
        CallMembershipIdentity, KeyTransport, MalformedKeyEvent, Statistics, ToDeviceKeyTransport,
    },
    membership_data::{FocusActive, Transport},
    membership_manager::{
        MembershipConfig, MembershipManager, MembershipManagerEvent, RtcRoom, Status,
    },
    session::{MatrixRtcSession, SlotDescription},
};

/// The event type of MSC4075 call notifications.
pub const RTC_NOTIFICATION_EVENT_TYPE: &str = "org.matrix.msc4075.rtc.notification";

/// Configuration for an [`RtcCallSession`].
#[derive(Clone, Debug, Default)]
pub struct RtcCallSessionConfig {
    /// The membership manager configuration.
    pub membership: MembershipConfig,
    /// The encryption manager configuration.
    pub encryption: EncryptionConfig,
    /// What kind of notification to send when starting the session, e.g.
    /// `ring` or `notification`. `None` sends no notification.
    pub notification_type: Option<String>,
}

/// Events emitted by an [`RtcCallSession`].
#[derive(Clone, Debug)]
pub enum RtcCallSessionEvent {
    /// A member joined, left, or updated a property of their membership.
    MembershipsChanged {
        /// The memberships before the update.
        old: Vec<CallMembership>,
        /// The memberships after the update.
        new: Vec<CallMembership>,
    },
    /// We joined or left the session (our own local idea of whether we are
    /// joined, independent of whether our member event has gone through).
    JoinStateChanged(bool),
    /// A key used to encrypt media was added or changed.
    EncryptionKeyChanged {
        /// The key material.
        key: Vec<u8>,
        /// The index (id) of the key.
        key_index: u32,
        /// The member this key belongs to.
        membership: CallMembershipIdentity,
        /// The identity of the member on the RTC backend.
        rtc_backend_identity: String,
    },
    /// The connection status of the membership manager changed.
    StatusChanged {
        /// The previous status.
        previous: Status,
        /// The new status.
        current: Status,
    },
    /// Whether the server probably already sent our delayed leave event.
    ProbablyLeft(bool),
    /// The ID of our scheduled delayed leave event changed.
    DelayIdChanged(Option<String>),
    /// The membership manager shut down due to an unrecoverable error.
    MembershipManagerError(String),
    /// The session sent a call notification because we joined the call as
    /// the first member. Carries the content of the sent notification plus
    /// its `event_id`.
    DidSendCallNotification(JsonValue),
}

struct SessionState {
    /// Whether the encryption machinery has been created at least once,
    /// mirroring the js-sdk where `membershipManager` is only created on the
    /// first join.
    machinery_created: bool,
    pending_notification: Option<String>,
    transport: Option<Arc<ToDeviceKeyTransport>>,
    encryption_manager: Option<Arc<RtcEncryptionManager>>,
    pump_tasks: Vec<tokio::task::JoinHandle<()>>,
}

struct SessionCtx {
    client: Arc<dyn RtcClientApi>,
    room: RtcRoom,
    own_identity: CallMembershipIdentity,
    config: RtcCallSessionConfig,
    membership_manager: MembershipManager,
    statistics: Arc<Mutex<Statistics>>,
    memberships: Arc<Mutex<Vec<CallMembership>>>,
    state: Mutex<SessionState>,
    subscribers: Mutex<Vec<mpsc::UnboundedSender<RtcCallSessionEvent>>>,
}

impl SessionCtx {
    fn emit(&self, event: &RtcCallSessionEvent) {
        let mut subscribers = self.subscribers.lock().unwrap();
        subscribers.retain(|tx| tx.send(event.clone()).is_ok());
    }
}

/// An `RtcCallSession` manages the membership, encryption keys and
/// properties of a MatrixRTC room call session.
///
/// This class doesn't deal with media at all, just the membership and
/// properties of a session. The application feeds room state updates in via
/// [`Self::on_room_state_update`] and to-device events via
/// [`Self::on_to_device_event`], and observes the session through
/// [`Self::subscribe`].
pub struct RtcCallSession {
    ctx: Arc<SessionCtx>,
}

impl RtcCallSession {
    /// Create a session for the room-wide call in the given room.
    ///
    /// Fails if `own_identity.user_id` is not a valid Matrix user ID.
    pub fn new(
        client: Arc<dyn RtcClientApi>,
        room: RtcRoom,
        own_identity: CallMembershipIdentity,
        config: RtcCallSessionConfig,
    ) -> Result<Self, ruma::IdParseError> {
        let user_id = OwnedUserId::try_from(own_identity.user_id.clone())?;
        let device_id = OwnedDeviceId::from(own_identity.device_id.as_str());

        let membership_manager = MembershipManager::new(
            config.membership.clone(),
            room.clone(),
            user_id,
            device_id,
            SlotDescription::room_call(),
            Arc::clone(&client),
        );

        Ok(Self {
            ctx: Arc::new(SessionCtx {
                client,
                room,
                own_identity,
                config,
                membership_manager,
                statistics: Arc::new(Mutex::new(Statistics::default())),
                memberships: Arc::new(Mutex::new(Vec::new())),
                state: Mutex::new(SessionState {
                    machinery_created: false,
                    pending_notification: None,
                    transport: None,
                    encryption_manager: None,
                    pump_tasks: Vec::new(),
                }),
                subscribers: Mutex::new(Vec::new()),
            }),
        })
    }

    /// Subscribe to the events emitted by this session.
    pub fn subscribe(&self) -> mpsc::UnboundedReceiver<RtcCallSessionEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.ctx.subscribers.lock().unwrap().push(tx);
        rx
    }

    /// Whether we intend to be participating in the session.
    pub fn is_joined(&self) -> bool {
        self.ctx.membership_manager.is_activated()
    }

    /// The current memberships of the session, oldest first.
    pub fn memberships(&self) -> Vec<CallMembership> {
        self.ctx.memberships.lock().unwrap().clone()
    }

    /// The connection status of the membership manager, or `None` if the
    /// session was never joined.
    pub fn membership_status(&self) -> Option<Status> {
        self.ctx
            .state
            .lock()
            .unwrap()
            .machinery_created
            .then(|| self.ctx.membership_manager.status())
    }

    /// Whether the server probably already sent our delayed leave event, or
    /// `None` if the session was never joined.
    pub fn probably_left(&self) -> Option<bool> {
        self.ctx
            .state
            .lock()
            .unwrap()
            .machinery_created
            .then(|| self.ctx.membership_manager.probably_left())
    }

    /// The ID of our scheduled delayed leave event, if any.
    pub fn delay_id(&self) -> Option<String> {
        self.ctx.membership_manager.delay_id()
    }

    /// The key distribution statistics of this session.
    pub fn statistics(&self) -> Statistics {
        *self.ctx.statistics.lock().unwrap()
    }

    /// The encryption keys currently known to the session, or `None` if the
    /// session was never joined.
    pub fn get_encryption_keys(&self) -> Option<HashMap<String, Vec<KeyRingEntry>>> {
        self.ctx
            .state
            .lock()
            .unwrap()
            .encryption_manager
            .as_ref()
            .map(|manager| manager.get_encryption_keys())
    }

    /// The transport (focus) in use by the session, resolved with the
    /// `oldest_membership` selection method.
    pub fn get_active_focus(&self) -> Option<Transport> {
        let session = MatrixRtcSession {
            memberships: self.memberships(),
        };
        session.get_active_focus(&FocusActive::livekit_oldest_membership())
    }

    /// The call intent of the session, based on what members advertise. If
    /// members disagree, or nobody specifies one, `None` is returned.
    pub fn get_consensus_call_intent(&self) -> Option<String> {
        let memberships = self.memberships();
        let first_intent = memberships
            .iter()
            .find_map(|m| m.call_intent().map(ToOwned::to_owned))?;
        if memberships
            .iter()
            .all(|m| m.call_intent().is_none_or(|intent| intent == first_intent))
        {
            Some(first_intent)
        } else {
            None
        }
    }

    /// Announce this user and device as joined to the session, and keep the
    /// membership valid until [`Self::leave_rtc_session`] is called.
    ///
    /// This returns immediately; the session is joined in the background.
    pub fn join_rtc_session(&self, foci_preferred: Vec<Transport>) {
        if self.is_joined() {
            info!(
                "Already joined to session in room {}: ignoring join call",
                self.ctx.room.room_id
            );
            return;
        }

        let transport = Arc::new(ToDeviceKeyTransport::new(
            self.ctx.own_identity.clone(),
            self.ctx.room.room_id.to_string(),
            Arc::clone(&self.ctx.client),
            Arc::clone(&self.ctx.statistics),
        ));

        let encryption_manager = {
            let memberships = Arc::clone(&self.ctx.memberships);
            let weak_ctx = Arc::downgrade(&self.ctx);
            Arc::new(RtcEncryptionManager::new(
                self.ctx.own_identity.clone(),
                move || memberships.lock().unwrap().clone(),
                Arc::clone(&transport) as Arc<dyn KeyTransport>,
                move |key, key_index, membership, rtc_backend_identity| {
                    if let Some(ctx) = weak_ctx.upgrade() {
                        ctx.emit(&RtcCallSessionEvent::EncryptionKeyChanged {
                            key: key.to_vec(),
                            key_index,
                            membership: membership.clone(),
                            rtc_backend_identity: rtc_backend_identity.to_owned(),
                        });
                    }
                },
                None,
            ))
        };

        // Pump: keys received by the transport are fed into the encryption
        // manager.
        let key_pump = {
            let mut received = transport.subscribe();
            let encryption_manager = Arc::clone(&encryption_manager);
            tokio::spawn(async move {
                while let Some(event) = received.recv().await {
                    encryption_manager.on_new_key_received(
                        event.membership,
                        &event.key_base64,
                        event.index,
                        event.timestamp,
                    );
                }
            })
        };

        // Forward membership manager events to the session event stream.
        let manager_pump = {
            let mut events = self.ctx.membership_manager.subscribe();
            let weak_ctx = Arc::downgrade(&self.ctx);
            tokio::spawn(async move {
                while let Some(event) = events.recv().await {
                    let Some(ctx) = weak_ctx.upgrade() else {
                        break;
                    };
                    let event = match event {
                        MembershipManagerEvent::StatusChanged { previous, current } => {
                            RtcCallSessionEvent::StatusChanged { previous, current }
                        }
                        MembershipManagerEvent::ProbablyLeft(probably_left) => {
                            RtcCallSessionEvent::ProbablyLeft(probably_left)
                        }
                        MembershipManagerEvent::DelayIdChanged(delay_id) => {
                            RtcCallSessionEvent::DelayIdChanged(delay_id)
                        }
                        MembershipManagerEvent::Error(message) => {
                            error!(
                                "MembershipManager encountered an unrecoverable error: {message}"
                            );
                            RtcCallSessionEvent::MembershipManagerError(message)
                        }
                    };
                    ctx.emit(&event);
                }
            })
        };

        {
            let mut state = self.ctx.state.lock().unwrap();
            state.machinery_created = true;
            state
                .pending_notification
                .clone_from(&self.ctx.config.notification_type);
            state.transport = Some(transport);
            state.encryption_manager = Some(Arc::clone(&encryption_manager));
            state.pump_tasks.push(key_pump);
            state.pump_tasks.push(manager_pump);
        }

        // Join!
        self.ctx.membership_manager.join(foci_preferred);
        encryption_manager.join(self.ctx.config.encryption.clone());

        self.ctx.emit(&RtcCallSessionEvent::JoinStateChanged(true));
    }

    /// Announce this user and device as having left the session and stop the
    /// scheduled updates.
    ///
    /// Returns whether the membership update went through before the
    /// optional timeout.
    pub async fn leave_rtc_session(&self, timeout: Option<Duration>) -> bool {
        if !self.is_joined() {
            info!(
                "Not joined to session in room {}: ignoring leave call",
                self.ctx.room.room_id
            );
            return false;
        }

        info!("Leaving call session in room {}", self.ctx.room.room_id);

        let (encryption_manager, pump_tasks) = {
            let mut state = self.ctx.state.lock().unwrap();
            (
                state.encryption_manager.clone(),
                std::mem::take(&mut state.pump_tasks),
            )
        };
        if let Some(encryption_manager) = encryption_manager {
            encryption_manager.leave();
        }

        let left = self.ctx.membership_manager.leave(timeout).await;
        self.ctx.emit(&RtcCallSessionEvent::JoinStateChanged(false));

        for task in pump_tasks {
            task.abort();
        }

        left
    }

    /// Feed an incoming (decrypted) to-device event into the session.
    pub fn on_to_device_event(&self, event: &ToDeviceEvent) -> Result<(), MalformedKeyEvent> {
        let transport = self.ctx.state.lock().unwrap().transport.clone();
        match transport {
            Some(transport) => transport.on_to_device_event(event),
            None => Ok(()),
        }
    }

    /// Call this when the `m.call.member` room state (or the room member
    /// list) may have changed.
    ///
    /// * `member_events` - The current `org.matrix.msc3401.call.member` state
    ///   events of the room.
    /// * `is_joined_room_member` - Whether the given user ID is a joined member
    ///   of the room.
    /// * `now` - The current time in milliseconds since the Unix epoch.
    pub fn on_room_state_update(
        &self,
        member_events: &[MemberStateEvent],
        is_joined_room_member: impl Fn(&str) -> bool,
        now: u64,
    ) {
        let new_memberships =
            MatrixRtcSession::room_call_memberships(member_events, is_joined_room_member, now);

        let old_memberships = {
            let mut memberships = self.ctx.memberships.lock().unwrap();
            let old = memberships.clone();
            memberships.clone_from(&new_memberships);
            old
        };

        let changed = old_memberships.len() != new_memberships.len()
            || old_memberships
                .iter()
                .zip(new_memberships.iter())
                .any(|(a, b)| !CallMembership::data_equal(a, b));

        if changed {
            info!(
                "Memberships for call in room {} have changed ({} members)",
                self.ctx.room.room_id,
                new_memberships.len()
            );
            self.ctx.emit(&RtcCallSessionEvent::MembershipsChanged {
                old: old_memberships.clone(),
                new: new_memberships.clone(),
            });

            self.ctx
                .membership_manager
                .on_rtc_session_member_update(&new_memberships);

            // If we are the first member in the call, we are responsible for
            // sending the notification event.
            let own_membership = self.ctx.membership_manager.own_membership();
            let pending_notification = self.ctx.state.lock().unwrap().pending_notification.clone();
            if let (Some(notification_type), Some(own_membership)) =
                (pending_notification, own_membership)
                && old_memberships.is_empty()
            {
                self.send_call_notify(
                    own_membership.event_id(),
                    &notification_type,
                    own_membership.call_intent(),
                );
            }
            // If anyone else joins the session it is no longer our
            // responsibility to send the notification. (If we were the
            // joiner we already sent the notification above.)
            if !new_memberships.is_empty() {
                self.ctx.state.lock().unwrap().pending_notification = None;
            }
        }

        // This also needs to be done if nothing changed: a member might have
        // updated their fingerprint (`created_ts`).
        let encryption_manager = self.ctx.state.lock().unwrap().encryption_manager.clone();
        if let Some(encryption_manager) = encryption_manager {
            encryption_manager.on_memberships_update();
        }
    }

    /// Send a notification event to indicate that the call has started.
    ///
    /// This does not block; the notification event is sent in the
    /// background.
    fn send_call_notify(
        &self,
        parent_event_id: &str,
        notification_type: &str,
        call_intent: Option<&str>,
    ) {
        let mut content = json!({
            "m.mentions": { "user_ids": [], "room": true },
            "notification_type": notification_type,
            "m.relates_to": {
                "event_id": parent_event_id,
                "rel_type": "m.reference",
            },
            "sender_ts": now_ms(),
            "lifetime": 30_000,
        });
        if let Some(call_intent) = call_intent {
            content
                .as_object_mut()
                .expect("content is an object")
                .insert("m.call.intent".to_owned(), call_intent.into());
        }

        let ctx = Arc::clone(&self.ctx);
        tokio::spawn(async move {
            match ctx
                .client
                .send_event(
                    &ctx.room.room_id,
                    RTC_NOTIFICATION_EVENT_TYPE,
                    content.clone(),
                )
                .await
            {
                Ok(response) => {
                    content
                        .as_object_mut()
                        .expect("content is an object")
                        .insert("event_id".to_owned(), response.event_id.to_string().into());
                    ctx.emit(&RtcCallSessionEvent::DidSendCallNotification(content));
                }
                Err(error) => {
                    error!("Failed to send call notification: {error}");
                }
            }
        });
    }
}

/// Activity change of the room call session of one room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionActivity {
    /// A member has joined the session, creating an active session in a room
    /// where there wasn't one previously.
    Started(String),
    /// All participants have left the session.
    Ended(String),
}

/// Detects sessions starting and ending across rooms, mirroring
/// `matrix-js-sdk`'s `MatrixRTCSessionManager` session start/end detection.
///
/// Feed it the current member count of each room's call session whenever it
/// may have changed.
#[derive(Debug, Default)]
pub struct SessionActivityTracker {
    /// Room ID -> whether the session was known and active.
    known_active: HashMap<String, bool>,
}

impl SessionActivityTracker {
    /// Create an empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Update the member count of the call session in `room_id`. Returns the
    /// resulting activity change, if any.
    pub fn update(&mut self, room_id: &str, member_count: usize) -> Option<SessionActivity> {
        let was_active_and_known = self.known_active.get(room_id).copied().unwrap_or(false);
        let now_active = member_count > 0;
        self.known_active.insert(room_id.to_owned(), now_active);

        if was_active_and_known && !now_active {
            Some(SessionActivity::Ended(room_id.to_owned()))
        } else if !was_active_and_known && now_active {
            Some(SessionActivity::Started(room_id.to_owned()))
        } else {
            None
        }
    }
}

fn now_ms() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}
