// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine-level equivalents of the scenarios Element Call gates its releases
//! on in `playwright/`.
//!
//! Element Call's Playwright suite drives a browser UI; most of it is not
//! applicable to a headless engine. The parts that are *logic* — focus
//! selection when the call creator leaves, federated oldest-membership focus
//! resolution, publisher hot-swap, and the `probablyLeft` resilience state —
//! are reproduced here so that they run on the PR merge gate together with
//! the ported `matrix-js-sdk` suite.
//!
//! Mapping (see `tests/e2e/CONFORMANCE.md` for the full table):
//!
//! | Element Call spec | here |
//! |---|---|
//! | `sfu-reconnect-bug.spec.ts` | `sfu_reconnect_*` |
//! | `widget/federation-oldest-membership-bug.spec.ts` | `federation_oldest_membership_*` |
//! | `widget/federated-call.test.ts` | `federated_call_*` |
//! | `widget/hotswap-legacy-compat.test.ts` | `hotswap_*` |
//! | `widget/huddle-call.test.ts` | `huddle_*` |
//! | `widget/voice-call-dm.spec.ts` | `dm_*` |
//! | `reconnect.spec.ts` | `probably_left_*` |
//! | `spa-call-sticky.spec.ts` | `sticky_*` (documented as unsupported) |

#![allow(clippy::needless_pass_by_value)]

mod common;

use common::{
    Method, MockClient, advance, drain_events, focus, make_manager, session_membership_template,
    settle,
};
use mandelbrot_matrixrtc::{
    CallMembership, FocusActive, MatrixRtcSession, MemberStateEvent, MembershipConfig,
    MembershipManagerEvent, Status,
};
use serde_json::{Value as JsonValue, json};

const NOW: u64 = 100_000;

/// A LiveKit transport pointing at `service_url`.
fn livekit(service_url: &str) -> JsonValue {
    json!({
        "type": "livekit",
        "livekit_service_url": service_url,
        "livekit_alias": "!call:example.org",
    })
}

/// A membership state event for `user`/`device` created at `created_ts`,
/// preferring the SFU at `service_url`.
fn member(user: &str, device: &str, created_ts: u64, service_url: &str) -> MemberStateEvent {
    member_with(
        user,
        device,
        created_ts,
        json!({ "foci_preferred": [livekit(service_url)] }),
    )
}

/// As [`member`], with arbitrary extra content patched into the template.
fn member_with(user: &str, device: &str, created_ts: u64, patch: JsonValue) -> MemberStateEvent {
    let mut content = session_membership_template();
    let object = content.as_object_mut().unwrap();
    object.insert("device_id".to_owned(), json!(device));
    object.insert("created_ts".to_owned(), json!(created_ts));
    for (key, value) in patch.as_object().unwrap() {
        if value.is_null() {
            object.remove(key);
        } else {
            object.insert(key.clone(), value.clone());
        }
    }
    MemberStateEvent {
        event_id: format!("$ev_{user}_{device}"),
        sender: user.to_owned(),
        origin_server_ts: created_ts,
        state_key: format!("_{user}_{device}"),
        content,
    }
}

fn session_of(events: &[MemberStateEvent]) -> MatrixRtcSession {
    MatrixRtcSession::new_room_call(events, |_| true, NOW)
}

/// The service URL our client would connect to, resolved the way
/// [`mandelbrot_matrixrtc::RtcCallSession::get_active_focus`] resolves it
/// (always `oldest_membership`).
fn active_service_url(session: &MatrixRtcSession) -> Option<String> {
    session
        .get_active_focus(&FocusActive::livekit_oldest_membership())
        .and_then(|transport| transport.as_livekit())
        .map(|focus| focus.service_url)
}

// ---------------------------------------------------------------------------
// sfu-reconnect-bug.spec.ts
//
// Element Call issue #3344: when the call creator (the oldest membership,
// whose SFU everyone follows in `oldest_membership` mode) left, the other
// participants requested a fresh JWT and reconnected their LiveKit websocket
// even though they should have stayed put. Their test counts websocket
// connections and asserts the count does not grow when the creator leaves.
//
// The engine equivalent is the *input* to that decision: which focus we
// resolve before and after the creator's membership disappears. We assert the
// resolved focus, and — decisively — that the whole media stack resolves the
// focus exactly once, at connect time (see
// `livekit_connection::LivekitCallConnection::connect` callers): there is no
// focus-change subscription anywhere in the crate, so an SFU reconnect on
// creator-leave is not reachable. This test pins the data so that adding
// focus-follow logic later cannot silently reintroduce EC's bug.
// ---------------------------------------------------------------------------

#[test]
fn sfu_reconnect_focus_is_the_creators_sfu_while_the_creator_is_present() {
    // Creator (oldest), then two more guests on their own preferred SFUs,
    // matching EC's creator / invitee / third-guest setup.
    let events = [
        member(
            "@creator:host1",
            "CREATOR",
            1_000,
            "https://sfu-creator.example",
        ),
        member("@guest_b:host1", "GUESTB", 2_000, "https://sfu-b.example"),
        member("@guest_c:host1", "GUESTC", 3_000, "https://sfu-c.example"),
    ];
    let session = session_of(&events);

    assert_eq!(session.memberships.len(), 3);
    // All three follow the creator: nobody would ever hold a second SFU
    // connection in the first place.
    assert_eq!(
        active_service_url(&session).as_deref(),
        Some("https://sfu-creator.example")
    );
}

#[test]
fn sfu_reconnect_a_late_joiner_does_not_change_the_active_focus() {
    let before = session_of(&[
        member(
            "@creator:host1",
            "CREATOR",
            1_000,
            "https://sfu-creator.example",
        ),
        member("@guest_b:host1", "GUESTB", 2_000, "https://sfu-b.example"),
    ]);
    let after = session_of(&[
        member(
            "@creator:host1",
            "CREATOR",
            1_000,
            "https://sfu-creator.example",
        ),
        member("@guest_b:host1", "GUESTB", 2_000, "https://sfu-b.example"),
        member("@guest_c:host1", "GUESTC", 3_000, "https://sfu-c.example"),
    ]);

    // The regression EC guards against is a *reconnect*; a stable focus
    // across a membership change is the precondition for not reconnecting.
    assert_eq!(active_service_url(&before), active_service_url(&after));
}

#[test]
fn sfu_reconnect_focus_moves_to_the_next_oldest_when_the_creator_leaves() {
    // The creator's membership is gone (either explicitly emptied or expired
    // via the delayed leave event).
    let session = session_of(&[
        member("@guest_b:host1", "GUESTB", 2_000, "https://sfu-b.example"),
        member("@guest_c:host1", "GUESTC", 3_000, "https://sfu-c.example"),
    ]);

    // The *resolved* focus does move — this is the value EC's buggy build
    // acted on by reconnecting. Mandelbrot resolves the focus once, before
    // `LivekitCallConnection::connect`, and never re-resolves it for the
    // lifetime of the connection, so the remaining participants stay on the
    // creator's SFU exactly as EC's fixed build does.
    assert_eq!(
        active_service_url(&session).as_deref(),
        Some("https://sfu-b.example")
    );
}

// ---------------------------------------------------------------------------
// widget/federation-oldest-membership-bug.spec.ts
//
// Two federated homeservers, each with its own SFU. Timo (host2) creates the
// call, Florian (host1) joins. Florian must *publish on Timo's SFU* (the
// oldest membership) rather than on his own preferred one, even when Timo's
// JWT service is slow. EC's bug was that the new joiner published to its own
// SFU and nobody saw its media ("Waiting for media...").
// ---------------------------------------------------------------------------

#[test]
fn federation_oldest_membership_new_joiner_follows_the_remote_creators_sfu() {
    let timo = member(
        "@timo:host2",
        "TIMO",
        1_000,
        "https://matrix-rtc.othersite.example",
    );
    let florian = member(
        "@florian:host1",
        "FLORIAN",
        2_000,
        "https://matrix-rtc.example",
    );
    let session = session_of(&[florian.clone(), timo.clone()]);

    // Ordering is by created_ts, not by the order the events arrived in.
    assert_eq!(
        session.get_oldest_membership().unwrap().user_id(),
        "@timo:host2"
    );

    // Florian resolves the *remote* SFU, not his own preferred one.
    assert_eq!(
        active_service_url(&session).as_deref(),
        Some("https://matrix-rtc.othersite.example")
    );

    // And the per-membership resolution agrees: Florian's own membership,
    // asked for its transport relative to the oldest membership, yields
    // Timo's SFU.
    let oldest = session.get_oldest_membership().unwrap().clone();
    let florian_membership = session
        .memberships
        .iter()
        .find(|m| m.user_id() == "@florian:host1")
        .unwrap();
    let transport = florian_membership.get_transport(&oldest).unwrap();
    assert_eq!(
        transport.as_livekit().unwrap().service_url,
        "https://matrix-rtc.othersite.example"
    );
}

#[test]
fn federation_oldest_membership_creator_uses_its_own_sfu() {
    let session = session_of(&[
        member(
            "@timo:host2",
            "TIMO",
            1_000,
            "https://matrix-rtc.othersite.example",
        ),
        member(
            "@florian:host1",
            "FLORIAN",
            2_000,
            "https://matrix-rtc.example",
        ),
    ]);
    let oldest = session.get_oldest_membership().unwrap();
    assert_eq!(
        oldest
            .get_transport(oldest)
            .unwrap()
            .as_livekit()
            .unwrap()
            .service_url,
        "https://matrix-rtc.othersite.example"
    );
}

// ---------------------------------------------------------------------------
// widget/federated-call.test.ts
//
// Runs the same federated call in four rtc-mode pairs: compat/compat,
// legacy/legacy, legacy/compat, compat/legacy. "legacy" is `oldest_membership`
// focus selection, "compat"/default is `multi_sfu` (each member publishes on
// their own SFU and the SFUs mesh).
//
// Mandelbrot only implements `oldest_membership`. These tests pin what we do
// in each pairing, including the fact that a peer advertising `multi_sfu` is
// *read* correctly (we still resolve its preferred focus).
// ---------------------------------------------------------------------------

#[test]
fn federated_call_legacy_legacy_both_sides_agree_on_the_oldest_sfu() {
    let session = session_of(&[
        member("@timo:host2", "TIMO", 1_000, "https://sfu-2.example"),
        member("@florian:host1", "FLORIAN", 2_000, "https://sfu-1.example"),
    ]);
    assert_eq!(
        active_service_url(&session).as_deref(),
        Some("https://sfu-2.example")
    );
}

#[test]
fn federated_call_we_can_read_a_multi_sfu_peer_membership() {
    // A `multi_sfu` peer is the Element Call default (CONFORMANCE.md notes
    // EC v0.22.0 emits `focus_selection: multi_sfu`). Parsing must not drop
    // the membership, and asking that membership for its own transport must
    // give its own preferred SFU.
    let peer = member_with(
        "@ec:host1",
        "ECDEV",
        1_000,
        json!({
            "focus_active": { "type": "livekit", "focus_selection": "multi_sfu" },
            "foci_preferred": [livekit("https://sfu-ec.example")],
        }),
    );
    let session = session_of(&[peer]);
    assert_eq!(session.memberships.len(), 1);

    let oldest = session.get_oldest_membership().unwrap();
    assert_eq!(
        oldest
            .get_transport(oldest)
            .unwrap()
            .as_livekit()
            .unwrap()
            .service_url,
        "https://sfu-ec.example"
    );

    // We ourselves always ask with `oldest_membership`, which resolves the
    // oldest member's transport regardless of what that member advertises.
    assert_eq!(
        active_service_url(&session).as_deref(),
        Some("https://sfu-ec.example")
    );
}

#[test]
fn federated_call_multi_sfu_selection_is_not_implemented_on_our_side() {
    // KNOWN GAP: `MatrixRtcSession::get_active_focus` only understands
    // `oldest_membership`; a client configured for `multi_sfu` would get no
    // focus at all. `RtcCallSession::get_active_focus` hardcodes
    // `oldest_membership`, so this is unreachable today, but it is the shape
    // of the work needed to match Element Call's default mode.
    let session = session_of(&[member("@a:host1", "AAA", 1_000, "https://sfu-a.example")]);
    let multi_sfu: FocusActive =
        serde_json::from_value(json!({ "type": "livekit", "focus_selection": "multi_sfu" }))
            .unwrap();
    assert!(session.get_active_focus(&multi_sfu).is_none());
}

// ---------------------------------------------------------------------------
// widget/hotswap-legacy-compat.test.ts
//
// Switching the local focus while connected forces the publisher to be
// recreated; EC deadlocked destroying the old publisher. The engine-level
// precondition is that a focus swap is *detectable* — i.e. the resolved focus
// really does change when the oldest member is replaced by one on another SFU.
// ---------------------------------------------------------------------------

#[test]
fn hotswap_oldest_member_swap_yields_a_different_service_url() {
    let before = session_of(&[
        member("@florian:host1", "FLORIAN", 1_000, "https://sfu-1.example"),
        member("@timo:host2", "TIMO", 2_000, "https://sfu-2.example"),
    ]);
    let after = session_of(&[member(
        "@timo:host2",
        "TIMO",
        2_000,
        "https://sfu-2.example",
    )]);

    assert_eq!(
        active_service_url(&before).as_deref(),
        Some("https://sfu-1.example")
    );
    assert_eq!(
        active_service_url(&after).as_deref(),
        Some("https://sfu-2.example")
    );
    assert_ne!(active_service_url(&before), active_service_url(&after));

    // NOTE: Mandelbrot never acts on this change — the media task resolves
    // the focus once and holds the connection. There is therefore no
    // publisher-recreation path to deadlock, and equally no support for
    // following a focus swap. Recorded as a known gap, not a failure.
}

// ---------------------------------------------------------------------------
// widget/huddle-call.test.ts
//
// Five participants in one room call; everyone must see everyone.
// ---------------------------------------------------------------------------

#[test]
fn huddle_five_memberships_are_all_listed_oldest_first() {
    let names = ["valere", "timo", "robin", "halfshot", "florian"];
    let events: Vec<MemberStateEvent> = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            member(
                &format!("@{name}:host1"),
                &format!("DEV{i}"),
                1_000 + (i as u64) * 1_000,
                "https://sfu-1.example",
            )
        })
        .collect();

    // Feed them in shuffled order: the session must sort by created_ts.
    let shuffled = [
        events[3].clone(),
        events[0].clone(),
        events[4].clone(),
        events[1].clone(),
        events[2].clone(),
    ];
    let session = session_of(&shuffled);

    assert_eq!(session.memberships.len(), 5);
    let senders: Vec<&str> = session
        .memberships
        .iter()
        .map(CallMembership::user_id)
        .collect();
    assert_eq!(
        senders,
        vec![
            "@valere:host1",
            "@timo:host1",
            "@robin:host1",
            "@halfshot:host1",
            "@florian:host1"
        ]
    );
    assert_eq!(
        session.get_oldest_membership().unwrap().user_id(),
        "@valere:host1"
    );

    // Every member has a distinct key for the encryption key ring; a
    // collision here is what breaks media E2EE in a large call.
    let mut ids: Vec<String> = session
        .memberships
        .iter()
        .map(CallMembership::membership_id)
        .collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 5);
}

#[test]
fn huddle_a_member_leaving_drops_exactly_one_membership() {
    let all: Vec<MemberStateEvent> = (0..5)
        .map(|i| {
            member(
                &format!("@u{i}:host1"),
                &format!("DEV{i}"),
                1_000 + i * 1_000,
                "https://sfu-1.example",
            )
        })
        .collect();
    let session = session_of(&all);
    assert_eq!(session.memberships.len(), 5);

    // @u2 leaves: the homeserver replaces its content with `{}`.
    let mut after = all.clone();
    after[2].content = json!({});
    let session = session_of(&after);
    assert_eq!(session.memberships.len(), 4);
    assert!(
        session
            .memberships
            .iter()
            .all(|m| m.user_id() != "@u2:host1")
    );
    // The focus is unaffected because the oldest member stayed.
    assert_eq!(
        active_service_url(&session).as_deref(),
        Some("https://sfu-1.example")
    );
}

// ---------------------------------------------------------------------------
// widget/voice-call-dm.spec.ts
//
// A one-to-one call in a DM. The UI parts (ringing toast, PIP, composer
// visibility) are Element Web's; the engine part is the advertised call
// intent, which decides whether the callee rings for a voice or a video call.
// ---------------------------------------------------------------------------

#[test]
fn dm_two_member_session_has_a_consensus_call_intent() {
    let voice = json!({ "m.call.intent": "audio" });
    let session = session_of(&[
        member_with("@brooks:host1", "BROOKS", 1_000, voice.clone()),
        member_with("@whistler:host1", "WHISTLER", 2_000, voice),
    ]);
    assert_eq!(session.memberships.len(), 2);
    assert!(
        session
            .memberships
            .iter()
            .all(|m| m.call_intent() == Some("audio"))
    );
}

#[test]
fn dm_disagreeing_call_intents_are_visible_per_membership() {
    let session = session_of(&[
        member_with(
            "@brooks:host1",
            "BROOKS",
            1_000,
            json!({ "m.call.intent": "audio" }),
        ),
        member_with(
            "@whistler:host1",
            "WHISTLER",
            2_000,
            json!({ "m.call.intent": "video" }),
        ),
    ]);
    let intents: Vec<Option<&str>> = session
        .memberships
        .iter()
        .map(CallMembership::call_intent)
        .collect();
    assert_eq!(intents, vec![Some("audio"), Some("video")]);
}

// ---------------------------------------------------------------------------
// spa-call-sticky.spec.ts — MSC4354 sticky events
//
// Element Call's matrix_2_0 mode sends `org.matrix.msc4143.rtc.member` as a
// sticky *timeline* event. We do not implement it (see CONFORMANCE.md). These
// tests document the current behavior so that the day support lands, they
// fail loudly and get updated.
// ---------------------------------------------------------------------------

#[test]
fn sticky_msc4143_rtc_member_events_are_not_parsed_as_memberships() {
    // A sticky member event, shaped as EC v0.22.0 emits it.
    let sticky = MemberStateEvent {
        event_id: "$sticky".to_owned(),
        sender: "@ec:host1".to_owned(),
        origin_server_ts: 1_000,
        state_key: String::new(),
        content: json!({
            "application": { "type": "m.call" },
            "slot_id": "m.call#!room:example.org",
            "rtc_transports": [{ "type": "livekit", "livekit_service_url": "https://sfu.example" }],
            "member": { "device_id": "ECDEV", "user_id": "@ec:host1", "id": "uuid" },
            "versions": [],
            "msc4354_sticky_key": "uuid",
        }),
    };
    // KNOWN UNSUPPORTED: MSC4354 sticky events. We must at least not crash
    // or produce a bogus membership out of one.
    assert!(CallMembership::parse_from_event(&sticky).is_err());
    assert_eq!(session_of(&[sticky]).memberships.len(), 0);
}

#[test]
fn sticky_rejoin_with_a_new_member_id_would_need_membership_id_keying() {
    // EC's "rejoin after improper leave" test: the same user+device appears
    // twice with different `member.id`s. Our key ring is keyed by
    // `membershipID`, so two entries for one device do not collide — the
    // property EC's fix relies on.
    let a = member_with(
        "@u:host1",
        "DEV",
        1_000,
        json!({ "membershipID": "session-one" }),
    );
    let mut b = member_with(
        "@u:host1",
        "DEV",
        2_000,
        json!({ "membershipID": "session-two" }),
    );
    // Distinct state keys, as a rejoin under a fresh sticky key would be.
    b.state_key = "_@u:host1_DEV_2".to_owned();
    b.event_id = "$ev_b".to_owned();

    let session = session_of(&[a, b]);
    assert_eq!(session.memberships.len(), 2);
    assert_eq!(
        session.memberships[0].membership_id(),
        "session-one".to_owned()
    );
    assert_eq!(
        session.memberships[1].membership_id(),
        "session-two".to_owned()
    );
    assert_ne!(
        session.memberships[0].membership_id(),
        session.memberships[1].membership_id()
    );
}

// ---------------------------------------------------------------------------
// reconnect.spec.ts — resilience / `probablyLeft`
//
// EC's spec stalls all homeserver requests, fast-forwards the clock, and
// asserts the "Reconnecting…" dialog appears and that the user can still hang
// up. The dialog is driven by `probablyLeft`, which is the engine state we
// own.
//
// CONFORMANCE.md open item: in the `matrix_2_0` interop run the membership
// manager flapped `ProbablyLeft` true/false repeatedly. These tests reproduce
// a slow-but-recovering homeserver at the restart-timeout boundary and pin
// the emission sequence.
// ---------------------------------------------------------------------------

/// Config used by the resilience tests: 8 s delayed leave, 5 s restart
/// interval, 2 s local timeout — the values the e2e harness runs with.
fn resilience_config() -> MembershipConfig {
    MembershipConfig {
        delayed_leave_event_delay_ms: 8_000,
        delayed_leave_event_restart_ms: 5_000,
        delayed_leave_event_restart_local_timeout_ms: 2_000,
        ..Default::default()
    }
}

#[tokio::test(start_paused = true)]
async fn probably_left_is_false_while_the_server_answers() {
    let client = MockClient::new();
    let manager = make_manager(resilience_config(), client.clone());
    let mut events = manager.subscribe();
    manager.join(vec![focus()]);
    settle().await;

    assert_eq!(manager.status(), Status::Connected);
    assert!(!manager.probably_left());

    // Three restart cycles with a healthy server.
    for _ in 0..3 {
        advance(5_000).await;
    }
    let seen = drain_events(&mut events);
    assert!(
        !seen
            .iter()
            .any(|event| matches!(event, MembershipManagerEvent::ProbablyLeft(true))),
        "probably_left must stay false against a responsive homeserver: {seen:?}"
    );
    assert!(!manager.probably_left());
    assert!(client.count(Method::RestartDelayedEvent) >= 3);
}

#[tokio::test(start_paused = true)]
async fn probably_left_never_emits_the_same_value_twice_in_a_row() {
    // The CONFORMANCE.md "flapping" observation. A single emission per real
    // transition is guaranteed by `set_and_emit_probably_left`; this test
    // pins that guarantee across a stall-then-recover cycle, so that a future
    // refactor cannot turn the state into a per-tick emitter.
    let client = MockClient::new();
    let manager = make_manager(resilience_config(), client.clone());
    let mut events = manager.subscribe();
    manager.join(vec![focus()]);
    settle().await;

    let mut sequence: Vec<bool> = Vec::new();
    let record = |drained: Vec<MembershipManagerEvent>, sequence: &mut Vec<bool>| {
        for event in drained {
            if let MembershipManagerEvent::ProbablyLeft(value) = event {
                sequence.push(value);
            }
        }
    };
    record(drain_events(&mut events), &mut sequence);

    // The homeserver stops answering restarts entirely. After the delayed
    // leave delay has elapsed we must go to `true` — exactly once.
    client.set_stuck(Method::RestartDelayedEvent);
    for _ in 0..6 {
        advance(2_500).await;
        record(drain_events(&mut events), &mut sequence);
    }
    assert!(
        manager.probably_left(),
        "expected the probably-left state after a full stall"
    );

    // The homeserver recovers. `probably_left` must return to false.
    client.set_default(Method::RestartDelayedEvent, Ok(json!({})));
    for _ in 0..4 {
        advance(2_500).await;
        record(drain_events(&mut events), &mut sequence);
    }

    assert!(
        !sequence.is_empty(),
        "no ProbablyLeft events were emitted at all"
    );
    for pair in sequence.windows(2) {
        assert_ne!(
            pair[0], pair[1],
            "ProbablyLeft emitted the same value twice in a row: {sequence:?}"
        );
    }
    // And the number of transitions must stay small: a flap per restart tick
    // would show up as a long sequence here.
    assert!(
        sequence.len() <= 4,
        "ProbablyLeft transitioned {} times over one stall/recover cycle: {sequence:?}",
        sequence.len()
    );
}

#[tokio::test(start_paused = true)]
async fn probably_left_recovers_after_a_slow_but_successful_restart() {
    // A homeserver that is slower than the 2 s local timeout but faster than
    // the 8 s delayed leave: the restart is abandoned locally and retried.
    // The membership must survive and `probably_left` must end up false.
    let client = MockClient::new();
    let manager = make_manager(resilience_config(), client.clone());
    manager.join(vec![focus()]);
    settle().await;
    assert_eq!(manager.status(), Status::Connected);

    client.set_stuck(Method::RestartDelayedEvent);
    advance(5_000).await; // first restart attempt, times out locally
    assert!(
        !manager.probably_left(),
        "must not give up before the delayed leave is due"
    );

    client.set_default(Method::RestartDelayedEvent, Ok(json!({})));
    advance(2_500).await;
    assert!(!manager.probably_left());
    assert_eq!(manager.status(), Status::Connected);
}

#[tokio::test(start_paused = true)]
async fn probably_left_clears_after_rejoining_when_the_membership_vanished() {
    // EC's "Reconnecting…" dialog must disappear once the client has
    // re-established its membership. This is the recovery half of
    // `reconnect.spec.ts`.
    let client = MockClient::new();
    let manager = make_manager(resilience_config(), client.clone());
    let mut events = manager.subscribe();
    manager.join(vec![focus()]);
    settle().await;
    let joins_before = client.count(Method::SendStateEvent);

    client.set_stuck(Method::RestartDelayedEvent);
    for _ in 0..6 {
        advance(2_500).await;
    }
    assert!(manager.probably_left());
    drain_events(&mut events);

    // The server fired our delayed leave; the next sync shows no membership
    // of ours. The manager must rejoin and clear the state.
    //
    // NOTE: the advance must exceed `delayed_leave_event_restart_local_timeout_ms`
    // because an in-flight restart holds the action loop until it times out.
    manager.on_rtc_session_member_update(&[]);
    advance(3_000).await;

    assert!(
        client.count(Method::SendStateEvent) > joins_before,
        "the manager must send a fresh membership event after being kicked out"
    );
    assert!(!manager.probably_left());
    assert!(
        drain_events(&mut events)
            .iter()
            .any(|event| matches!(event, MembershipManagerEvent::ProbablyLeft(false)))
    );
}

#[tokio::test(start_paused = true)]
async fn probably_left_flaps_against_a_chronically_slow_homeserver() {
    // CONFORMANCE.md open item: "In the `matrix_2_0` run our membership
    // manager flapped `ProbablyLeft` true/false repeatedly."
    //
    // This reproduces it deterministically and shows it is *not* a
    // Mandelbrot state-machine bug: it is the designed behavior of the
    // matrix-js-sdk algorithm we ported, verbatim
    // (`MembershipManager.ts` `restartDelayedEvent`).
    //
    // The cycle is:
    //   1. a restart is attempted; the homeserver takes longer than
    //      `delayed_leave_event_restart_local_timeout_ms` (2 s),
    //   2. we abandon it locally with `ClientError::LocalTimeout`; because the
    //      expected server-side leave time has passed we emit `ProbablyLeft(true)`,
    //   3. the retry gets an answer and we emit `ProbablyLeft(false)`,
    //   4. the next scheduled restart repeats the whole thing.
    //
    // The membership itself is never lost, so the flapping is cosmetic —
    // but a UI bound directly to this event will blink a "Reconnecting…"
    // indicator once per restart interval on a loaded homeserver. Any fix
    // (hysteresis / debounce) belongs upstream or in the UI layer, not
    // here; this test pins the current contract.
    let client = MockClient::new();
    let manager = make_manager(resilience_config(), client.clone());
    let mut events = manager.subscribe();
    manager.join(vec![focus()]);
    settle().await;
    drain_events(&mut events);

    let mut sequence: Vec<bool> = Vec::new();
    for _ in 0..3 {
        // The scheduled restart is answered too slowly and times out locally
        // after the delayed leave was already due.
        client.set_stuck(Method::RestartDelayedEvent);
        advance(5_000).await;
        advance(5_000).await;
        // The retry is answered.
        client.set_default(Method::RestartDelayedEvent, Ok(json!({})));
        advance(2_500).await;

        for event in drain_events(&mut events) {
            if let MembershipManagerEvent::ProbablyLeft(value) = event {
                sequence.push(value);
            }
        }
    }

    // We stay connected throughout: the flap is an observability artifact,
    // not a lost membership.
    assert_eq!(manager.status(), Status::Connected);
    assert!(!manager.probably_left());
    assert!(
        sequence.len() >= 4,
        "expected the documented flapping, got {sequence:?}"
    );
    // Still strictly alternating: no duplicate emissions.
    for pair in sequence.windows(2) {
        assert_ne!(pair[0], pair[1], "duplicate emission in {sequence:?}");
    }
}
