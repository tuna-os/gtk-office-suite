// SPDX-License-Identifier: GPL-3.0-or-later

//! Port of the join/leave lifecycle tests of `matrix-js-sdk`'s
//! `MatrixRTCSession.spec.ts` (the ones needing `joinRTCSession`) and the
//! session start/end detection of `MatrixRTCSessionManager.spec.ts`.

mod common;

use std::sync::Arc;

use common::{
    Method, MockClient, RecordedCall, advance, focus, session_membership_template, settle,
};
use mandelbrot_matrixrtc::{
    CallMembershipIdentity, MemberStateEvent, MembershipConfig, RtcCallSession,
    RtcCallSessionConfig, RtcCallSessionEvent, RtcRoom, SessionActivity, SessionActivityTracker,
    Status, ToDeviceTarget,
};
use serde_json::{Value as JsonValue, json};
use tokio::sync::mpsc;

const NOW: u64 = 10_000;

fn alice_identity() -> CallMembershipIdentity {
    CallMembershipIdentity {
        user_id: "@alice:example.org".to_owned(),
        device_id: "AAAAAAA".to_owned(),
        member_id: "@alice:example.org:AAAAAAA".to_owned(),
    }
}

fn make_session(client: Arc<MockClient>, config: RtcCallSessionConfig) -> RtcCallSession {
    RtcCallSession::new(
        client,
        RtcRoom {
            room_id: common::room_id(),
            version: "default".to_owned(),
        },
        alice_identity(),
        config,
    )
    .unwrap()
}

fn member_event(event_id: &str, sender: &str, content: JsonValue) -> MemberStateEvent {
    let device_id = content
        .get("device_id")
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_owned();
    MemberStateEvent {
        event_id: event_id.to_owned(),
        sender: sender.to_owned(),
        origin_server_ts: 1000,
        state_key: format!("_{sender}_{device_id}"),
        content,
    }
}

fn template_member(sender: &str, device_id: &str) -> MemberStateEvent {
    let mut content = session_membership_template();
    content
        .as_object_mut()
        .unwrap()
        .insert("device_id".to_owned(), json!(device_id));
    member_event(&format!("$ev{sender}{device_id}"), sender, content)
}

/// The membership state event the manager sent for us, as a room state
/// event.
fn own_member_event(client: &MockClient) -> MemberStateEvent {
    let content = client
        .calls_of(Method::SendStateEvent)
        .iter()
        .find_map(|call| match call {
            RecordedCall::SendStateEvent { content, .. } if content.get("device_id").is_some() => {
                Some(content.clone())
            }
            _ => None,
        })
        .expect("a membership state event was sent");
    member_event("$own:e.org", "@alice:example.org", content)
}

fn drain(rx: &mut mpsc::UnboundedReceiver<RtcCallSessionEvent>) -> Vec<RtcCallSessionEvent> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

fn notification_calls(client: &MockClient) -> Vec<(String, JsonValue)> {
    client
        .calls_of(Method::SendEvent)
        .into_iter()
        .map(|call| match call {
            RecordedCall::SendEvent {
                event_type,
                content,
                ..
            } => (event_type, content),
            _ => unreachable!(),
        })
        .collect()
}

// joining

#[tokio::test(start_paused = true)]
async fn starts_un_joined() {
    let client = MockClient::new();
    let session = make_session(client, RtcCallSessionConfig::default());
    assert!(!session.is_joined());
}

#[tokio::test(start_paused = true)]
async fn shows_joined_once_join_is_called() {
    let client = MockClient::new();
    let session = make_session(client, RtcCallSessionConfig::default());
    session.join_rtc_session(vec![focus()]);
    assert!(session.is_joined());
    session.leave_rtc_session(None).await;
}

#[tokio::test(start_paused = true)]
async fn sends_a_notification_when_starting_a_call_and_emit_did_send_call_notification() {
    let client = MockClient::new();
    let session = make_session(
        client.clone(),
        RtcCallSessionConfig {
            notification_type: Some("ring".to_owned()),
            ..Default::default()
        },
    );
    let mut events = session.subscribe();

    session.join_rtc_session(vec![focus()]);
    advance(1).await;

    // Simulate the sync echoing back our own membership.
    session.on_room_state_update(&[own_member_event(&client)], |_| true, NOW);
    settle().await;

    let notifications = notification_calls(&client);
    assert_eq!(notifications.len(), 1);
    let (event_type, content) = &notifications[0];
    assert_eq!(event_type, "org.matrix.msc4075.rtc.notification");
    assert_eq!(content.get("notification_type"), Some(&json!("ring")));
    assert_eq!(
        content.get("m.mentions"),
        Some(&json!({ "user_ids": [], "room": true }))
    );
    assert_eq!(
        content.get("m.relates_to"),
        Some(&json!({ "event_id": "$own:e.org", "rel_type": "m.reference" }))
    );
    assert_eq!(content.get("lifetime"), Some(&json!(30_000)));
    assert!(content.get("sender_ts").is_some_and(JsonValue::is_number));
    assert!(content.get("m.call.intent").is_none());

    // And ensure we emitted DidSendCallNotification with both payloads.
    let did_send = drain(&mut events)
        .into_iter()
        .find_map(|event| match event {
            RtcCallSessionEvent::DidSendCallNotification(content) => Some(content),
            _ => None,
        });
    let did_send = did_send.expect("DidSendCallNotification was emitted");
    assert_eq!(did_send.get("event_id"), Some(&json!("$id:e.org")));
    assert_eq!(did_send.get("notification_type"), Some(&json!("ring")));

    session.leave_rtc_session(None).await;
}

#[tokio::test(start_paused = true)]
async fn sends_a_notification_with_an_intent_when_starting_a_call_and_emits_did_send_call_notification()
 {
    let client = MockClient::new();
    let session = make_session(
        client.clone(),
        RtcCallSessionConfig {
            notification_type: Some("ring".to_owned()),
            membership: MembershipConfig {
                call_intent: Some("audio".to_owned()),
                ..Default::default()
            },
            ..Default::default()
        },
    );

    session.join_rtc_session(vec![focus()]);
    advance(1).await;

    // The sent membership contains `m.call.intent: audio`, which is what
    // triggers the intent on the notification event.
    session.on_room_state_update(&[own_member_event(&client)], |_| true, NOW);
    settle().await;

    assert_eq!(
        session.get_consensus_call_intent().as_deref(),
        Some("audio")
    );

    let notifications = notification_calls(&client);
    assert_eq!(notifications.len(), 1);
    let (_, content) = &notifications[0];
    assert_eq!(content.get("notification_type"), Some(&json!("ring")));
    assert_eq!(content.get("m.call.intent"), Some(&json!("audio")));

    session.leave_rtc_session(None).await;
}

#[tokio::test(start_paused = true)]
async fn doesnt_send_a_notification_when_joining_an_existing_call() {
    let client = MockClient::new();
    let session = make_session(
        client.clone(),
        RtcCallSessionConfig {
            notification_type: Some("ring".to_owned()),
            ..Default::default()
        },
    );

    // Add another member to the call so that it is considered an existing
    // call.
    session.on_room_state_update(
        &[template_member("@mock:user.example", "AAAAAAA")],
        |_| true,
        NOW,
    );
    settle().await;

    session.join_rtc_session(vec![focus()]);
    advance(1).await;
    session.on_room_state_update(
        &[
            template_member("@mock:user.example", "AAAAAAA"),
            own_member_event(&client),
        ],
        |_| true,
        NOW,
    );
    settle().await;

    // Check we sent our join event, but no notification event.
    assert!(client.count(Method::SendStateEvent) >= 1);
    assert_eq!(client.count(Method::SendEvent), 0);

    session.leave_rtc_session(None).await;
}

#[tokio::test(start_paused = true)]
async fn doesnt_send_a_notification_when_someone_else_starts_the_call_faster_than_us() {
    let client = MockClient::new();
    let session = make_session(
        client.clone(),
        RtcCallSessionConfig {
            notification_type: Some("ring".to_owned()),
            ..Default::default()
        },
    );

    session.join_rtc_session(vec![focus()]);
    advance(1).await;

    // Simulate a race condition in which we receive a state event from
    // someone else, starting the call before our own state event arrived.
    session.on_room_state_update(
        &[template_member("@mock:user.example", "AAAAAAA")],
        |_| true,
        NOW,
    );
    settle().await;
    session.on_room_state_update(
        &[
            template_member("@mock:user.example", "AAAAAAA"),
            own_member_event(&client),
        ],
        |_| true,
        NOW,
    );
    settle().await;

    // Check we sent our join event, but no notification event. The
    // responsibility to send a notification lies with the other participant
    // who won the race.
    assert!(client.count(Method::SendStateEvent) >= 1);
    assert_eq!(client.count(Method::SendEvent), 0);

    session.leave_rtc_session(None).await;
}

// onMembershipsChanged

#[tokio::test(start_paused = true)]
async fn only_emit_if_membership_changes() {
    let client = MockClient::new();
    let session = make_session(client, RtcCallSessionConfig::default());

    session.on_room_state_update(
        &[template_member("@mock:user.example", "AAAAAAA")],
        |_| true,
        NOW,
    );
    let mut events = session.subscribe();

    // No change -> no emission (event ids differ, data is equal).
    session.on_room_state_update(
        &[template_member("@mock:user.example", "AAAAAAA")],
        |_| true,
        NOW,
    );
    assert!(
        !drain(&mut events)
            .iter()
            .any(|event| matches!(event, RtcCallSessionEvent::MembershipsChanged { .. }))
    );

    // Change -> emission.
    session.on_room_state_update(&[], |_| true, NOW);
    assert!(
        drain(&mut events)
            .iter()
            .any(|event| matches!(event, RtcCallSessionEvent::MembershipsChanged { .. }))
    );
}

// key management

#[tokio::test(start_paused = true)]
async fn provides_encryption_keys_for_memberships() {
    let client = MockClient::new();
    let session = make_session(client.clone(), RtcCallSessionConfig::default());

    session.join_rtc_session(vec![focus()]);
    advance(1).await;

    session.on_room_state_update(
        &[
            template_member("@bob:user.example", "BBBBBB"),
            own_member_event(&client),
        ],
        |_| true,
        NOW,
    );
    settle().await;

    let calls = client.calls_of(Method::EncryptAndSendToDevice);
    assert_eq!(calls.len(), 1);
    let RecordedCall::EncryptAndSendToDevice {
        event_type,
        targets,
        ..
    } = &calls[0]
    else {
        unreachable!()
    };
    assert_eq!(event_type, "io.element.call.encryption_keys");
    assert_eq!(
        targets,
        &[ToDeviceTarget {
            user_id: "@bob:user.example".to_owned(),
            device_id: "BBBBBB".to_owned(),
        }]
    );
    assert_eq!(session.statistics().encryption_keys_sent, 1);

    session.leave_rtc_session(None).await;
}

// read status

#[tokio::test(start_paused = true)]
async fn returns_the_correct_probably_left_status() {
    let client = MockClient::new();
    let session = make_session(client, RtcCallSessionConfig::default());
    assert_eq!(session.probably_left(), None);

    session.join_rtc_session(vec![focus()]);
    assert_eq!(session.probably_left(), Some(false));

    session.leave_rtc_session(None).await;
}

#[tokio::test(start_paused = true)]
async fn returns_membership_status_once_join_rtc_session_got_called() {
    let client = MockClient::new();
    let session = make_session(client, RtcCallSessionConfig::default());
    assert_eq!(session.membership_status(), None);

    session.join_rtc_session(vec![focus()]);
    assert_eq!(session.membership_status(), Some(Status::Connecting));

    session.leave_rtc_session(None).await;
}

#[tokio::test(start_paused = true)]
async fn reemits_membership_manager_events() {
    let client = MockClient::new();
    let session = make_session(client, RtcCallSessionConfig::default());
    let mut events = session.subscribe();

    session.join_rtc_session(vec![focus()]);
    advance(1).await;

    let events = drain(&mut events);
    assert!(events.iter().any(|event| matches!(
        event,
        RtcCallSessionEvent::StatusChanged {
            previous: Status::Disconnected,
            current: Status::Connecting,
        }
    )));
    assert!(
        events.iter().any(
            |event| matches!(event, RtcCallSessionEvent::DelayIdChanged(Some(id)) if id == "id")
        )
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, RtcCallSessionEvent::JoinStateChanged(true)))
    );

    session.leave_rtc_session(None).await;
}

// leaving

#[tokio::test(start_paused = true)]
async fn leave_does_nothing_when_not_joined_and_leaves_after_join() {
    let client = MockClient::new();
    let session = make_session(client, RtcCallSessionConfig::default());

    assert!(!session.leave_rtc_session(None).await);

    session.join_rtc_session(vec![focus()]);
    advance(1).await;
    assert!(session.leave_rtc_session(None).await);
    assert!(!session.is_joined());
}

// active focus

#[tokio::test(start_paused = true)]
async fn resolves_the_active_focus_from_the_oldest_membership() {
    let client = MockClient::new();
    let session = make_session(client, RtcCallSessionConfig::default());

    let mut content = session_membership_template();
    content.as_object_mut().unwrap().insert(
        "foci_preferred".to_owned(),
        json!([{
            "type": "livekit",
            "livekit_service_url": "https://active.url",
            "livekit_alias": "!active:active.url",
        }]),
    );
    session.on_room_state_update(
        &[member_event("$oldest:e.org", "@mock:user.example", content)],
        |_| true,
        NOW,
    );

    let active_focus = session.get_active_focus().unwrap();
    assert_eq!(
        serde_json::to_value(&active_focus).unwrap(),
        json!({
            "type": "livekit",
            "livekit_service_url": "https://active.url",
            "livekit_alias": "!active:active.url",
        })
    );
}

// session manager: session start/end detection

#[test]
fn fires_event_when_session_starts() {
    let mut tracker = SessionActivityTracker::new();
    assert_eq!(
        tracker.update("!room1:example.org", 1),
        Some(SessionActivity::Started("!room1:example.org".to_owned()))
    );
    // Still active: no new event.
    assert_eq!(tracker.update("!room1:example.org", 2), None);
}

#[test]
fn fires_event_when_session_ends() {
    let mut tracker = SessionActivityTracker::new();
    assert_eq!(
        tracker.update("!room1:example.org", 1),
        Some(SessionActivity::Started("!room1:example.org".to_owned()))
    );
    assert_eq!(
        tracker.update("!room1:example.org", 0),
        Some(SessionActivity::Ended("!room1:example.org".to_owned()))
    );
    // Already inactive: no new event.
    assert_eq!(tracker.update("!room1:example.org", 0), None);
    // A room that never had members never fires an end event.
    assert_eq!(tracker.update("!room2:example.org", 0), None);
}
