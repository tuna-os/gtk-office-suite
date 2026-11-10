// SPDX-License-Identifier: GPL-3.0-or-later

//! Port of `matrix-js-sdk`'s `MembershipManager.spec.ts` (the state event
//! based manager; the sticky event manager is out of scope).

mod common;

use std::time::Duration;

use common::{
    Method, MockClient, RecordedCall, advance, drain_events, focus, make_manager,
    make_manager_for_slot, mock_call_membership, session_membership_template, settle,
};
use mandelbrot_matrixrtc::{
    ClientError, MembershipConfig, MembershipManagerEvent, SlotDescription, Status,
};
use serde_json::{Value as JsonValue, json};

const CALL_MEMBER_EVENT_TYPE: &str = "org.matrix.msc3401.call.member";
const ALICE_STATE_KEY: &str = "_@alice:example.org_AAAAAAA_m.call";

fn expected_join_content() -> JsonValue {
    serde_json::from_str(common::EXPECTED_JOIN_CONTENT).unwrap()
}

/// Strip `created_ts` (a timestamp) before comparing against fixtures.
fn without_created_ts(mut content: JsonValue) -> JsonValue {
    if let Some(object) = content.as_object_mut() {
        object.remove("created_ts");
    }
    content
}

fn state_event_calls(client: &MockClient) -> Vec<(String, String, String, JsonValue)> {
    client
        .calls_of(Method::SendStateEvent)
        .into_iter()
        .map(|call| match call {
            RecordedCall::SendStateEvent {
                room_id,
                event_type,
                state_key,
                content,
            } => (room_id, event_type, state_key, content),
            _ => unreachable!(),
        })
        .collect()
}

fn delayed_state_event_calls(client: &MockClient) -> Vec<(String, u64, String, String, JsonValue)> {
    client
        .calls_of(Method::SendDelayedStateEvent)
        .into_iter()
        .map(|call| match call {
            RecordedCall::SendDelayedStateEvent {
                room_id,
                delay_ms,
                event_type,
                state_key,
                content,
            } => (room_id, delay_ms, event_type, state_key, content),
            _ => unreachable!(),
        })
        .collect()
}

// isActivated()

#[tokio::test(start_paused = true)]
async fn is_activated_defaults_to_false() {
    let client = MockClient::new();
    let manager = make_manager(MembershipConfig::default(), client);
    assert!(!manager.is_activated());
}

#[tokio::test(start_paused = true)]
async fn is_activated_returns_true_after_join() {
    let client = MockClient::new();
    let manager = make_manager(MembershipConfig::default(), client);
    manager.join(vec![]);
    assert!(manager.is_activated());
}

// join(): sends a membership event

#[tokio::test(start_paused = true)]
async fn sends_a_membership_event_and_schedules_delayed_leave_when_joining_a_call() {
    let client = MockClient::new();
    let restart_handle = client.pending(Method::RestartDelayedEvent);

    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.join(vec![focus()]);
    settle().await;

    let state_calls = state_event_calls(&client);
    assert_eq!(state_calls.len(), 1);
    let (room_id, event_type, state_key, content) = &state_calls[0];
    assert_eq!(room_id, "!room:example.org");
    assert_eq!(event_type, CALL_MEMBER_EVENT_TYPE);
    assert_eq!(state_key, ALICE_STATE_KEY);
    // The wire format must match the js-sdk field for field (ignoring
    // timestamps).
    assert_eq!(without_created_ts(content.clone()), expected_join_content());

    restart_handle.send(Ok(json!({}))).unwrap();
    settle().await;

    let delayed_calls = delayed_state_event_calls(&client);
    assert_eq!(delayed_calls.len(), 1);
    let (room_id, delay_ms, event_type, state_key, content) = &delayed_calls[0];
    assert_eq!(room_id, "!room:example.org");
    assert_eq!(*delay_ms, 8000);
    assert_eq!(event_type, CALL_MEMBER_EVENT_TYPE);
    assert_eq!(state_key, ALICE_STATE_KEY);
    assert_eq!(content, &json!({}));
}

#[tokio::test(start_paused = true)]
async fn sends_correct_call_id_and_state_key_when_using_non_empty_string() {
    // Not using the empty string -> ROOM hack. See INFO_SLOT_ID_LEGACY_CASE.
    let client = MockClient::new();
    let restart_handle = client.pending(Method::RestartDelayedEvent);

    let custom_slot = SlotDescription {
        application: "m.call".to_owned(),
        id: "custom".to_owned(),
    };
    let manager = make_manager_for_slot(
        MembershipConfig::default(),
        client.clone(),
        custom_slot,
        "default",
    );
    manager.join(vec![focus()]);
    settle().await;

    let state_calls = state_event_calls(&client);
    assert_eq!(state_calls.len(), 1);
    let (_, _, state_key, content) = &state_calls[0];
    assert_eq!(state_key, "_@alice:example.org_AAAAAAA_m.callcustom");
    assert_eq!(content.get("call_id"), Some(&json!("custom")));

    restart_handle.send(Ok(json!({}))).unwrap();
    settle().await;

    let delayed_calls = delayed_state_event_calls(&client);
    assert_eq!(delayed_calls.len(), 1);
    assert_eq!(
        delayed_calls[0].3,
        "_@alice:example.org_AAAAAAA_m.callcustom"
    );
}

#[tokio::test(start_paused = true)]
async fn reschedules_delayed_leave_event_if_sending_state_cancels_it() {
    let client = MockClient::new();
    client.enqueue(Method::RestartDelayedEvent, Err(ClientError::not_found()));

    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.join(vec![focus()]);
    advance(1).await;

    // Once for the initial event and once because of the M_NOT_FOUND.
    assert_eq!(client.count(Method::SendDelayedStateEvent), 2);
}

/// Port of `testJoin` for both the prefixed and the user-owned state key
/// cases. This covers: a delayed event send with a too-long timeout
/// (`M_MAX_DELAY_EXCEEDED`), a rate limit while sending the delayed event,
/// and a rate limit while sending the membership state event.
async fn test_join(use_owned_state_events: bool) {
    let client = MockClient::new();
    let room_version = if use_owned_state_events {
        "org.matrix.msc3757.default"
    } else {
        "default"
    };
    let user_state_key = if use_owned_state_events {
        "@alice:example.org_AAAAAAA_m.call"
    } else {
        ALICE_STATE_KEY
    };

    // Preparing the delayed disconnect should handle the delay being too
    // long.
    client.enqueue(
        Method::SendDelayedStateEvent,
        Err(ClientError::max_delay_exceeded(Duration::from_millis(7500))),
    );
    // Preparing the delayed disconnect should handle rate limiting.
    client.enqueue(
        Method::SendDelayedStateEvent,
        Err(ClientError::rate_limited(None)),
    );
    // Setting the membership state should handle rate limiting (also with a
    // retry-after value).
    client.enqueue(
        Method::SendStateEvent,
        Err(ClientError::rate_limited(Some(Duration::from_secs(1)))),
    );

    let manager = make_manager_for_slot(
        MembershipConfig {
            delayed_leave_event_delay_ms: 9000,
            ..Default::default()
        },
        client.clone(),
        SlotDescription::room_call(),
        room_version,
    );
    manager.join(vec![focus()]);
    settle().await;

    let delayed_calls = delayed_state_event_calls(&client);
    assert_eq!(delayed_calls.len(), 2);
    assert_eq!(delayed_calls[0].1, 9000);
    assert_eq!(delayed_calls[0].3, user_state_key);
    assert_eq!(delayed_calls[1].1, 7500);
    assert_eq!(delayed_calls[1].3, user_state_key);

    // Wait out the rate limit of the delayed event.
    advance(5000).await;
    // Wait out the rate limit of the state event.
    advance(1000).await;

    let state_calls = state_event_calls(&client);
    let (_, event_type, state_key, content) = state_calls.last().unwrap();
    assert_eq!(event_type, CALL_MEMBER_EVENT_TYPE);
    assert_eq!(state_key, user_state_key);
    assert_eq!(without_created_ts(content.clone()), expected_join_content());

    // Should have prepared the heartbeat to keep delaying the leave event
    // while still connected.
    assert_eq!(client.count(Method::RestartDelayedEvent), 1);

    // Should update the delayed disconnect.
    advance(5000).await;
    assert_eq!(client.count(Method::RestartDelayedEvent), 2);
}

#[tokio::test(start_paused = true)]
async fn sends_a_membership_event_after_rate_limits_during_delayed_event_setup_when_joining_a_call()
{
    test_join(false).await;
}

#[tokio::test(start_paused = true)]
async fn does_not_prefix_the_state_key_with_underscore_for_rooms_with_user_owned_state_events() {
    test_join(true).await;
}

// join(): delayed leave event

#[tokio::test(start_paused = true)]
async fn does_not_try_again_to_schedule_a_delayed_leave_event_if_not_supported() {
    let client = MockClient::new();
    client.set_default(
        Method::SendDelayedStateEvent,
        Err(ClientError::UnsupportedDelayedEventsEndpoint),
    );
    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.join(vec![focus()]);
    settle().await;

    // One initial attempt plus one post-join delayed event check; after the
    // endpoint reported unsupported there, no further delayed event attempts
    // are made.
    assert_eq!(client.count(Method::SendDelayedStateEvent), 2);
    advance(20_000).await;
    assert_eq!(client.count(Method::SendDelayedStateEvent), 2);
}

#[tokio::test(start_paused = true)]
async fn does_try_to_schedule_a_delayed_leave_event_again_if_rate_limited() {
    let client = MockClient::new();
    client.enqueue(
        Method::SendDelayedStateEvent,
        Err(ClientError::Http { status: 429 }),
    );
    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.join(vec![focus()]);
    settle().await;
    assert_eq!(client.count(Method::SendDelayedStateEvent), 1);

    advance(5000).await;
    assert_eq!(client.count(Method::SendDelayedStateEvent), 2);
}

#[tokio::test(start_paused = true)]
async fn uses_delayed_leave_event_delay_ms_from_config() {
    let client = MockClient::new();
    let manager = make_manager(
        MembershipConfig {
            delayed_leave_event_delay_ms: 123_456,
            ..Default::default()
        },
        client.clone(),
    );
    manager.join(vec![focus()]);
    settle().await;

    let delayed_calls = delayed_state_event_calls(&client);
    let (room_id, delay_ms, event_type, state_key, content) = &delayed_calls[0];
    assert_eq!(room_id, "!room:example.org");
    assert_eq!(*delay_ms, 123_456);
    assert_eq!(event_type, CALL_MEMBER_EVENT_TYPE);
    assert_eq!(state_key, ALICE_STATE_KEY);
    assert_eq!(content, &json!({}));
}

#[tokio::test(start_paused = true)]
async fn rejoins_if_delayed_event_is_not_found_404() {
    const RESTART_DELAY: u64 = 15_000;
    let client = MockClient::new();
    let manager = make_manager(
        MembershipConfig {
            delayed_leave_event_restart_ms: RESTART_DELAY,
            ..Default::default()
        },
        client.clone(),
    );

    manager.join(vec![focus()]);
    assert_eq!(manager.status(), Status::Connecting);
    settle().await;
    assert_eq!(client.count(Method::SendStateEvent), 1);
    assert_eq!(manager.status(), Status::Connected);

    // Simulate that the delayed event activated and caused the user to
    // leave, with a race between the sync informing us about the leave and
    // the "not found" of the delayed event restart.
    client.enqueue(Method::RestartDelayedEvent, Err(ClientError::not_found()));
    let delayed_handle = client.pending(Method::SendDelayedStateEvent);
    advance(RESTART_DELAY).await;

    // First simulate the sync, then resolve sending the delayed event.
    manager.on_rtc_session_member_update(&[mock_call_membership(
        session_membership_template(),
        "@mock:user.example",
        1000,
    )]);
    delayed_handle
        .send(Ok(json!({ "delay_id": "id" })))
        .unwrap();
    advance(1).await;

    assert_eq!(client.count(Method::SendStateEvent), 2);
}

#[tokio::test(start_paused = true)]
async fn uses_membership_event_expiry_ms_from_config() {
    let client = MockClient::new();
    let manager = make_manager(
        MembershipConfig {
            membership_event_expiry_ms: 1_234_567,
            ..Default::default()
        },
        client.clone(),
    );
    manager.join(vec![focus()]);
    settle().await;

    let state_calls = state_event_calls(&client);
    let (_, _, state_key, content) = &state_calls[0];
    assert_eq!(state_key, ALICE_STATE_KEY);
    let mut expected = expected_join_content();
    expected
        .as_object_mut()
        .unwrap()
        .insert("expires".to_owned(), json!(1_234_567));
    assert_eq!(without_created_ts(content.clone()), expected);
}

#[tokio::test(start_paused = true)]
async fn does_nothing_if_join_called_when_already_joined() {
    let client = MockClient::new();
    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.join(vec![focus()]);
    settle().await;
    assert_eq!(client.count(Method::SendStateEvent), 1);

    manager.join(vec![focus()]);
    settle().await;
    assert_eq!(client.count(Method::SendStateEvent), 1);
}

// leave()

#[tokio::test(start_paused = true)]
async fn resolves_delayed_leave_event_when_leave_is_called() {
    let client = MockClient::new();
    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.join(vec![focus()]);
    advance(1).await;

    assert!(manager.leave(None).await);

    let send_calls = client.calls_of(Method::SendDelayedEvent);
    assert!(matches!(
        send_calls.last().unwrap(),
        RecordedCall::UpdateDelayedEvent { delay_id, .. } if delay_id == "id"
    ));
    assert!(client.count(Method::SendStateEvent) >= 1);
    assert_eq!(manager.delay_id(), None);
}

#[tokio::test(start_paused = true)]
async fn send_leave_event_when_leave_is_called_and_resolving_delayed_leave_fails_unknown_error() {
    let client = MockClient::new();
    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.join(vec![focus()]);
    advance(1).await;

    client.set_default(
        Method::SendDelayedEvent,
        Err(ClientError::Other("unknown".to_owned())),
    );
    assert!(manager.leave(None).await);

    // We send a normal leave event since resolving the delayed event failed.
    let state_calls = state_event_calls(&client);
    let (room_id, event_type, state_key, content) = state_calls.last().unwrap();
    assert_eq!(room_id, "!room:example.org");
    assert_eq!(event_type, CALL_MEMBER_EVENT_TYPE);
    assert_eq!(state_key, ALICE_STATE_KEY);
    assert_eq!(content, &json!({}));
    // On an unknown error we do not reset the delay ID: the delayed event
    // might still be around and we track it.
    assert!(manager.delay_id().is_some());
}

#[tokio::test(start_paused = true)]
async fn send_leave_event_when_leave_is_called_and_resolving_delayed_leave_fails_not_found_error() {
    let client = MockClient::new();
    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.join(vec![focus()]);
    advance(1).await;

    client.set_default(Method::SendDelayedEvent, Err(ClientError::not_found()));
    assert!(manager.leave(None).await);

    let state_calls = state_event_calls(&client);
    let (_, _, state_key, content) = state_calls.last().unwrap();
    assert_eq!(state_key, ALICE_STATE_KEY);
    assert_eq!(content, &json!({}));
    assert_eq!(manager.delay_id(), None);
}

#[tokio::test(start_paused = true)]
async fn leave_does_nothing_if_not_joined() {
    let client = MockClient::new();
    let manager = make_manager(MembershipConfig::default(), client.clone());
    assert!(manager.leave(None).await);
    assert_eq!(client.count(Method::SendDelayedStateEvent), 0);
    assert_eq!(client.count(Method::SendStateEvent), 0);
}

// onRTCSessionMemberUpdate()

#[tokio::test(start_paused = true)]
async fn on_member_update_does_nothing_if_not_joined() {
    let client = MockClient::new();
    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.on_rtc_session_member_update(&[mock_call_membership(
        session_membership_template(),
        "@mock:user.example",
        1000,
    )]);
    advance(1).await;
    assert!(client.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn does_nothing_if_own_membership_still_present() {
    let client = MockClient::new();
    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.join(vec![focus()]);
    advance(1).await;

    let my_membership_content = state_event_calls(&client)[0].3.clone();
    client.clear_calls();

    manager.on_rtc_session_member_update(&[
        mock_call_membership(session_membership_template(), "@mock:user.example", 1000),
        mock_call_membership(my_membership_content, "@alice:example.org", 1000),
    ]);
    advance(1).await;

    assert!(client.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn recreates_membership_if_it_is_missing() {
    let client = MockClient::new();
    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.join(vec![focus()]);
    advance(1).await;
    client.clear_calls();

    // Our own membership is removed.
    manager.on_rtc_session_member_update(&[mock_call_membership(
        session_membership_template(),
        "@mock:user.example",
        1000,
    )]);
    advance(1).await;

    assert!(client.count(Method::SendStateEvent) >= 1);
    assert!(client.count(Method::SendDelayedStateEvent) >= 1);
    assert!(client.count(Method::RestartDelayedEvent) >= 1);
}

#[tokio::test(start_paused = true)]
async fn updates_the_update_expiry_entry_in_the_action_scheduler() {
    let client = MockClient::new();
    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.join(vec![focus()]);
    advance(1).await;
    client.clear_calls();

    client.enqueue(Method::RestartDelayedEvent, Err(ClientError::not_found()));
    let delayed_handle = client.pending(Method::SendDelayedStateEvent);
    advance(10_000).await;

    manager.on_rtc_session_member_update(&[mock_call_membership(
        session_membership_template(),
        "@mock:user.example",
        1000,
    )]);
    delayed_handle
        .send(Ok(json!({ "delay_id": "id" })))
        .unwrap();
    advance(10_000).await;

    assert!(client.count(Method::SendStateEvent) >= 1);
    assert!(client.count(Method::SendDelayedStateEvent) >= 1);
    assert!(client.count(Method::RestartDelayedEvent) >= 1);
    assert_eq!(manager.status(), Status::Connected);
}

// Background timers

#[tokio::test(start_paused = true)]
async fn sends_only_one_keep_alive_for_delayed_leave_event_per_restart_ms() {
    let client = MockClient::new();
    let manager = make_manager_for_slot(
        MembershipConfig {
            delayed_leave_event_restart_ms: 10_000,
            delayed_leave_event_delay_ms: 30_000,
            ..Default::default()
        },
        client.clone(),
        SlotDescription {
            application: "m.call".to_owned(),
            id: String::new(),
        },
        "default",
    );
    manager.join(vec![focus()]);
    advance(1).await;
    assert_eq!(client.count(Method::SendDelayedStateEvent), 1);

    // The first restart is from checking if the server deleted the delayed
    // event, so it does not need any time to pass.
    assert_eq!(client.count(Method::RestartDelayedEvent), 1);

    for i in 2..=12 {
        advance(10_000).await;
        assert_eq!(client.count(Method::RestartDelayedEvent), i);
    }
}

async fn test_expires(expire: u64, headroom: Option<u64>) {
    let client = MockClient::new();
    let manager = make_manager_for_slot(
        MembershipConfig {
            membership_event_expiry_ms: expire,
            membership_event_expiry_headroom_ms: headroom.unwrap_or(5000),
            ..Default::default()
        },
        client.clone(),
        SlotDescription {
            application: "m.call".to_owned(),
            id: String::new(),
        },
        "default",
    );
    manager.join(vec![focus()]);
    settle().await;

    assert_eq!(client.count(Method::SendStateEvent), 1);
    let sent = &state_event_calls(&client)[0].3;
    assert_eq!(sent.get("expires"), Some(&json!(expire)));

    for i in 2..=12_u64 {
        advance(expire).await;
        assert_eq!(
            client.count(Method::SendStateEvent),
            usize::try_from(i).unwrap()
        );
        let sent = state_event_calls(&client).last().unwrap().3.clone();
        assert_eq!(sent.get("expires"), Some(&json!(expire * i)));
    }
}

#[tokio::test(start_paused = true)]
async fn extends_expires_when_call_still_active() {
    test_expires(10_000, None).await;
}

#[tokio::test(start_paused = true)]
async fn extends_expires_using_headroom_configuration() {
    test_expires(10_000, Some(1_000)).await;
}

// Status updates

#[tokio::test(start_paused = true)]
async fn starts_disconnected() {
    let client = MockClient::new();
    let manager = make_manager(MembershipConfig::default(), client);
    assert_eq!(manager.status(), Status::Disconnected);
}

#[tokio::test(start_paused = true)]
async fn emits_connecting_and_connected_after_join() {
    let client = MockClient::new();
    let delayed_handle = client.pending(Method::SendDelayedStateEvent);
    let state_handle = client.pending(Method::SendStateEvent);

    let manager = make_manager(MembershipConfig::default(), client.clone());
    assert_eq!(manager.status(), Status::Disconnected);
    let mut events = manager.subscribe();

    manager.join(vec![focus()]);
    assert_eq!(manager.status(), Status::Connecting);

    delayed_handle
        .send(Ok(json!({ "delay_id": "id" })))
        .unwrap();
    advance(1).await;
    assert!(drain_events(&mut events).iter().any(|event| matches!(
        event,
        MembershipManagerEvent::StatusChanged {
            previous: Status::Disconnected,
            current: Status::Connecting,
        }
    )));

    state_handle
        .send(Ok(json!({ "event_id": "$id:e.org" })))
        .unwrap();
    advance(1).await;
    assert!(drain_events(&mut events).iter().any(|event| matches!(
        event,
        MembershipManagerEvent::StatusChanged {
            previous: Status::Connecting,
            current: Status::Connected,
        }
    )));
}

#[tokio::test(start_paused = true)]
async fn emits_disconnecting_and_disconnected_after_leave() {
    let client = MockClient::new();
    let manager = make_manager(MembershipConfig::default(), client.clone());
    let mut events = manager.subscribe();
    manager.join(vec![focus()]);
    advance(1).await;

    assert!(manager.leave(None).await);
    let events = drain_events(&mut events);
    assert!(events.iter().any(|event| matches!(
        event,
        MembershipManagerEvent::StatusChanged {
            previous: Status::Connected,
            current: Status::Disconnecting,
        }
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        MembershipManagerEvent::StatusChanged {
            previous: Status::Disconnecting,
            current: Status::Disconnected,
        }
    )));
}

// Server error handling

#[tokio::test(start_paused = true)]
async fn sends_retry_if_call_membership_event_is_still_valid_at_time_of_retry() {
    let client = MockClient::new();
    client.enqueue(
        Method::SendDelayedStateEvent,
        Err(ClientError::rate_limited(Some(Duration::from_secs(1)))),
    );
    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.join(vec![focus()]);
    settle().await;
    assert_eq!(client.count(Method::SendDelayedStateEvent), 1);

    advance(1000).await;
    assert_eq!(client.count(Method::SendDelayedStateEvent), 2);
}

#[tokio::test(start_paused = true)]
async fn abandons_retry_loop_and_sends_new_own_membership_if_not_present_anymore() {
    let client = MockClient::new();
    client.set_default(
        Method::SendDelayedStateEvent,
        Err(ClientError::rate_limited(Some(Duration::from_secs(1)))),
    );
    let manager = make_manager(MembershipConfig::default(), client.clone());
    // Should call the delayed event endpoint but not send the state event
    // because of the rate limit error.
    manager.join(vec![focus()]);
    advance(1).await;

    assert_eq!(client.count(Method::SendDelayedStateEvent), 1);
    client.set_default(
        Method::SendDelayedStateEvent,
        Ok(json!({ "delay_id": "id" })),
    );

    // The membership is no longer present on the homeserver.
    manager.on_rtc_session_member_update(&[]);
    advance(1000).await;

    // We should send the first own membership and a new delayed event after
    // the rate limit timeout.
    assert_eq!(client.count(Method::SendDelayedStateEvent), 2);
    assert_eq!(client.count(Method::SendStateEvent), 1);
}

#[tokio::test(start_paused = true)]
async fn abandons_retry_loop_if_leave_was_called_before_sending_state_event() {
    let client = MockClient::new();
    client.enqueue(
        Method::SendDelayedStateEvent,
        Err(ClientError::rate_limited(Some(Duration::from_secs(1)))),
    );
    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.join(vec![focus()]);
    advance(1).await;
    assert_eq!(client.count(Method::SendDelayedStateEvent), 1);

    // The user terminated the call locally.
    assert!(manager.leave(None).await);

    advance(1000).await;

    // No new events should have been sent.
    assert_eq!(client.count(Method::SendDelayedStateEvent), 1);
}

#[tokio::test(start_paused = true)]
async fn resends_the_initial_check_delayed_update_event() {
    let client = MockClient::new();
    client.set_default(
        Method::RestartDelayedEvent,
        Err(ClientError::rate_limited(Some(Duration::from_secs(1)))),
    );
    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.join(vec![focus()]);

    // Hit the rate limit.
    advance(1).await;
    assert_eq!(client.count(Method::RestartDelayedEvent), 1);

    // Hit the second rate limit.
    advance(1000).await;
    assert_eq!(client.count(Method::RestartDelayedEvent), 2);

    // Set up the resolution.
    client.set_default(Method::RestartDelayedEvent, Ok(json!({})));
    advance(1000).await;

    assert_eq!(client.count(Method::RestartDelayedEvent), 3);
    assert_eq!(client.count(Method::SendStateEvent), 1);
}

// Unrecoverable errors

#[tokio::test(start_paused = true)]
async fn throws_when_reaching_maximum_number_of_retries_for_initial_delayed_event_creation() {
    let client = MockClient::new();
    client.set_default(
        Method::SendDelayedStateEvent,
        Err(ClientError::rate_limited(Some(Duration::from_secs(2)))),
    );
    let manager = make_manager(MembershipConfig::default(), client.clone());
    let mut events = manager.subscribe();
    manager.join(vec![focus()]);
    settle().await;

    for _ in 0..10 {
        advance(2000).await;
    }

    assert!(
        drain_events(&mut events)
            .iter()
            .any(|event| matches!(event, MembershipManagerEvent::Error(_)))
    );
}

#[tokio::test(start_paused = true)]
async fn throws_when_reaching_maximum_number_of_retries() {
    let client = MockClient::new();
    client.set_default(
        Method::RestartDelayedEvent,
        Err(ClientError::rate_limited(Some(Duration::from_secs(1)))),
    );
    let manager = make_manager(MembershipConfig::default(), client.clone());
    let mut events = manager.subscribe();
    manager.join(vec![focus()]);
    settle().await;

    for _ in 0..10 {
        advance(1000).await;
    }

    assert!(
        drain_events(&mut events)
            .iter()
            .any(|event| matches!(event, MembershipManagerEvent::Error(_)))
    );
}

#[tokio::test(start_paused = true)]
async fn falls_back_to_using_pure_state_events_when_some_error_occurs_while_sending_delayed_events()
{
    let client = MockClient::new();
    client.set_default(
        Method::SendDelayedStateEvent,
        Err(ClientError::Http { status: 601 }),
    );
    let manager = make_manager(MembershipConfig::default(), client.clone());
    manager.join(vec![focus()]);
    settle().await;

    assert!(client.count(Method::SendStateEvent) >= 1);
}

#[tokio::test(start_paused = true)]
async fn retries_before_failing_in_case_its_a_network_error() {
    let client = MockClient::new();
    client.set_default(
        Method::SendDelayedStateEvent,
        Err(ClientError::Http { status: 501 }),
    );
    let manager = make_manager(
        MembershipConfig {
            network_error_retry_ms: 1000,
            maximum_network_error_retry_count: 7,
            ..Default::default()
        },
        client.clone(),
    );
    let mut events = manager.subscribe();
    manager.join(vec![focus()]);
    settle().await;

    for retries in 0..7 {
        assert_eq!(client.count(Method::SendDelayedStateEvent), retries + 1);
        advance(1000).await;
    }

    let error = drain_events(&mut events)
        .into_iter()
        .find_map(|event| match event {
            MembershipManagerEvent::Error(message) => Some(message),
            _ => None,
        })
        .expect("an unrecoverable error should have been reported");
    assert!(error.contains("The MembershipManager shut down because of the end condition"));
    assert_eq!(client.count(Method::SendStateEvent), 0);
}

#[tokio::test(start_paused = true)]
async fn falls_back_to_using_pure_state_events_when_unsupported_endpoint_error_encountered() {
    let client = MockClient::new();
    client.set_default(
        Method::SendDelayedStateEvent,
        Err(ClientError::UnsupportedDelayedEventsEndpoint),
    );
    let manager = make_manager(MembershipConfig::default(), client.clone());
    let mut events = manager.subscribe();
    manager.join(vec![focus()]);
    advance(1).await;

    assert!(
        !drain_events(&mut events)
            .iter()
            .any(|event| matches!(event, MembershipManagerEvent::Error(_)))
    );
    assert!(client.count(Method::SendStateEvent) >= 1);
}

// probablyLeft

#[tokio::test(start_paused = true)]
async fn emits_probably_left_when_the_server_does_not_respond_for_the_delayed_event_duration() {
    let client = MockClient::new();
    let manager = make_manager(
        MembershipConfig {
            delayed_leave_event_delay_ms: 10_000,
            ..Default::default()
        },
        client.clone(),
    );
    let mut events = manager.subscribe();
    manager.join(vec![focus()]);
    settle().await;

    // The join succeeded and the first delayed event restart check went
    // through.
    assert_eq!(client.count(Method::SendStateEvent), 1);
    assert_eq!(client.count(Method::RestartDelayedEvent), 1);
    assert_eq!(manager.status(), Status::Connected);

    // From now on the server does not respond to restarts anymore.
    client.set_stuck(Method::RestartDelayedEvent);

    let probably_left_true = |events: &[MembershipManagerEvent]| {
        events
            .iter()
            .any(|event| matches!(event, MembershipManagerEvent::ProbablyLeft(true)))
    };

    // No emission after 5s.
    advance(5000).await;
    assert!(!probably_left_true(&drain_events(&mut events)));
    assert_eq!(client.count(Method::RestartDelayedEvent), 2);

    // Note: the js-sdk test sees one more restart attempt here because
    // vitest fires intermediate timers step by step, while tokio's paused
    // clock jumps; the observable behavior (no probably-left before the
    // delayed event delay elapsed) is the same.
    advance(4999).await;
    assert_eq!(client.count(Method::RestartDelayedEvent), 3);
    assert!(!probably_left_true(&drain_events(&mut events)));

    // Let restarts succeed again before advancing the last millisecond.
    client.set_default(Method::RestartDelayedEvent, Ok(json!({})));

    // Emitted after 10s.
    advance(1).await;
    assert_eq!(client.count(Method::RestartDelayedEvent), 4);
    // Like the js-sdk assertions, `all_events` accumulates across the rest
    // of the test.
    let mut all_events = drain_events(&mut events);
    assert!(probably_left_true(&all_events));

    // Mock a sync which does not include our own membership.
    manager.on_rtc_session_member_update(&[]);
    advance(1).await;

    // We should send a new state event and an associated delayed leave
    // event.
    assert_eq!(client.count(Method::SendDelayedStateEvent), 2);
    assert_eq!(client.count(Method::SendStateEvent), 2);
    // And we are back operational.
    all_events.extend(drain_events(&mut events));
    assert!(
        all_events
            .iter()
            .any(|event| matches!(event, MembershipManagerEvent::ProbablyLeft(false)))
    );
    assert!(!manager.probably_left());
}
