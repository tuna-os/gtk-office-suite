// SPDX-License-Identifier: GPL-3.0-or-later

//! Port of `matrix-js-sdk`'s `CallMembership.spec.ts` (the
//! `SessionMembershipData` half; the sticky `RtcMembershipData` format is out
//! of scope).

#![allow(clippy::needless_pass_by_value)]

mod common;

use mandelbrot_matrixrtc::{
    CallMembership, DEFAULT_EXPIRE_DURATION_MS, MemberStateEvent, SessionMembershipData,
};
use serde_json::{Value as JsonValue, json};

/// The `membershipTemplate` of `CallMembership.spec.ts`.
fn membership_template() -> JsonValue {
    json!({
        "call_id": "",
        "scope": "m.room",
        "application": "m.call",
        "device_id": "AAAAAAA",
        "focus_active": { "type": "livekit", "focus_selection": "oldest_membership" },
        "foci_preferred": [{ "type": "livekit" }],
        "m.call.intent": "voice",
    })
}

fn make_mock_event(origin_ts: u64, content: JsonValue) -> MemberStateEvent {
    MemberStateEvent {
        event_id: "$eventid".to_owned(),
        sender: "@alice:example.org".to_owned(),
        origin_server_ts: origin_ts,
        state_key: "_@alice:example.org_AAAAAAA".to_owned(),
        content,
    }
}

fn create_call_membership(origin_ts: u64, content: JsonValue) -> CallMembership {
    CallMembership::parse_from_event(&make_mock_event(origin_ts, content)).unwrap()
}

fn template_with(patch: JsonValue) -> JsonValue {
    let mut content = membership_template();
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

#[test]
fn rejects_membership_with_no_device_id() {
    let content = template_with(json!({ "device_id": null }));
    assert!(CallMembership::parse_from_event(&make_mock_event(0, content)).is_err());
}

#[test]
fn rejects_membership_with_no_call_id() {
    let content = template_with(json!({ "call_id": null }));
    assert!(CallMembership::parse_from_event(&make_mock_event(0, content)).is_err());
}

#[test]
fn allow_membership_with_no_scope() {
    let content = template_with(json!({ "scope": null }));
    assert!(CallMembership::parse_from_event(&make_mock_event(0, content)).is_ok());
}

#[test]
fn uses_event_timestamp_if_no_created_ts() {
    let membership = create_call_membership(12345, membership_template());
    assert_eq!(membership.created_ts(), 12345);
}

#[test]
fn uses_created_ts_if_present() {
    let membership = create_call_membership(12345, template_with(json!({ "created_ts": 67890 })));
    assert_eq!(membership.created_ts(), 67890);
}

#[test]
fn considers_memberships_unexpired_if_local_age_low_enough() {
    let now = 100_000_000_000;
    let membership = create_call_membership(
        now - (DEFAULT_EXPIRE_DURATION_MS - 1),
        membership_template(),
    );
    assert!(!membership.is_expired(now));
}

#[test]
fn considers_memberships_expired_if_local_age_large_enough() {
    let now = 100_000_000_000;
    let membership = create_call_membership(
        now - (DEFAULT_EXPIRE_DURATION_MS + 1),
        membership_template(),
    );
    assert!(membership.is_expired(now));
}

#[test]
fn returns_preferred_foci() {
    let mock_focus = json!({ "type": "this_is_a_mock_focus" });
    let membership =
        create_call_membership(0, template_with(json!({ "foci_preferred": [mock_focus] })));
    assert_eq!(
        serde_json::to_value(membership.transports()).unwrap(),
        json!([{ "type": "this_is_a_mock_focus" }])
    );
}

#[test]
fn gets_the_correct_active_transport_with_oldest_membership() {
    let mock_focus = json!({ "type": "this_is_a_mock_focus" });
    let oldest_membership = create_call_membership(0, membership_template());
    let membership = create_call_membership(
        0,
        template_with(json!({
            "foci_preferred": [mock_focus],
            "focus_active": { "type": "livekit", "focus_selection": "oldest_membership" },
        })),
    );

    // If we are the oldest member we use our focus.
    assert_eq!(
        serde_json::to_value(membership.get_transport(&membership).unwrap()).unwrap(),
        json!({ "type": "this_is_a_mock_focus" })
    );

    // If there is an older member we use its focus.
    assert_eq!(
        serde_json::to_value(membership.get_transport(&oldest_membership).unwrap()).unwrap(),
        json!({ "type": "livekit" })
    );
}

#[test]
fn gets_the_correct_active_transport_with_multi_sfu() {
    let mock_focus = json!({ "type": "this_is_a_mock_focus" });
    let oldest_membership = create_call_membership(0, membership_template());
    let membership = create_call_membership(
        0,
        template_with(json!({
            "foci_preferred": [mock_focus.clone()],
            "focus_active": { "type": "livekit", "focus_selection": "multi_sfu" },
        })),
    );

    // If we are the oldest member we use our focus.
    assert_eq!(
        serde_json::to_value(membership.get_transport(&membership).unwrap()).unwrap(),
        mock_focus
    );

    // If there is an older member we still use our own focus in multi SFU.
    assert_eq!(
        serde_json::to_value(membership.get_transport(&oldest_membership).unwrap()).unwrap(),
        mock_focus
    );
}

#[test]
fn does_not_provide_focus_if_the_selection_method_is_unknown() {
    let mock_focus = json!({ "type": "this_is_a_mock_focus" });
    let membership = create_call_membership(
        0,
        template_with(json!({
            "foci_preferred": [mock_focus],
            "focus_active": { "type": "livekit", "focus_selection": "unknown" },
        })),
    );

    assert!(membership.get_transport(&membership).is_none());
}

#[test]
fn returns_correct_sender() {
    let membership = create_call_membership(0, membership_template());
    assert_eq!(membership.user_id(), "@alice:example.org");
}

#[test]
fn returns_correct_event_id() {
    let membership = create_call_membership(0, membership_template());
    assert_eq!(membership.event_id(), "$eventid");
}

#[test]
fn returns_correct_device_id() {
    let membership = create_call_membership(0, membership_template());
    assert_eq!(membership.device_id(), "AAAAAAA");
}

#[test]
fn returns_correct_call_intent() {
    let membership = create_call_membership(0, membership_template());
    assert_eq!(membership.call_intent(), Some("voice"));
}

#[test]
fn returns_correct_application() {
    let membership = create_call_membership(0, membership_template());
    assert_eq!(membership.application(), "m.call");
}

#[test]
fn returns_correct_scope() {
    let membership = create_call_membership(0, membership_template());
    assert_eq!(
        membership.scope().map(ToString::to_string).as_deref(),
        Some("m.room")
    );
}

#[test]
fn returns_correct_membership_id() {
    let membership = create_call_membership(0, membership_template());
    assert_eq!(membership.membership_id(), "@alice:example.org:AAAAAAA");
}

#[test]
fn returns_correct_unused_fields() {
    let now = 1_000_000_000_000;
    let membership = create_call_membership(0, membership_template());
    assert_eq!(membership.get_absolute_expiry(), DEFAULT_EXPIRE_DURATION_MS);
    assert_eq!(
        membership.get_ms_until_expiry(now),
        i64::try_from(DEFAULT_EXPIRE_DURATION_MS).unwrap() - i64::try_from(now).unwrap()
    );
    assert!(membership.is_expired(now));
}

#[test]
fn calculates_time_until_expiry() {
    // The server origin timestamp for this event is 1000.
    let membership = create_call_membership(1000, membership_template());
    // Should be using the absolute expiry time.
    assert_eq!(
        membership.get_ms_until_expiry(2000),
        i64::try_from(DEFAULT_EXPIRE_DURATION_MS).unwrap() - 1000
    );
}

#[test]
fn session_membership_template_roundtrips_field_for_field() {
    let template = common::session_membership_template();
    let data: SessionMembershipData = serde_json::from_value(template.clone()).unwrap();
    assert_eq!(serde_json::to_value(data).unwrap(), template);
}
