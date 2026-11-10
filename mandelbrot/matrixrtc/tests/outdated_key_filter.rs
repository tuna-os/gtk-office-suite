// SPDX-License-Identifier: GPL-3.0-or-later

//! Table-driven boundary tests for [`OutdatedKeyFilter`]: identity isolation,
//! key-index isolation, equal timestamps and reversed arrival order.
//!
//! The ported `matrix-js-sdk` case (one identity, one index, one older key)
//! lives in `key_transport.rs`; these cover the edges around it.

use mandelbrot_matrixrtc::{CallMembershipIdentity, InboundEncryptionSession, OutdatedKeyFilter};

fn identity(user_id: &str, device_id: &str, member_id: &str) -> CallMembershipIdentity {
    CallMembershipIdentity {
        user_id: user_id.to_owned(),
        device_id: device_id.to_owned(),
        member_id: member_id.to_owned(),
    }
}

/// The identity every case uses unless it is testing identity isolation.
fn alice() -> CallMembershipIdentity {
    identity("@alice:localhost", "ALICEDEV", "@alice:localhost:ALICEDEV")
}

/// One key arriving at the filter and the verdict expected for it.
struct Arrival {
    membership: CallMembershipIdentity,
    key_index: u32,
    creation_ts: u64,
    expect_outdated: bool,
}

fn arrival(
    membership: &CallMembershipIdentity,
    key_index: u32,
    creation_ts: u64,
    expect_outdated: bool,
) -> Arrival {
    Arrival {
        membership: membership.clone(),
        key_index,
        creation_ts,
        expect_outdated,
    }
}

/// Feed every arrival to one fresh filter, in order, and check each verdict.
fn run_case(name: &str, arrivals: &[Arrival]) {
    let mut filter = OutdatedKeyFilter::new();
    for (step, a) in arrivals.iter().enumerate() {
        let item = InboundEncryptionSession {
            key: vec![0; 16],
            membership: a.membership.clone(),
            key_index: a.key_index,
            creation_ts: a.creation_ts,
        };
        assert_eq!(
            filter.is_outdated(&a.membership, &item),
            a.expect_outdated,
            "case {name:?}, step {step} (index {}, ts {})",
            a.key_index,
            a.creation_ts,
        );
    }
}

#[test]
fn first_key_is_never_outdated() {
    // Even a timestamp of zero has nothing to be older than.
    run_case("zero ts", &[arrival(&alice(), 0, 0, false)]);
    run_case("max ts", &[arrival(&alice(), 7, u64::MAX, false)]);
}

#[test]
fn equal_timestamps_are_not_outdated() {
    // Only a strictly older key is stale; a redelivery or a same-millisecond
    // rotation must still reach the encryption layer.
    let a = alice();
    run_case(
        "same index, same ts",
        &[
            arrival(&a, 0, 1000, false),
            arrival(&a, 0, 1000, false),
            arrival(&a, 0, 1000, false),
        ],
    );
    run_case(
        "equal ts after newer then older",
        &[
            arrival(&a, 0, 1000, false),
            arrival(&a, 0, 999, true),
            arrival(&a, 0, 1000, false),
        ],
    );
}

#[test]
fn reversed_arrival_order_keeps_the_newest_key() {
    let a = alice();
    // Quick join/leave/join: two keys of index 0 are distributed, and the
    // second-created one may arrive first.
    run_case(
        "newer first, older second",
        &[arrival(&a, 0, 2000, false), arrival(&a, 0, 1000, true)],
    );
    run_case(
        "in order",
        &[arrival(&a, 0, 1000, false), arrival(&a, 0, 2000, false)],
    );
    // A rejected key must not lower the watermark: anything older than the
    // newest accepted key stays outdated, however many stale keys came between.
    run_case(
        "stale keys do not lower the watermark",
        &[
            arrival(&a, 0, 3000, false),
            arrival(&a, 0, 1000, true),
            arrival(&a, 0, 2000, true),
            arrival(&a, 0, 2999, true),
            arrival(&a, 0, 3000, false),
            arrival(&a, 0, 3001, false),
        ],
    );
    // An accepted key raises the watermark.
    run_case(
        "accepted key raises the watermark",
        &[
            arrival(&a, 0, 1000, false),
            arrival(&a, 0, 2000, false),
            arrival(&a, 0, 1500, true),
        ],
    );
}

#[test]
fn key_indexes_are_isolated() {
    let a = alice();
    // A newer key at one index says nothing about the other indexes.
    run_case(
        "older ts at a different index is fresh",
        &[
            arrival(&a, 0, 5000, false),
            arrival(&a, 1, 100, false),
            arrival(&a, 255, 1, false),
        ],
    );
    // Each index keeps its own watermark.
    run_case(
        "watermarks are per index",
        &[
            arrival(&a, 0, 5000, false),
            arrival(&a, 1, 100, false),
            arrival(&a, 0, 4999, true),
            arrival(&a, 1, 99, true),
            arrival(&a, 1, 100, false),
            arrival(&a, 0, 5000, false),
        ],
    );
    run_case(
        "index wraps independently of its neighbours",
        &[
            arrival(&a, u32::MAX, 10, false),
            arrival(&a, 0, 5, false),
            arrival(&a, u32::MAX, 9, true),
        ],
    );
}

#[test]
fn identities_are_isolated() {
    let base = alice();
    let other_user = identity("@bob:localhost", "ALICEDEV", "@alice:localhost:ALICEDEV");
    let other_device = identity("@alice:localhost", "OTHERDEV", "@alice:localhost:ALICEDEV");
    let other_member = identity(
        "@alice:localhost",
        "ALICEDEV",
        "@alice:localhost:OTHERMEMBER",
    );

    // Differing in exactly one of user, device or member id makes a distinct
    // participant with its own watermark.
    for (what, other) in [
        ("user id", &other_user),
        ("device id", &other_device),
        ("member id", &other_member),
    ] {
        run_case(
            &format!("different {what}: older ts is fresh"),
            &[
                arrival(&base, 0, 5000, false),
                arrival(other, 0, 100, false),
            ],
        );
        run_case(
            &format!("different {what}: watermarks are per identity"),
            &[
                arrival(&base, 0, 5000, false),
                arrival(other, 0, 100, false),
                arrival(&base, 0, 4999, true),
                arrival(other, 0, 99, true),
                arrival(other, 0, 100, false),
                arrival(&base, 0, 5000, false),
            ],
        );
    }
}

#[test]
fn identity_and_index_isolation_compose() {
    let a = alice();
    let b = identity("@bob:localhost", "BOBDEV", "@bob:localhost:BOBDEV");
    run_case(
        "same index different identity, same identity different index",
        &[
            arrival(&a, 0, 1000, false),
            arrival(&b, 0, 500, false),
            arrival(&a, 1, 500, false),
            arrival(&b, 1, 400, false),
            arrival(&a, 0, 999, true),
            arrival(&b, 0, 499, true),
            arrival(&a, 1, 499, true),
            arrival(&b, 1, 399, true),
        ],
    );
}

#[test]
fn separate_filters_do_not_share_state() {
    let a = alice();
    let mut first = OutdatedKeyFilter::new();
    let mut second = OutdatedKeyFilter::new();
    let item = |ts| InboundEncryptionSession {
        key: vec![0; 16],
        membership: a.clone(),
        key_index: 0,
        creation_ts: ts,
    };

    assert!(!first.is_outdated(&a, &item(2000)));
    assert!(!second.is_outdated(&a, &item(1000)));
    assert!(first.is_outdated(&a, &item(1000)));
}
