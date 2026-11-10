// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared test helpers: a channel-recording mock of [`RtcClientApi`] and
//! fixtures ported from `matrix-js-sdk`'s `spec/unit/matrixrtc/mocks.ts`.

#![allow(dead_code)]
#![allow(clippy::enum_variant_names)]

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use mandelbrot_matrixrtc::{
    CallMembership, ClientError, MemberStateEvent, MembershipConfig, MembershipManager,
    MembershipManagerEvent, RtcClientApi, RtcRoom, SendDelayedEventResponse, SendEventResponse,
    SlotDescription, ToDeviceTarget, Transport,
};
use ruma::{OwnedDeviceId, OwnedRoomId, OwnedUserId, RoomId, events::StateEventType};
use serde_json::{Value as JsonValue, json};
use tokio::sync::{mpsc, oneshot};

pub const SESSION_MEMBERSHIP_TEMPLATE: &str =
    include_str!("../fixtures/session_membership_template.json");
pub const EXPECTED_JOIN_CONTENT: &str = include_str!("../fixtures/expected_join_content.json");

/// The mockable methods of the client API.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    SendStateEvent,
    SendDelayedStateEvent,
    RestartDelayedEvent,
    CancelDelayedEvent,
    SendDelayedEvent,
    SendEvent,
    EncryptAndSendToDevice,
}

/// A recorded call to the mock client.
#[derive(Clone, Debug)]
pub enum RecordedCall {
    SendStateEvent {
        room_id: String,
        event_type: String,
        state_key: String,
        content: JsonValue,
    },
    SendDelayedStateEvent {
        room_id: String,
        delay_ms: u64,
        event_type: String,
        state_key: String,
        content: JsonValue,
    },
    UpdateDelayedEvent {
        delay_id: String,
        method: Method,
    },
    SendEvent {
        room_id: String,
        event_type: String,
        content: JsonValue,
    },
    EncryptAndSendToDevice {
        event_type: String,
        targets: Vec<ToDeviceTarget>,
        content: JsonValue,
    },
}

impl RecordedCall {
    pub fn method(&self) -> Method {
        match self {
            Self::SendStateEvent { .. } => Method::SendStateEvent,
            Self::SendDelayedStateEvent { .. } => Method::SendDelayedStateEvent,
            Self::UpdateDelayedEvent { method, .. } => *method,
            Self::SendEvent { .. } => Method::SendEvent,
            Self::EncryptAndSendToDevice { .. } => Method::EncryptAndSendToDevice,
        }
    }
}

type MockResult = Result<JsonValue, ClientError>;

enum Reply {
    Value(MockResult),
    Pending(oneshot::Receiver<MockResult>),
}

enum DefaultReply {
    Value(MockResult),
    /// Never resolves; simulates a server that does not answer.
    Stuck,
}

/// A channel-recording mock of the [`RtcClientApi`].
pub struct MockClient {
    calls: Mutex<Vec<RecordedCall>>,
    queues: Mutex<HashMap<Method, VecDeque<Reply>>>,
    defaults: Mutex<HashMap<Method, DefaultReply>>,
}

impl MockClient {
    /// A mock with the default "non error" server behavior.
    pub fn new() -> Arc<Self> {
        let client = Self {
            calls: Mutex::new(Vec::new()),
            queues: Mutex::new(HashMap::new()),
            defaults: Mutex::new(HashMap::new()),
        };
        client.set_default(
            Method::SendStateEvent,
            Ok(json!({ "event_id": "$id:e.org" })),
        );
        client.set_default(
            Method::SendDelayedStateEvent,
            Ok(json!({ "delay_id": "id" })),
        );
        client.set_default(Method::RestartDelayedEvent, Ok(json!({})));
        client.set_default(Method::CancelDelayedEvent, Ok(json!({})));
        client.set_default(Method::SendDelayedEvent, Ok(json!({})));
        client.set_default(Method::SendEvent, Ok(json!({ "event_id": "$id:e.org" })));
        client.set_default(Method::EncryptAndSendToDevice, Ok(json!({})));
        Arc::new(client)
    }

    /// Set the default reply of a method.
    pub fn set_default(&self, method: Method, result: MockResult) {
        self.defaults
            .lock()
            .unwrap()
            .insert(method, DefaultReply::Value(result));
    }

    /// Make a method never resolve by default.
    pub fn set_stuck(&self, method: Method) {
        self.defaults
            .lock()
            .unwrap()
            .insert(method, DefaultReply::Stuck);
    }

    /// Queue a one-time reply for a method, used before the default.
    pub fn enqueue(&self, method: Method, result: MockResult) {
        self.queues
            .lock()
            .unwrap()
            .entry(method)
            .or_default()
            .push_back(Reply::Value(result));
    }

    /// Queue a one-time reply whose resolution is controlled by the returned
    /// sender.
    pub fn pending(&self, method: Method) -> oneshot::Sender<MockResult> {
        let (tx, rx) = oneshot::channel();
        self.queues
            .lock()
            .unwrap()
            .entry(method)
            .or_default()
            .push_back(Reply::Pending(rx));
        tx
    }

    /// All recorded calls.
    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
    }

    /// All recorded calls of the given method.
    pub fn calls_of(&self, method: Method) -> Vec<RecordedCall> {
        self.calls()
            .into_iter()
            .filter(|c| c.method() == method)
            .collect()
    }

    /// The number of recorded calls of the given method.
    pub fn count(&self, method: Method) -> usize {
        self.calls_of(method).len()
    }

    /// Forget all recorded calls.
    pub fn clear_calls(&self) {
        self.calls.lock().unwrap().clear();
    }

    async fn reply(&self, method: Method) -> MockResult {
        let reply = self
            .queues
            .lock()
            .unwrap()
            .entry(method)
            .or_default()
            .pop_front();
        if let Some(reply) = reply {
            match reply {
                Reply::Value(result) => result,
                Reply::Pending(rx) => rx
                    .await
                    .unwrap_or_else(|_| Err(ClientError::Other("pending reply dropped".into()))),
            }
        } else {
            let default = match self.defaults.lock().unwrap().get(&method) {
                Some(DefaultReply::Value(result)) => Some(result.clone()),
                Some(DefaultReply::Stuck) => None,
                None => Some(Err(ClientError::Other("no mock behavior".into()))),
            };
            match default {
                Some(result) => result,
                None => std::future::pending().await,
            }
        }
    }
}

#[async_trait::async_trait]
impl RtcClientApi for MockClient {
    async fn send_state_event(
        &self,
        room_id: &RoomId,
        event_type: StateEventType,
        state_key: &str,
        content: JsonValue,
    ) -> Result<SendEventResponse, ClientError> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::SendStateEvent {
                room_id: room_id.to_string(),
                event_type: event_type.to_string(),
                state_key: state_key.to_owned(),
                content,
            });
        self.reply(Method::SendStateEvent)
            .await
            .map(|_| SendEventResponse {
                event_id: ruma::OwnedEventId::try_from("$id:e.org").unwrap(),
            })
    }

    async fn send_delayed_state_event(
        &self,
        room_id: &RoomId,
        delay: Duration,
        event_type: StateEventType,
        state_key: &str,
        content: JsonValue,
    ) -> Result<SendDelayedEventResponse, ClientError> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::SendDelayedStateEvent {
                room_id: room_id.to_string(),
                delay_ms: u64::try_from(delay.as_millis()).unwrap(),
                event_type: event_type.to_string(),
                state_key: state_key.to_owned(),
                content,
            });
        self.reply(Method::SendDelayedStateEvent)
            .await
            .map(|value| SendDelayedEventResponse {
                delay_id: value
                    .get("delay_id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("id")
                    .to_owned(),
            })
    }

    async fn update_delayed_event(
        &self,
        delay_id: &str,
        action: mandelbrot_matrixrtc::UpdateDelayedEventAction,
    ) -> Result<(), ClientError> {
        use mandelbrot_matrixrtc::UpdateDelayedEventAction as Action;
        let method = match action {
            Action::Restart => Method::RestartDelayedEvent,
            Action::Cancel => Method::CancelDelayedEvent,
            Action::Send => Method::SendDelayedEvent,
        };
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::UpdateDelayedEvent {
                delay_id: delay_id.to_owned(),
                method,
            });
        self.reply(method).await.map(|_| ())
    }

    async fn send_event(
        &self,
        room_id: &RoomId,
        event_type: &str,
        content: JsonValue,
    ) -> Result<SendEventResponse, ClientError> {
        self.calls.lock().unwrap().push(RecordedCall::SendEvent {
            room_id: room_id.to_string(),
            event_type: event_type.to_owned(),
            content,
        });
        self.reply(Method::SendEvent)
            .await
            .map(|_| SendEventResponse {
                event_id: ruma::OwnedEventId::try_from("$id:e.org").unwrap(),
            })
    }

    async fn encrypt_and_send_to_device(
        &self,
        event_type: &str,
        targets: &[ToDeviceTarget],
        content: JsonValue,
    ) -> Result<(), ClientError> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall::EncryptAndSendToDevice {
                event_type: event_type.to_owned(),
                targets: targets.to_vec(),
                content,
            });
        self.reply(Method::EncryptAndSendToDevice).await.map(|_| ())
    }
}

pub fn room_id() -> OwnedRoomId {
    OwnedRoomId::try_from("!room:example.org").unwrap()
}

pub fn alice() -> OwnedUserId {
    OwnedUserId::try_from("@alice:example.org").unwrap()
}

pub fn alice_device() -> OwnedDeviceId {
    OwnedDeviceId::from("AAAAAAA")
}

/// Construct a manager for `@alice:example.org`/`AAAAAAA` on the room call
/// slot.
pub fn make_manager(config: MembershipConfig, client: Arc<MockClient>) -> MembershipManager {
    make_manager_for_slot(config, client, SlotDescription::room_call(), "default")
}

pub fn make_manager_for_slot(
    config: MembershipConfig,
    client: Arc<MockClient>,
    slot: SlotDescription,
    room_version: &str,
) -> MembershipManager {
    MembershipManager::new(
        config,
        RtcRoom {
            room_id: room_id(),
            version: room_version.to_owned(),
        },
        alice(),
        alice_device(),
        slot,
        client,
    )
}

/// The `focus` transport used throughout the `MembershipManager` spec.
pub fn focus() -> Transport {
    serde_json::from_value(json!({
        "type": "livekit",
        "livekit_service_url": "https://active.url",
        "livekit_alias": "!active:active.url",
    }))
    .unwrap()
}

/// The `sessionMembershipTemplate` fixture content.
pub fn session_membership_template() -> JsonValue {
    serde_json::from_str(SESSION_MEMBERSHIP_TEMPLATE).unwrap()
}

/// Port of `mockCallMembership`: build a [`CallMembership`] for the given
/// content and sender.
pub fn mock_call_membership(content: JsonValue, sender: &str, ts: u64) -> CallMembership {
    CallMembership::parse_from_event(&MemberStateEvent {
        event_id: "$event:e.org".to_owned(),
        sender: sender.to_owned(),
        origin_server_ts: ts,
        state_key: format!("_{sender}_AAAAAAA"),
        content,
    })
    .unwrap()
}

/// Advance the paused tokio clock and let background tasks settle.
pub async fn advance(ms: u64) {
    tokio::time::advance(Duration::from_millis(ms)).await;
    settle().await;
}

/// Let background tasks make progress without advancing time.
pub async fn settle() {
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }
}

/// Drain all currently pending events from an event receiver.
pub fn drain_events(
    rx: &mut mpsc::UnboundedReceiver<MembershipManagerEvent>,
) -> Vec<MembershipManagerEvent> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}
