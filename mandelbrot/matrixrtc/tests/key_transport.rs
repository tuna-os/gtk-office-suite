// SPDX-License-Identifier: GPL-3.0-or-later

//! Port of `matrix-js-sdk`'s `ToDeviceKeyTransport.spec.ts` and
//! `OutdatedKeyFilter.spec.ts`.

mod common;

use std::sync::{Arc, Mutex};

use common::{Method, MockClient, RecordedCall};
use mandelbrot_matrixrtc::{
    CallMembershipIdentity, InboundEncryptionSession, KeyTransport, OutdatedKeyFilter, Statistics,
    ToDeviceEvent, ToDeviceKeyTransport, ToDeviceTarget,
};
use serde_json::{Value as JsonValue, json};
use tokio::sync::mpsc;

const ROOM_ID: &str = "!room:id";

struct Fixture {
    client: Arc<MockClient>,
    statistics: Arc<Mutex<Statistics>>,
    transport: ToDeviceKeyTransport,
}

fn make_transport() -> Fixture {
    let client = MockClient::new();
    let statistics = Arc::new(Mutex::new(Statistics::default()));
    let transport = ToDeviceKeyTransport::new(
        CallMembershipIdentity {
            user_id: "@alice:example.org".to_owned(),
            device_id: "MYDEVICE".to_owned(),
            member_id: "@alice:example.org:MYDEVICE".to_owned(),
        },
        ROOM_ID.to_owned(),
        client.clone(),
        statistics.clone(),
    );
    Fixture {
        client,
        statistics,
        transport,
    }
}

fn member(user_id: &str, device_id: &str) -> mandelbrot_matrixrtc::ParticipantDeviceInfo {
    mandelbrot_matrixrtc::ParticipantDeviceInfo {
        user_id: user_id.to_owned(),
        device_id: device_id.to_owned(),
        membership_ts: 1234,
    }
}

fn key_event(sender: &str, content: JsonValue) -> ToDeviceEvent {
    ToDeviceEvent {
        sender: sender.to_owned(),
        event_type: "io.element.call.encryption_keys".to_owned(),
        content,
    }
}

fn drain(rx: &mut mpsc::UnboundedReceiver<mandelbrot_matrixrtc::ReceivedKeyEvent>) -> usize {
    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    count
}

#[tokio::test]
async fn should_send_my_keys_on_via_to_device() {
    let fixture = make_transport();
    fixture.transport.start();

    let key_base64_encoded = "ABCDEDF";
    let key_index = 2;
    fixture
        .transport
        .send_key(
            key_base64_encoded,
            key_index,
            &[
                member("@bob:example.org", "BOBDEVICE"),
                member("@carl:example.org", "CARLDEVICE"),
                member("@mat:example.org", "MATDEVICE"),
            ],
        )
        .await
        .unwrap();

    let calls = fixture.client.calls_of(Method::EncryptAndSendToDevice);
    assert_eq!(calls.len(), 1);
    let RecordedCall::EncryptAndSendToDevice {
        event_type,
        targets,
        content,
    } = &calls[0]
    else {
        unreachable!()
    };
    assert_eq!(event_type, "io.element.call.encryption_keys");
    assert_eq!(
        targets,
        &[
            ToDeviceTarget {
                user_id: "@bob:example.org".to_owned(),
                device_id: "BOBDEVICE".to_owned(),
            },
            ToDeviceTarget {
                user_id: "@carl:example.org".to_owned(),
                device_id: "CARLDEVICE".to_owned(),
            },
            ToDeviceTarget {
                user_id: "@mat:example.org".to_owned(),
                device_id: "MATDEVICE".to_owned(),
            },
        ]
    );

    // `sent_ts` is a timestamp; check it exists then compare the rest field
    // for field.
    assert!(content.get("sent_ts").is_some_and(JsonValue::is_number));
    let mut content = content.clone();
    content.as_object_mut().unwrap().remove("sent_ts");
    assert_eq!(
        content,
        json!({
            "keys": {
                "index": key_index,
                "key": key_base64_encoded,
            },
            "member": {
                "claimed_device_id": "MYDEVICE",
                "id": "@alice:example.org:MYDEVICE",
            },
            "room_id": ROOM_ID,
            "session": {
                "application": "m.call",
                "call_id": "",
                "scope": "m.room",
            },
        })
    );

    assert_eq!(fixture.statistics.lock().unwrap().encryption_keys_sent, 1);
}

#[tokio::test]
async fn should_emit_when_a_key_is_received() {
    let fixture = make_transport();
    let mut received = fixture.transport.subscribe();
    fixture.transport.start();

    let test_encoded = "ABCDEDF";
    let test_key_index = 2;

    fixture
        .transport
        .on_to_device_event(&key_event(
            "@bob:example.org",
            json!({
                "keys": {
                    "index": test_key_index,
                    "key": test_encoded,
                },
                "member": {
                    "claimed_device_id": "BOBDEVICE",
                },
                "room_id": ROOM_ID,
                "session": {
                    "application": "m.call",
                    "call_id": "",
                    "scope": "m.room",
                },
            }),
        ))
        .unwrap();

    let event = received.try_recv().unwrap();
    assert_eq!(event.membership.user_id, "@bob:example.org");
    assert_eq!(event.membership.device_id, "BOBDEVICE");
    assert_eq!(event.key_base64, test_encoded);
    assert_eq!(event.index, test_key_index);

    assert_eq!(
        fixture.statistics.lock().unwrap().encryption_keys_received,
        1
    );
}

#[tokio::test]
async fn should_not_sent_to_ourself() {
    let fixture = make_transport();

    fixture
        .transport
        .send_key("ABCDEDF", 2, &[member("@alice:example.org", "MYDEVICE")])
        .await
        .unwrap();

    fixture.transport.start();

    assert_eq!(fixture.client.count(Method::EncryptAndSendToDevice), 0);
    assert_eq!(fixture.statistics.lock().unwrap().encryption_keys_sent, 0);
}

#[tokio::test]
async fn should_warn_when_there_is_a_room_mismatch() {
    let fixture = make_transport();
    let mut received = fixture.transport.subscribe();
    fixture.transport.start();

    let result = fixture.transport.on_to_device_event(&key_event(
        "@bob:example.org",
        json!({
            "keys": {
                "index": 2,
                "key": "ABCDEDF",
            },
            "member": {
                "claimed_device_id": "BOBDEVICE",
            },
            "room_id": "!anotherroom:id",
            "session": {
                "application": "m.call",
                "call_id": "",
                "scope": "m.room",
            },
        }),
    ));

    assert_eq!(
        result.unwrap_err().to_string(),
        "Malformed Event: Mismatch roomId"
    );
    assert_eq!(
        fixture.statistics.lock().unwrap().encryption_keys_received,
        0
    );
    assert_eq!(drain(&mut received), 0);
}

#[tokio::test]
async fn should_warn_on_malformed_event() {
    let malformed_events = [
        json!({
            "keys": {},
            "member": { "claimed_device_id": "MYDEVICE" },
            "room_id": "!room:id",
            "session": { "application": "m.call", "call_id": "", "scope": "m.room" },
        }),
        json!({
            "keys": { "index": 0 },
            "member": { "claimed_device_id": "MYDEVICE" },
            "room_id": "!room:id",
            "session": { "application": "m.call", "call_id": "", "scope": "m.room" },
        }),
        json!({
            "keys": { "key": "ABCDEF" },
            "member": { "claimed_device_id": "MYDEVICE" },
            "room_id": "!room:id",
            "session": { "application": "m.call", "call_id": "", "scope": "m.room" },
        }),
        json!({
            "keys": { "key": "ABCDEF", "index": 2 },
            "room_id": "!room:id",
            "session": { "application": "m.call", "call_id": "", "scope": "m.room" },
        }),
        json!({
            "keys": { "key": "ABCDEF", "index": 2 },
            "member": {},
            "room_id": "!room:id",
            "session": { "application": "m.call", "call_id": "", "scope": "m.room" },
        }),
        json!({
            "keys": { "key": "ABCDEF", "index": 2 },
            "member": { "claimed_device_id": "MYDEVICE" },
            "session": { "application": "m.call", "call_id": "", "scope": "m.room" },
        }),
        json!({
            "keys": { "key": "ABCDEF", "index": 2 },
            "member": { "claimed_device_id": "MYDEVICE" },
            "room_id": "!wrong_room",
            "session": { "application": "m.call", "call_id": "", "scope": "m.room" },
        }),
    ];

    for event_content in malformed_events {
        let fixture = make_transport();
        let mut received = fixture.transport.subscribe();
        fixture.transport.start();

        let result = fixture
            .transport
            .on_to_device_event(&key_event("@bob:example.org", event_content.clone()));

        assert!(result.is_err(), "event should be rejected: {event_content}");
        assert_eq!(
            fixture.statistics.lock().unwrap().encryption_keys_received,
            0
        );
        assert_eq!(drain(&mut received), 0);
    }
}

#[tokio::test]
async fn should_ignore_events_before_start_and_after_stop() {
    let fixture = make_transport();
    let mut received = fixture.transport.subscribe();

    let content = json!({
        "keys": { "index": 2, "key": "ABCDEDF" },
        "member": { "claimed_device_id": "BOBDEVICE" },
        "room_id": ROOM_ID,
        "session": { "application": "m.call", "call_id": "", "scope": "m.room" },
    });

    // Not started yet: ignored.
    fixture
        .transport
        .on_to_device_event(&key_event("@bob:example.org", content.clone()))
        .unwrap();
    assert_eq!(drain(&mut received), 0);

    fixture.transport.start();
    fixture
        .transport
        .on_to_device_event(&key_event("@bob:example.org", content.clone()))
        .unwrap();
    assert_eq!(drain(&mut received), 1);

    fixture.transport.stop();
    fixture
        .transport
        .on_to_device_event(&key_event("@bob:example.org", content))
        .unwrap();
    assert_eq!(drain(&mut received), 0);
}

// OutdatedKeyFilter

fn fake_inbound_session_with_timestamp(ts: u64) -> InboundEncryptionSession {
    InboundEncryptionSession {
        key_index: 0,
        creation_ts: ts,
        membership: CallMembershipIdentity {
            user_id: "@alice:localhost".to_owned(),
            device_id: "ABDE".to_owned(),
            member_id: "@alice:localhost:ABCDE".to_owned(),
        },
        key: vec![0; 16],
    }
}

#[test]
fn should_buffer_and_disambiguate_keys_by_timestamp() {
    let mut filter = OutdatedKeyFilter::new();

    let a_key = fake_inbound_session_with_timestamp(1000);
    let older_key = fake_inbound_session_with_timestamp(300);
    // Simulate receiving out of order keys.

    assert!(!filter.is_outdated(&a_key.membership.clone(), &a_key));
    // Then we receive the most recent key out of order; this key is older
    // and should be ignored even if received after.
    assert!(filter.is_outdated(&a_key.membership.clone(), &older_key));
}
