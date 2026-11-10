// SPDX-License-Identifier: GPL-3.0-or-later

//! Transport for sharing MatrixRTC media encryption keys between devices.
//!
//! This is a port of `matrix-js-sdk`'s `IKeyTransport`/`ToDeviceKeyTransport`
//! using `io.element.call.encryption_keys` to-device messages.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use serde_json::{Value as JsonValue, json};
use tokio::sync::mpsc;
use tracing::warn;

use crate::client::{ClientError, RtcClientApi, ToDeviceEvent, ToDeviceTarget};

/// The to-device event type used to share call encryption keys.
pub const CALL_ENCRYPTION_KEYS_EVENT_TYPE: &str = "io.element.call.encryption_keys";

/// The parts identifying one member of a call, from the point of view of the
/// encryption layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallMembershipIdentity {
    /// The Matrix user ID of the member.
    pub user_id: String,
    /// The Matrix device ID of the member.
    pub device_id: String,
    /// The member ID of the member (legacy form: `{user_id}:{device_id}`).
    pub member_id: String,
}

/// The string used for the keys in the encryption key map:
/// `@bob:example.org:DEVICEID(MEMBERID)`.
pub fn encryption_key_map_key(membership: &CallMembershipIdentity) -> String {
    format!(
        "{}:{}({})",
        membership.user_id, membership.device_id, membership.member_id
    )
}

/// A participant device that should receive a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParticipantDeviceInfo {
    /// The Matrix user ID of the participant.
    pub user_id: String,
    /// The Matrix device ID of the participant.
    pub device_id: String,
    /// The `created_ts` of the participant's membership when the key was
    /// shared. Used to detect memberships that were re-created.
    pub membership_ts: u64,
}

/// A key received over a key transport.
#[derive(Clone, Debug)]
pub struct ReceivedKeyEvent {
    /// The claimed identity of the sender.
    pub membership: CallMembershipIdentity,
    /// The key, base64 encoded.
    pub key_base64: String,
    /// The index (id) of the key.
    pub index: u32,
    /// The (local) timestamp in milliseconds at which the key was received.
    pub timestamp: u64,
}

/// Statistics about the key distribution of a session.
#[derive(Clone, Copy, Debug, Default)]
pub struct Statistics {
    /// The number of times we have sent an event containing encryption keys.
    pub encryption_keys_sent: u64,
    /// The number of times we have received an event containing encryption
    /// keys.
    pub encryption_keys_received: u64,
    /// The total age, in milliseconds, of all received encryption key
    /// events.
    pub encryption_keys_received_total_age: i64,
}

/// Generic interface for the transport used to share media keys.
#[async_trait::async_trait]
pub trait KeyTransport: Send + Sync {
    /// Send the current user media key to the given members.
    async fn send_key(
        &self,
        key_base64: &str,
        index: u32,
        members: &[ParticipantDeviceInfo],
    ) -> Result<(), ClientError>;

    /// Start the transport: incoming events will be processed.
    fn start(&self);

    /// Stop the transport: incoming events will be ignored.
    fn stop(&self);
}

/// An invalid `io.element.call.encryption_keys` event.
#[derive(Clone, Debug, thiserror::Error)]
#[error("{0}")]
pub struct MalformedKeyEvent(pub String);

/// `ToDeviceKeyTransport` is used to send MatrixRTC keys to other devices
/// using the to-device CS-API.
///
/// Incoming to-device events are fed in by the application with
/// [`Self::on_to_device_event`]; validated keys are emitted on the channels
/// returned by [`Self::subscribe`].
pub struct ToDeviceKeyTransport {
    membership: CallMembershipIdentity,
    room_id: String,
    client: Arc<dyn RtcClientApi>,
    statistics: Arc<Mutex<Statistics>>,
    active: AtomicBool,
    subscribers: Mutex<Vec<mpsc::UnboundedSender<ReceivedKeyEvent>>>,
}

impl ToDeviceKeyTransport {
    /// Construct a transport for the given own membership identity and room.
    pub fn new(
        membership: CallMembershipIdentity,
        room_id: String,
        client: Arc<dyn RtcClientApi>,
        statistics: Arc<Mutex<Statistics>>,
    ) -> Self {
        Self {
            membership,
            room_id,
            client,
            statistics,
            active: AtomicBool::new(false),
            subscribers: Mutex::new(Vec::new()),
        }
    }

    /// Subscribe to the keys received by this transport.
    pub fn subscribe(&self) -> mpsc::UnboundedReceiver<ReceivedKeyEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.subscribers.lock().unwrap().push(tx);
        rx
    }

    /// Feed an incoming (decrypted) to-device event into the transport.
    ///
    /// Events of other types are silently ignored; malformed
    /// `io.element.call.encryption_keys` events are rejected with an error
    /// (and a warning is logged), mirroring the reference implementation's
    /// validation.
    pub fn on_to_device_event(&self, event: &ToDeviceEvent) -> Result<(), MalformedKeyEvent> {
        if !self.active.load(Ordering::SeqCst) {
            return Ok(());
        }
        if event.event_type != CALL_ENCRYPTION_KEYS_EVENT_TYPE {
            // Ignore, this is not a call encryption event.
            return Ok(());
        }

        let content = self.get_valid_event_content(&event.content).map_err(|e| {
            warn!("{e}");
            e
        })?;

        if event.sender.is_empty() {
            return Ok(());
        }

        self.receive_call_key_event(&event.sender, &content);
        Ok(())
    }

    fn receive_call_key_event(&self, from_user: &str, content: &ValidKeyEventContent) {
        // The event has already been validated at this point.
        let now = now_ms();
        {
            let mut statistics = self.statistics.lock().unwrap();
            statistics.encryption_keys_received += 1;
            let age = i64::try_from(now).unwrap_or(i64::MAX)
                - i64::try_from(content.sent_ts.unwrap_or(now)).unwrap_or(i64::MAX);
            statistics.encryption_keys_received_total_age += age;
        }

        let hardcoded_member_id_alternative = format!("{from_user}:{}", content.claimed_device_id);

        let received = ReceivedKeyEvent {
            // This is claimed information.
            membership: CallMembershipIdentity {
                user_id: from_user.to_owned(),
                device_id: content.claimed_device_id.clone(),
                member_id: content
                    .member_id
                    .clone()
                    .unwrap_or(hardcoded_member_id_alternative),
            },
            key_base64: content.key.clone(),
            index: content.index,
            timestamp: now,
        };

        let mut subscribers = self.subscribers.lock().unwrap();
        subscribers.retain(|tx| tx.send(received.clone()).is_ok());
    }

    fn get_valid_event_content(
        &self,
        content: &JsonValue,
    ) -> Result<ValidKeyEventContent, MalformedKeyEvent> {
        let room_id = content.get("room_id").and_then(JsonValue::as_str);
        match room_id {
            None | Some("") => {
                return Err(MalformedKeyEvent(
                    "Malformed Event: invalid call encryption keys event, no roomId".to_owned(),
                ));
            }
            Some(room_id) if room_id != self.room_id => {
                return Err(MalformedKeyEvent(
                    "Malformed Event: Mismatch roomId".to_owned(),
                ));
            }
            Some(_) => {}
        }

        let keys = content.get("keys");
        let key = keys.and_then(|k| k.get("key")).and_then(JsonValue::as_str);
        let index = keys
            .and_then(|k| k.get("index"))
            .and_then(JsonValue::as_u64);
        let (Some(key), Some(index)) = (key, index) else {
            return Err(MalformedKeyEvent(
                "Malformed Event: Missing keys field".to_owned(),
            ));
        };
        if key.is_empty() {
            return Err(MalformedKeyEvent(
                "Malformed Event: Missing keys field".to_owned(),
            ));
        }

        let claimed_device_id = content
            .get("member")
            .and_then(|m| m.get("claimed_device_id"))
            .and_then(JsonValue::as_str);
        let Some(claimed_device_id) = claimed_device_id else {
            return Err(MalformedKeyEvent(
                "Malformed Event: Missing claimed_device_id".to_owned(),
            ));
        };
        if claimed_device_id.is_empty() {
            return Err(MalformedKeyEvent(
                "Malformed Event: Missing claimed_device_id".to_owned(),
            ));
        }

        Ok(ValidKeyEventContent {
            key: key.to_owned(),
            index: u32::try_from(index)
                .map_err(|_| MalformedKeyEvent("Malformed Event: Missing keys field".to_owned()))?,
            claimed_device_id: claimed_device_id.to_owned(),
            member_id: content
                .get("member")
                .and_then(|m| m.get("id"))
                .and_then(JsonValue::as_str)
                .map(ToOwned::to_owned),
            sent_ts: content.get("sent_ts").and_then(JsonValue::as_u64),
        })
    }
}

struct ValidKeyEventContent {
    key: String,
    index: u32,
    claimed_device_id: String,
    member_id: Option<String>,
    sent_ts: Option<u64>,
}

#[async_trait::async_trait]
impl KeyTransport for ToDeviceKeyTransport {
    async fn send_key(
        &self,
        key_base64: &str,
        index: u32,
        members: &[ParticipantDeviceInfo],
    ) -> Result<(), ClientError> {
        let content = json!({
            "keys": {
                "index": index,
                "key": key_base64,
            },
            "room_id": self.room_id,
            "member": {
                "claimed_device_id": self.membership.device_id,
                "id": self.membership.member_id,
            },
            "session": {
                "call_id": "",
                "application": "m.call",
                "scope": "m.room",
            },
            "sent_ts": now_ms(),
        });

        let targets: Vec<ToDeviceTarget> = members
            .iter()
            .map(|member| ToDeviceTarget {
                user_id: member.user_id.clone(),
                device_id: member.device_id.clone(),
            })
            // Filter out me.
            .filter(|member| {
                !(member.user_id == self.membership.user_id
                    && member.device_id == self.membership.device_id)
            })
            .collect();

        if targets.is_empty() {
            warn!("No targets found for sending key");
            return Ok(());
        }

        self.client
            .encrypt_and_send_to_device(CALL_ENCRYPTION_KEYS_EVENT_TYPE, &targets, content)
            .await?;
        self.statistics.lock().unwrap().encryption_keys_sent += 1;
        Ok(())
    }

    fn start(&self) {
        self.active.store(true, Ordering::SeqCst);
    }

    fn stop(&self) {
        self.active.store(false, Ordering::SeqCst);
    }
}

/// The current wall clock time in milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}
