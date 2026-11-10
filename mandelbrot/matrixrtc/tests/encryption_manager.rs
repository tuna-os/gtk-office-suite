// SPDX-License-Identifier: GPL-3.0-or-later

//! Port of `matrix-js-sdk`'s `RTCEncryptionManager.spec.ts`.

mod common;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use common::{advance, session_membership_template, settle};
use mandelbrot_matrixrtc::{
    CallMembership, CallMembershipIdentity, ClientError, EncryptionConfig, KeyTransport,
    MemberStateEvent, ParticipantDeviceInfo, RtcEncryptionManager, decode_base64,
    encryption_key_map_key,
};
use serde_json::json;

type SendKeyCall = (String, u32, Vec<ParticipantDeviceInfo>);
type KeysChangedCall = (Vec<u8>, u32, CallMembershipIdentity, String);

/// A recording mock of the [`KeyTransport`].
#[derive(Default)]
struct MockKeyTransport {
    calls: Mutex<Vec<SendKeyCall>>,
    started: AtomicUsize,
    stopped: AtomicUsize,
}

impl MockKeyTransport {
    fn calls(&self) -> Vec<SendKeyCall> {
        self.calls.lock().unwrap().clone()
    }

    fn clear_calls(&self) {
        self.calls.lock().unwrap().clear();
    }

    fn key_at_index_sent_to(&self, index: u32, user_id: &str, device_id: &str) -> bool {
        self.calls().iter().any(|(_, i, members)| {
            *i == index
                && members
                    .iter()
                    .any(|m| m.user_id == user_id && m.device_id == device_id)
        })
    }
}

#[async_trait::async_trait]
impl KeyTransport for MockKeyTransport {
    async fn send_key(
        &self,
        key_base64: &str,
        index: u32,
        members: &[ParticipantDeviceInfo],
    ) -> Result<(), ClientError> {
        self.calls
            .lock()
            .unwrap()
            .push((key_base64.to_owned(), index, members.to_vec()));
        Ok(())
    }

    fn start(&self) {
        self.started.fetch_add(1, Ordering::SeqCst);
    }

    fn stop(&self) {
        self.stopped.fetch_add(1, Ordering::SeqCst);
    }
}

/// Port of `aCallMembership`: a membership with an explicit RTC backend
/// identity.
fn a_call_membership(
    user_id: &str,
    device_id: &str,
    ts: u64,
    rtc_backend_identity: &str,
) -> CallMembership {
    let mut content = session_membership_template();
    let object = content.as_object_mut().unwrap();
    object.insert("device_id".to_owned(), json!(device_id));
    object.insert("created_ts".to_owned(), json!(ts));

    let mut membership = CallMembership::parse_from_event(&MemberStateEvent {
        event_id: "$event:e.org".to_owned(),
        sender: user_id.to_owned(),
        origin_server_ts: ts,
        state_key: format!("_{user_id}_{device_id}"),
        content,
    })
    .unwrap();
    membership.set_rtc_backend_identity(rtc_backend_identity.to_owned());
    membership
}

/// Port of `aStateBaseMembership`: the RTC backend identity is derived as
/// `{user_id}|{device_id}`.
fn a_state_base_membership(user_id: &str, device_id: &str, ts: u64) -> CallMembership {
    a_call_membership(user_id, device_id, ts, &format!("{user_id}|{device_id}"))
}

fn identity(user_id: &str, device_id: &str) -> CallMembershipIdentity {
    CallMembershipIdentity {
        user_id: user_id.to_owned(),
        device_id: device_id.to_owned(),
        member_id: format!("{user_id}:{device_id}"),
    }
}

fn alice_identity() -> CallMembershipIdentity {
    CallMembershipIdentity {
        user_id: "@alice:example.org".to_owned(),
        device_id: "DEVICE01".to_owned(),
        member_id: "@alice:example.org:DEVICE01".to_owned(),
    }
}

struct Harness {
    manager: RtcEncryptionManager,
    members: Arc<Mutex<Vec<CallMembership>>>,
    transport: Arc<MockKeyTransport>,
    keys_changed: Arc<Mutex<Vec<KeysChangedCall>>>,
}

impl Harness {
    fn new() -> Self {
        let members: Arc<Mutex<Vec<CallMembership>>> = Arc::new(Mutex::new(Vec::new()));
        let transport = Arc::new(MockKeyTransport::default());
        let keys_changed: Arc<Mutex<Vec<KeysChangedCall>>> = Arc::new(Mutex::new(Vec::new()));

        let manager = {
            let members = Arc::clone(&members);
            let keys_changed = Arc::clone(&keys_changed);
            RtcEncryptionManager::new(
                alice_identity(),
                move || members.lock().unwrap().clone(),
                transport.clone(),
                move |key, key_index, membership, rtc_backend_identity| {
                    keys_changed.lock().unwrap().push((
                        key.to_vec(),
                        key_index,
                        membership.clone(),
                        rtc_backend_identity.to_owned(),
                    ));
                },
                Some(Box::new(|user_id, device_id, member_id| {
                    format!("MOCKSHA<{user_id}|{device_id}|{member_id}>")
                })),
            )
        };

        Self {
            manager,
            members,
            transport,
            keys_changed,
        }
    }

    fn set_members(&self, members: Vec<CallMembership>) {
        *self.members.lock().unwrap() = members;
    }

    fn push_member(&self, member: CallMembership) {
        self.members.lock().unwrap().push(member);
    }

    fn keys_changed(&self) -> Vec<KeysChangedCall> {
        self.keys_changed.lock().unwrap().clone()
    }

    fn clear_keys_changed(&self) {
        self.keys_changed.lock().unwrap().clear();
    }
}

fn expected_participant(member: &CallMembership) -> ParticipantDeviceInfo {
    ParticipantDeviceInfo {
        user_id: member.user_id().to_owned(),
        device_id: member.device_id().to_owned(),
        membership_ts: member.created_ts(),
    }
}

#[tokio::test(start_paused = true)]
async fn should_start_and_stop_the_transport_properly() {
    let harness = Harness::new();
    harness.manager.join(EncryptionConfig::default());

    assert_eq!(harness.transport.started.load(Ordering::SeqCst), 1);
    harness.manager.leave();
    assert_eq!(harness.transport.stopped.load(Ordering::SeqCst), 1);
}

// Sharing keys

#[tokio::test(start_paused = true)]
async fn set_up_my_key_asap_even_if_no_key_distribution_is_needed() {
    let harness = Harness::new();

    harness.manager.join(EncryptionConfig::default());
    // After join it is too early, the key might be lost as no one is
    // listening yet.
    assert_eq!(harness.keys_changed(), Vec::<KeysChangedCall>::new());

    harness.manager.on_memberships_update();
    settle().await;
    // The key should have been rolled out immediately.
    assert_ne!(harness.keys_changed(), Vec::<KeysChangedCall>::new());
}

#[tokio::test(start_paused = true)]
async fn should_distribute_keys_to_members_on_join() {
    let harness = Harness::new();
    let members = vec![
        a_state_base_membership("@bob:example.org", "BOBDEVICE", 1000),
        a_state_base_membership("@bob:example.org", "BOBDEVICE2", 1000),
        a_state_base_membership("@carl:example.org", "CARLDEVICE", 1000),
    ];
    harness.set_members(members.clone());

    harness.manager.join(EncryptionConfig::default());
    harness.manager.on_memberships_update();
    settle().await;

    let calls = harness.transport.calls();
    assert_eq!(calls.len(), 1);
    let (_, index, sent_to) = &calls[0];
    // It is the first key.
    assert_eq!(*index, 0);
    assert_eq!(
        sent_to,
        &members.iter().map(expected_participant).collect::<Vec<_>>()
    );

    // The key should have been rolled out immediately.
    let keys_changed = harness.keys_changed();
    assert!(keys_changed.iter().any(|(_, index, membership, identity)| {
        *index == 0 && *membership == alice_identity() && identity == "@alice:example.org:DEVICE01"
    }));
}

#[tokio::test(start_paused = true)]
async fn should_re_distribute_keys_to_members_whom_call_membership_ts_has_changed() {
    let harness = Harness::new();
    harness.set_members(vec![a_state_base_membership(
        "@bob:example.org",
        "BOBDEVICE",
        1000,
    )]);

    harness.manager.join(EncryptionConfig::default());
    harness.manager.on_memberships_update();
    settle().await;

    let calls = harness.transport.calls();
    assert_eq!(calls.len(), 1);
    // It is the first key.
    assert_eq!(calls[0].1, 0);
    assert_eq!(
        calls[0].2,
        vec![ParticipantDeviceInfo {
            user_id: "@bob:example.org".to_owned(),
            device_id: "BOBDEVICE".to_owned(),
            membership_ts: 1000,
        }]
    );
    advance(1).await;
    // The key should have been rolled out immediately.
    assert_ne!(harness.keys_changed(), Vec::<KeysChangedCall>::new());

    harness.transport.clear_calls();
    harness.clear_keys_changed();

    harness.set_members(vec![a_state_base_membership(
        "@bob:example.org",
        "BOBDEVICE",
        2000,
    )]);

    // There is no membership change, but the membership ts has changed
    // (reset?). Resend the key.
    harness.manager.on_memberships_update();
    settle().await;

    let calls = harness.transport.calls();
    assert_eq!(calls.len(), 1);
    // Resend the same key to that user.
    assert_eq!(calls[0].1, 0);
    assert_eq!(
        calls[0].2,
        vec![ParticipantDeviceInfo {
            user_id: "@bob:example.org".to_owned(),
            device_id: "BOBDEVICE".to_owned(),
            membership_ts: 2000,
        }]
    );
}

#[tokio::test(start_paused = true)]
async fn should_not_rotate_key_when_a_user_join_within_the_rotation_grace_period() {
    let harness = Harness::new();
    let members = vec![
        a_state_base_membership("@bob:example.org", "BOBDEVICE", 1000),
        a_state_base_membership("@bob:example.org", "BOBDEVICE2", 1000),
    ];
    harness.set_members(members.clone());

    let grace_period = 15_000; // 15 seconds
    // Initial rollout.
    harness.manager.join(EncryptionConfig {
        key_rotation_grace_period_ms: grace_period,
        ..Default::default()
    });
    harness.manager.on_memberships_update();
    advance(1).await;

    let calls = harness.transport.calls();
    assert_eq!(calls.len(), 1);
    // It is the first key.
    assert_eq!(calls[0].1, 0);
    assert_eq!(
        calls[0].2,
        members.iter().map(expected_participant).collect::<Vec<_>>()
    );
    harness.clear_keys_changed();
    harness.transport.clear_calls();

    // Carl joins, within the grace period.
    harness.push_member(a_state_base_membership(
        "@carl:example.org",
        "CARLDEVICE",
        1000,
    ));
    advance(grace_period / 2).await;
    harness.manager.on_memberships_update();
    settle().await;

    let calls = harness.transport.calls();
    // It should not have incremented the key index, and sent it to the newly
    // joined only.
    assert_eq!(calls.last().unwrap().1, 0);
    assert_eq!(
        calls.last().unwrap().2,
        vec![ParticipantDeviceInfo {
            user_id: "@carl:example.org".to_owned(),
            device_id: "CARLDEVICE".to_owned(),
            membership_ts: 1000,
        }]
    );

    assert_eq!(harness.keys_changed(), Vec::<KeysChangedCall>::new());
    advance(1000).await;
}

// Test an edge case where the use key delay is higher than the grace period.
// This means that no matter what, the key once rolled out will be too old to
// be re-used for the new member that joined within the grace period. So we
// expect another rotation to happen in all cases where a new member joins.
#[tokio::test(start_paused = true)]
async fn test_grace_period_lower_than_delay_period() {
    let harness = Harness::new();
    harness.set_members(vec![
        a_state_base_membership("@bob:example.org", "BOBDEVICE", 1000),
        a_state_base_membership("@bob:example.org", "BOBDEVICE2", 1000),
    ]);

    let grace_period = 3_000; // 3 seconds
    let use_key_delay = grace_period + 2_000; // 5 seconds
    // Initial rollout.
    harness.manager.join(EncryptionConfig {
        use_key_delay_ms: use_key_delay,
        key_rotation_grace_period_ms: grace_period,
        ..Default::default()
    });
    harness.manager.on_memberships_update();
    advance(1).await;

    harness.clear_keys_changed();
    harness.transport.clear_calls();

    // The existing members have been talking for 5 minutes.
    advance(5 * 60 * 1000).await;

    // A new member joins, that should trigger a key rotation.
    harness.push_member(a_state_base_membership(
        "@carl:example.org",
        "CARLDEVICE",
        1000,
    ));
    harness.manager.on_memberships_update();
    advance(1).await;

    // A new member joins, within the grace period, but under the delay
    // period.
    harness.push_member(a_state_base_membership(
        "@david:example.org",
        "DAVDEVICE",
        1000,
    ));
    advance((use_key_delay - grace_period) / 2).await;
    harness.manager.on_memberships_update();

    // Wait past the delay period.
    advance(5_000).await;

    // Even though the new member joined within the grace period, the key
    // should be rotated because once the delay period has passed the grace
    // period is also exceeded/the key is too old to be reshared.

    // CARLDEVICE should have received a key with index 1 and another one
    // with index 2.
    assert!(
        harness
            .transport
            .key_at_index_sent_to(1, "@carl:example.org", "CARLDEVICE")
    );
    assert!(
        harness
            .transport
            .key_at_index_sent_to(2, "@carl:example.org", "CARLDEVICE")
    );
    // Of course, should not have received the first key.
    assert!(
        !harness
            .transport
            .key_at_index_sent_to(0, "@carl:example.org", "CARLDEVICE")
    );

    // DAVDEVICE should only have received a key with index 2.
    assert!(
        harness
            .transport
            .key_at_index_sent_to(2, "@david:example.org", "DAVDEVICE")
    );
    assert!(
        !harness
            .transport
            .key_at_index_sent_to(1, "@david:example.org", "DAVDEVICE")
    );
}

#[tokio::test(start_paused = true)]
async fn should_rotate_key_when_a_user_join_past_the_rotation_grace_period() {
    let harness = Harness::new();
    harness.set_members(vec![
        a_state_base_membership("@bob:example.org", "BOBDEVICE", 1000),
        a_state_base_membership("@bob:example.org", "BOBDEVICE2", 1000),
    ]);

    let grace_period = 15_000; // 15 seconds
    // Initial rollout.
    harness.manager.join(EncryptionConfig {
        key_rotation_grace_period_ms: grace_period,
        ..Default::default()
    });
    harness.manager.on_memberships_update();
    advance(1).await;

    harness.clear_keys_changed();
    harness.transport.clear_calls();

    advance(grace_period + 1000).await;
    harness.push_member(a_state_base_membership(
        "@carl:example.org",
        "CARLDEVICE",
        1000,
    ));
    harness.manager.on_memberships_update();
    settle().await;

    let calls = harness.transport.calls();
    let (_, index, sent_to) = calls.last().unwrap();
    // It should have incremented the key index and sent it to everyone.
    assert_eq!(*index, 1);
    assert_eq!(sent_to.len(), 3);
    for (user_id, device_id) in [
        ("@bob:example.org", "BOBDEVICE"),
        ("@bob:example.org", "BOBDEVICE2"),
        ("@carl:example.org", "CARLDEVICE"),
    ] {
        assert!(
            sent_to
                .iter()
                .any(|m| m.user_id == user_id && m.device_id == device_id)
        );
    }

    // Wait for the use key delay to pass.
    advance(5000).await;

    assert_ne!(harness.keys_changed(), Vec::<KeysChangedCall>::new());
}

#[tokio::test(start_paused = true)]
async fn should_not_rotate_key_when_several_users_join_within_the_rotation_grace_period() {
    let harness = Harness::new();
    harness.set_members(vec![
        a_state_base_membership("@bob:example.org", "BOBDEVICE", 1000),
        a_state_base_membership("@bob:example.org", "BOBDEVICE2", 1000),
    ]);

    // Initial rollout.
    harness.manager.join(EncryptionConfig::default());
    harness.manager.on_memberships_update();
    advance(1).await;

    harness.clear_keys_changed();
    harness.transport.clear_calls();

    let new_joiners = [
        ("@carl:example.org", "CARLDEVICE"),
        ("@dave:example.org", "DAVEDEVICE"),
        ("@eve:example.org", "EVEDEVICE"),
        ("@frank:example.org", "FRANKDEVICE"),
        ("@george:example.org", "GEORGEDEVICE"),
    ];

    for (user_id, device_id) in new_joiners {
        harness.push_member(a_state_base_membership(user_id, device_id, 1000));
        advance(1_000).await;
        harness.manager.on_memberships_update();
        advance(1).await;
    }

    let calls = harness.transport.calls();
    assert_eq!(calls.len(), new_joiners.len());

    for (i, (user_id, device_id)) in new_joiners.iter().enumerate() {
        // It should not have incremented the key index, and sent it to the
        // new joiner only.
        assert_eq!(calls[i].1, 0);
        assert!(
            calls[i]
                .2
                .iter()
                .any(|m| m.user_id == *user_id && m.device_id == *device_id)
        );
    }

    assert_eq!(harness.keys_changed(), Vec::<KeysChangedCall>::new());
}

#[tokio::test(start_paused = true)]
async fn should_not_resend_keys_when_no_changes() {
    let harness = Harness::new();
    harness.set_members(vec![
        a_state_base_membership("@bob:example.org", "BOBDEVICE", 1000),
        a_state_base_membership("@bob:example.org", "BOBDEVICE2", 1000),
    ]);

    // Initial rollout.
    harness.manager.join(EncryptionConfig::default());
    harness.manager.on_memberships_update();
    advance(1).await;

    assert_eq!(harness.transport.calls().len(), 1);
    harness.clear_keys_changed();
    harness.transport.clear_calls();

    harness.manager.on_memberships_update();
    advance(200).await;
    harness.manager.on_memberships_update();
    advance(100).await;
    harness.manager.on_memberships_update();
    advance(50).await;
    harness.manager.on_memberships_update();
    advance(100).await;

    assert_eq!(harness.transport.calls(), Vec::<SendKeyCall>::new());
}

#[tokio::test(start_paused = true)]
async fn should_rotate_key_when_a_user_leaves_and_delay_the_rollout() {
    let harness = Harness::new();
    let members = vec![
        a_state_base_membership("@bob:example.org", "BOBDEVICE", 1000),
        a_state_base_membership("@bob:example.org", "BOBDEVICE2", 1000),
        a_state_base_membership("@carl:example.org", "CARLDEVICE", 1000),
    ];
    harness.set_members(members.clone());

    harness.manager.join(EncryptionConfig::default());
    harness.manager.on_memberships_update();
    advance(10).await;

    let calls = harness.transport.calls();
    assert_eq!(calls.len(), 1);
    // It is the first key.
    assert_eq!(calls[0].1, 0);
    assert_eq!(
        calls[0].2,
        members.iter().map(expected_participant).collect::<Vec<_>>()
    );
    // Initial rollout.
    assert_eq!(harness.keys_changed().len(), 1);
    harness.clear_keys_changed();

    let updated_members = vec![
        a_state_base_membership("@bob:example.org", "BOBDEVICE", 1000),
        a_state_base_membership("@bob:example.org", "BOBDEVICE2", 1000),
    ];
    harness.set_members(updated_members.clone());

    harness.manager.on_memberships_update();

    advance(200).await;
    // The key is rotated but not rolled out yet, to give time for the key to
    // be delivered.
    let calls = harness.transport.calls();
    let (_, index, sent_to) = calls.last().unwrap();
    // It should have incremented the key index and sent it to the updated
    // members.
    assert_eq!(*index, 1);
    assert_eq!(
        sent_to,
        &updated_members
            .iter()
            .map(expected_participant)
            .collect::<Vec<_>>()
    );

    assert_eq!(harness.keys_changed(), Vec::<KeysChangedCall>::new());
    advance(1000).await;

    // Now it should be rolled out.
    let keys_changed = harness.keys_changed();
    assert!(keys_changed.iter().any(|(_, index, membership, identity)| {
        *index == 1 && *membership == alice_identity() && identity == "@alice:example.org:DEVICE01"
    }));
}

#[tokio::test(start_paused = true)]
async fn should_not_distribute_keys_if_encryption_is_disabled() {
    let harness = Harness::new();
    harness.set_members(vec![
        a_state_base_membership("@bob:example.org", "BOBDEVICE", 1000),
        a_state_base_membership("@bob:example.org", "BOBDEVICE2", 1000),
        a_state_base_membership("@carl:example.org", "CARLDEVICE", 1000),
    ]);

    harness.manager.join(EncryptionConfig {
        manage_media_keys: false,
        ..Default::default()
    });
    harness.manager.on_memberships_update();
    settle().await;

    assert_eq!(harness.transport.calls(), Vec::<SendKeyCall>::new());
    assert_eq!(harness.keys_changed(), Vec::<KeysChangedCall>::new());
}

// Receiving keys

#[tokio::test(start_paused = true)]
async fn should_not_accept_keys_when_manage_media_keys_is_disabled() {
    let harness = Harness::new();
    harness.set_members(vec![a_state_base_membership(
        "@bob:example.org",
        "BOBDEVICE",
        1000,
    )]);

    harness.manager.join(EncryptionConfig {
        manage_media_keys: false,
        ..Default::default()
    });
    harness.manager.on_memberships_update();
    advance(10).await;

    harness.manager.on_new_key_received(
        identity("@bob:example.org", "BOBDEVICE"),
        "AAAAAAAAAAA",
        0, // KeyId
        0, // Timestamp
    );
    settle().await;

    assert_eq!(harness.keys_changed(), Vec::<KeysChangedCall>::new());
}

#[tokio::test(start_paused = true)]
async fn should_accept_keys_from_transport() {
    let harness = Harness::new();
    harness.set_members(vec![
        a_call_membership("@bob:example.org", "BOBDEVICE", 1000, "rtcIDBOB1"),
        a_call_membership("@bob:example.org", "BOBDEVICE2", 1000, "rtcIDBOB2"),
        a_call_membership("@carl:example.org", "CARLDEVICE", 1000, "rtcIDCARL1"),
    ]);

    harness.manager.join(EncryptionConfig::default());
    harness.manager.on_memberships_update();
    advance(10).await;

    harness.manager.on_new_key_received(
        identity("@bob:example.org", "BOBDEVICE"),
        "AAAAAAAAAAA",
        0,
        0,
    );
    harness.manager.on_new_key_received(
        identity("@bob:example.org", "BOBDEVICE2"),
        "BBBBBBBBBBB",
        4,
        0,
    );
    harness.manager.on_new_key_received(
        identity("@carl:example.org", "CARLDEVICE"),
        "CCCCCCCCCC",
        8,
        0,
    );
    settle().await;

    let keys_changed = harness.keys_changed();
    assert_eq!(keys_changed.len(), 4);
    assert!(
        keys_changed
            .iter()
            .any(|(key, index, membership, identity)| {
                *key == decode_base64("AAAAAAAAAAA").unwrap()
                    && *index == 0
                    && membership.user_id == "@bob:example.org"
                    && membership.device_id == "BOBDEVICE"
                    && identity == "rtcIDBOB1"
            })
    );
    assert!(
        keys_changed
            .iter()
            .any(|(key, index, membership, identity)| {
                *key == decode_base64("BBBBBBBBBBB").unwrap()
                    && *index == 4
                    && membership.user_id == "@bob:example.org"
                    && membership.device_id == "BOBDEVICE2"
                    && identity == "rtcIDBOB2"
            })
    );
    assert!(
        keys_changed
            .iter()
            .any(|(key, index, membership, identity)| {
                *key == decode_base64("CCCCCCCCCC").unwrap()
                    && *index == 8
                    && membership.user_id == "@carl:example.org"
                    && membership.device_id == "CARLDEVICE"
                    && identity == "rtcIDCARL1"
            })
    );
}

#[tokio::test(start_paused = true)]
async fn should_support_quick_re_joiner_if_keys_received_out_of_order() {
    let harness = Harness::new();
    harness.set_members(vec![a_state_base_membership(
        "@carol:example.org",
        "CAROLDEVICE",
        1000,
    )]);

    // Let's join.
    harness.manager.join(EncryptionConfig::default());
    advance(10).await;

    // Simulate Carol leaving then joining back, and keys received out of
    // order.
    let initial_key0_timestamp = 1000;
    let new_key0_timestamp = 2000;

    harness.manager.on_new_key_received(
        identity("@carol:example.org", "CAROLDEVICE"),
        "BBBBBBBBBBB",
        0,
        new_key0_timestamp,
    );
    advance(20).await;

    harness.manager.on_new_key_received(
        identity("@carol:example.org", "CAROLDEVICE"),
        "AAAAAAAAAAA",
        0,
        initial_key0_timestamp,
    );
    advance(20).await;

    // The latest key used for Carol should be the one with the latest
    // timestamp.
    let keys_changed = harness.keys_changed();
    let (key, index, membership, rtc_identity) = keys_changed.last().unwrap();
    assert_eq!(*key, decode_base64("BBBBBBBBBBB").unwrap());
    assert_eq!(*index, 0);
    assert_eq!(*membership, identity("@carol:example.org", "CAROLDEVICE"));
    assert_eq!(rtc_identity, "@carol:example.org|CAROLDEVICE");
}

#[tokio::test(start_paused = true)]
async fn should_store_keys_for_later_retrieval() {
    let harness = Harness::new();
    harness.set_members(vec![
        a_call_membership(
            "@bob:example.org",
            "BOBDEVICE",
            1000,
            "@bob:example.org|BOBDEVICE",
        ),
        a_call_membership(
            "@bob:example.org",
            "BOBDEVICE2",
            1000,
            "@bob:example.org|BOBDEVICE2",
        ),
        a_call_membership(
            "@carl:example.org",
            "CARLDEVICE",
            1000,
            "@carl:example.org|CARLDEVICE",
        ),
    ]);

    // Let's join.
    harness.manager.join(EncryptionConfig::default());
    harness.manager.on_memberships_update();
    advance(10).await;

    harness.manager.on_new_key_received(
        identity("@carl:example.org", "CARLDEVICE"),
        "BBBBBBBBBBB",
        0,
        1000,
    );
    harness.manager.on_new_key_received(
        identity("@carl:example.org", "CARLDEVICE"),
        "CCCCCCCCCCC",
        5,
        1000,
    );
    harness.manager.on_new_key_received(
        identity("@bob:example.org", "BOBDEVICE2"),
        "DDDDDDDDDDD",
        0,
        1000,
    );
    settle().await;

    let known_keys = harness.manager.get_encryption_keys();

    // My own key should be there.
    let my_ring = known_keys
        .get(&encryption_key_map_key(&alice_identity()))
        .unwrap();
    assert_eq!(my_ring.len(), 1);
    assert_eq!(my_ring[0].key_index, 0);
    assert_eq!(my_ring[0].key.len(), 16);

    let carl_ring = known_keys
        .get(&encryption_key_map_key(&identity(
            "@carl:example.org",
            "CARLDEVICE",
        )))
        .unwrap();
    assert_eq!(carl_ring.len(), 2);
    assert_eq!(carl_ring[0].key_index, 0);
    assert_eq!(carl_ring[0].key, decode_base64("BBBBBBBBBBB").unwrap());
    assert_eq!(carl_ring[1].key_index, 5);
    assert_eq!(carl_ring[1].key, decode_base64("CCCCCCCCCCC").unwrap());

    let bob_ring = known_keys
        .get(&encryption_key_map_key(&identity(
            "@bob:example.org",
            "BOBDEVICE2",
        )))
        .unwrap();
    assert_eq!(bob_ring.len(), 1);
    assert_eq!(bob_ring[0].key_index, 0);
    assert_eq!(bob_ring[0].key, decode_base64("DDDDDDDDDDD").unwrap());

    let bob_device1_ring = known_keys.get(&encryption_key_map_key(&identity(
        "@bob:example.org",
        "BOBDEVICE",
    )));
    assert!(bob_device1_ring.is_none());
}

#[tokio::test(start_paused = true)]
async fn should_only_rotate_once_again_if_several_membership_changes_during_a_rollout() {
    let harness = Harness::new();
    harness.set_members(vec![
        a_state_base_membership("@bob:example.org", "BOBDEVICE", 1000),
        a_state_base_membership("@bob:example.org", "BOBDEVICE2", 1000),
        a_state_base_membership("@carl:example.org", "CARLDEVICE", 1000),
    ]);

    // Let's join.
    harness.manager.join(EncryptionConfig::default());
    harness.manager.on_memberships_update();
    advance(10).await;

    // The initial rollout.
    let keys_changed = harness.keys_changed();
    assert!(keys_changed.iter().any(|(_, index, membership, identity)| {
        *index == 0 && *membership == alice_identity() && identity == "@alice:example.org:DEVICE01"
    }));
    harness.clear_keys_changed();

    // Trigger a key rotation with a leaver. This should start a new key
    // rollout.
    harness.set_members(vec![
        a_state_base_membership("@bob:example.org", "BOBDEVICE", 1000),
        a_state_base_membership("@bob:example.org", "BOBDEVICE2", 1000),
    ]);
    harness.manager.on_memberships_update();
    advance(10).await;

    // Now simulate a new leaver. The key `1` rollout is in progress.
    harness.set_members(vec![a_state_base_membership(
        "@bob:example.org",
        "BOBDEVICE",
        1000,
    )]);
    harness.manager.on_memberships_update();
    advance(10).await;

    // And another one (plus a joiner). The key `1` rollout is still in
    // progress.
    harness.set_members(vec![a_state_base_membership(
        "@bob:example.org",
        "BOBDEVICE3",
        1000,
    )]);
    harness.manager.on_memberships_update();
    advance(10).await;

    // Let all rollouts finish. Two advances are needed because tokio's
    // paused clock jumps: the second rollout's use-key-delay timer is
    // created only after the first advance completed.
    advance(2000).await;
    advance(2000).await;

    // There should be 2 rollouts: the `1` rollout, then just one additional
    // one that has "buffered" the 2 membership changes with leavers.
    let keys_changed = harness.keys_changed();
    assert_eq!(keys_changed.len(), 2);
    assert!(keys_changed.iter().any(|(_, index, membership, identity)| {
        *index == 1 && *membership == alice_identity() && identity == "@alice:example.org:DEVICE01"
    }));
    assert!(keys_changed.iter().any(|(_, index, membership, identity)| {
        *index == 2 && *membership == alice_identity() && identity == "@alice:example.org:DEVICE01"
    }));

    // Key `2` should only be distributed to the last membership.
    let calls = harness.transport.calls();
    let (_, index, sent_to) = calls.last().unwrap();
    assert_eq!(*index, 2);
    assert_eq!(
        sent_to,
        &vec![ParticipantDeviceInfo {
            user_id: "@bob:example.org".to_owned(),
            device_id: "BOBDEVICE3".to_owned(),
            membership_ts: 1000,
        }]
    );
}

// RTC backend pseudonymous id

#[tokio::test(start_paused = true)]
async fn should_use_pseudo_rtc_backend_identity_if_using_sticky_events() {
    let harness = Harness::new();
    harness.manager.join(EncryptionConfig {
        manage_media_keys: true,
        unstable_send_sticky_events: true,
        ..Default::default()
    });
    harness.manager.on_memberships_update();
    settle().await;

    let keys_changed = harness.keys_changed();
    assert!(keys_changed.iter().any(|(_, index, membership, identity)| {
        *index == 0
            && *membership == alice_identity()
            && identity == "MOCKSHA<@alice:example.org|DEVICE01|@alice:example.org:DEVICE01>"
    }));
}

#[tokio::test(start_paused = true)]
async fn should_use_legacy_participant_id_if_not_using_sticky_event() {
    let harness = Harness::new();
    harness.manager.join(EncryptionConfig {
        manage_media_keys: true,
        unstable_send_sticky_events: false,
        ..Default::default()
    });
    harness.manager.on_memberships_update();
    settle().await;

    let keys_changed = harness.keys_changed();
    assert!(keys_changed.iter().any(|(_, index, membership, identity)| {
        *index == 0 && *membership == alice_identity() && identity == "@alice:example.org:DEVICE01"
    }));
}

#[tokio::test(start_paused = true)]
async fn should_use_early_keys_as_soon_as_the_membership_is_known() {
    let harness = Harness::new();
    harness.manager.join(EncryptionConfig {
        manage_media_keys: true,
        unstable_send_sticky_events: true,
        ..Default::default()
    });
    harness.manager.on_memberships_update();
    settle().await;

    // In 2.0 mode the participant identity is pseudonymous and known from
    // the RTC membership itself. If a key is received before we have
    // processed the membership, we cannot pass it to the media layer yet
    // because we don't know the rtcBackendIdentity to use.
    harness.manager.on_new_key_received(
        identity("@bob:example.org", "BOBDEVICE"),
        "AAAAAAAAAAA",
        0,
        0,
    );
    settle().await;

    // No membership yet, cannot process the key, so should not have called
    // the callback.
    let keys_changed = harness.keys_changed();
    assert_eq!(keys_changed.len(), 1 /* only own key */);
    assert!(
        !keys_changed
            .iter()
            .any(|(_, _, membership, _)| membership.user_id == "@bob:example.org")
    );

    // Now process the membership.
    let bob_rtc_id = "MOCKSHA<@bob:example.org|BOBDEVICE|@bob:example.org:BOBDEVICE>";
    harness.set_members(vec![a_call_membership(
        "@bob:example.org",
        "BOBDEVICE",
        1000,
        bob_rtc_id,
    )]);
    harness.manager.on_memberships_update();
    settle().await;

    let keys_changed = harness.keys_changed();
    assert_eq!(keys_changed.len(), 2);
    assert!(keys_changed.iter().any(|(_, index, membership, identity)| {
        *index == 0
            && membership.user_id == "@bob:example.org"
            && membership.device_id == "BOBDEVICE"
            && identity == bob_rtc_id
    }));
}
