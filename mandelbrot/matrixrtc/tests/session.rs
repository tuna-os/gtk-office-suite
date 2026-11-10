// SPDX-License-Identifier: GPL-3.0-or-later

//! Port of the membership-list-building tests of `matrix-js-sdk`'s
//! `MatrixRTCSession.spec.ts` (the legacy member state event configuration).

#![allow(clippy::needless_pass_by_value)]

mod common;

use common::session_membership_template;
use mandelbrot_matrixrtc::{FocusActive, MatrixRtcSession, MemberStateEvent};
use serde_json::{Value as JsonValue, json};

const NOW: u64 = 10_000;

fn member_event(sender: &str, content: JsonValue, ts: u64) -> MemberStateEvent {
    let device_id = content
        .get("device_id")
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_owned();
    MemberStateEvent {
        event_id: format!("$event{sender}{device_id}"),
        sender: sender.to_owned(),
        origin_server_ts: ts,
        state_key: format!("_{sender}_{device_id}"),
        content,
    }
}

/// Port of `generateMembership` for the legacy configuration.
fn generate_membership(patch: JsonValue) -> JsonValue {
    let mut content = session_membership_template();
    let object = content.as_object_mut().unwrap();
    for (key, value) in patch.as_object().unwrap() {
        if value.is_null() {
            object.remove(key);
        } else {
            object.insert(key.clone(), value.clone());
        }
    }
    content
}

fn session_for(events: &[MemberStateEvent]) -> MatrixRtcSession {
    MatrixRtcSession::new_room_call(events, |_| true, NOW)
}

#[test]
fn creates_a_room_scoped_session_from_room_state() {
    let events = [member_event(
        "@mock:user.example",
        session_membership_template(),
        1000,
    )];
    let session = session_for(&events);

    assert_eq!(session.memberships.len(), 1);
    let membership = &session.memberships[0];
    assert_eq!(membership.call_id(), "");
    assert_eq!(
        membership.scope().map(ToString::to_string).as_deref(),
        Some("m.room")
    );
    assert_eq!(membership.application(), "m.call");
    assert_eq!(membership.device_id(), "AAAAAAA");
    assert!(!membership.is_expired(NOW));
}

#[test]
fn ignores_memberships_where_application_is_not_m_call() {
    let content = generate_membership(json!({ "application": "not-m.call" }));
    let events = [member_event("@mock:user.example", content, 1000)];
    assert_eq!(session_for(&events).memberships.len(), 0);
}

#[test]
fn ignores_memberships_where_call_id_is_not_empty() {
    let content = generate_membership(json!({ "call_id": "not-empty", "scope": "m.room" }));
    let events = [member_event("@mock:user.example", content, 1000)];
    assert_eq!(session_for(&events).memberships.len(), 0);
}

#[test]
fn ignores_expired_memberships_events_if_legacy_session() {
    let expired =
        generate_membership(json!({ "expires": 1000, "device_id": "EXPIRED", "created_ts": 0 }));
    let events = [
        member_event("@mock:user.example", session_membership_template(), NOW),
        member_event("@mock:user.example", expired, 0),
    ];
    let session = session_for(&events);
    assert_eq!(session.memberships.len(), 1);
    assert_eq!(session.memberships[0].device_id(), "AAAAAAA");
}

#[test]
fn ignores_memberships_events_of_members_not_in_the_room() {
    let events = [member_event(
        "@mock:user.example",
        session_membership_template(),
        1000,
    )];
    let session = MatrixRtcSession::new_room_call(&events, |_| false, NOW);
    assert_eq!(session.memberships.len(), 0);
}

#[test]
fn ignores_memberships_events_with_no_sender() {
    let events = [member_event("", session_membership_template(), 1000)];
    assert_eq!(session_for(&events).memberships.len(), 0);
}

#[test]
fn honours_created_ts() {
    let content = generate_membership(json!({ "created_ts": 500, "expires": 1000 }));
    let events = [member_event("@mock:user.example", content, 1000)];
    let session = MatrixRtcSession::new_room_call(&events, |_| true, 500);
    assert_eq!(session.memberships[0].get_absolute_expiry(), 1500);
}

#[test]
fn returns_empty_session_if_no_membership_events_are_present() {
    assert_eq!(session_for(&[]).memberships.len(), 0);
}

#[test]
fn safely_ignores_events_with_no_memberships_section() {
    let events = [member_event("@mock:user.example", json!({}), 1000)];
    assert_eq!(session_for(&events).memberships.len(), 0);
}

#[test]
fn safely_ignores_events_with_junk_memberships_section() {
    let events = [member_event(
        "@mock:user.example",
        json!({ "memberships": ["i am a fish"] }),
        1000,
    )];
    assert_eq!(session_for(&events).memberships.len(), 0);
}

#[test]
fn ignores_memberships_with_no_device_id() {
    let content = generate_membership(json!({ "device_id": null }));
    let events = [member_event("@mock:user.example", content, 1000)];
    assert_eq!(session_for(&events).memberships.len(), 0);
}

#[test]
fn ignores_memberships_with_no_call_id() {
    let content = generate_membership(json!({ "call_id": null }));
    let events = [member_event("@mock:user.example", content, 1000)];
    assert_eq!(session_for(&events).memberships.len(), 0);
}

#[test]
fn returns_the_oldest_membership_event() {
    let events = [
        member_event(
            "@mock:user.example",
            generate_membership(json!({ "device_id": "foo", "created_ts": 3000 })),
            1000,
        ),
        member_event(
            "@mock:user.example",
            generate_membership(json!({ "device_id": "old", "created_ts": 1000 })),
            1000,
        ),
        member_event(
            "@mock:user.example",
            generate_membership(json!({ "device_id": "bar", "created_ts": 2000 })),
            1000,
        ),
    ];
    let session = MatrixRtcSession::new_room_call(&events, |_| true, 4000);
    assert_eq!(session.get_oldest_membership().unwrap().device_id(), "old");
}

fn first_preferred_focus() -> JsonValue {
    json!({
        "type": "livekit",
        "livekit_service_url": "https://active.url",
        "livekit_alias": "!active:active.url",
    })
}

fn three_member_session() -> MatrixRtcSession {
    let events = [
        member_event(
            "@mock:user.example",
            generate_membership(json!({
                "device_id": "foo",
                "created_ts": 500,
                "foci_preferred": [first_preferred_focus()],
            })),
            1000,
        ),
        member_event(
            "@mock:user.example",
            generate_membership(json!({ "device_id": "old", "created_ts": 1000 })),
            1000,
        ),
        member_event(
            "@mock:user.example",
            generate_membership(json!({ "device_id": "bar", "created_ts": 2000 })),
            1000,
        ),
    ];
    MatrixRtcSession::new_room_call(&events, |_| true, 3000)
}

#[test]
fn gets_the_correct_active_focus_with_oldest_membership() {
    let session = three_member_session();
    let own_focus_active: FocusActive = serde_json::from_value(json!({
        "type": "livekit",
        "focus_selection": "oldest_membership",
    }))
    .unwrap();

    let focus = session.get_active_focus(&own_focus_active).unwrap();
    assert_eq!(
        serde_json::to_value(focus).unwrap(),
        first_preferred_focus()
    );
}

#[test]
fn does_not_provide_focus_if_the_selection_method_is_unknown() {
    let session = three_member_session();
    let own_focus_active: FocusActive = serde_json::from_value(json!({
        "type": "livekit",
        "focus_selection": "unknown",
    }))
    .unwrap();

    assert!(session.get_active_focus(&own_focus_active).is_none());
}
