//! Negative controls: the cross-binding checks must be able to fail.
//!
//! Each test builds a rig with one defect and asserts that the check aimed at
//! it returns a `Failure` **whose message states what it observed instead of
//! what it expected**. A failure that is only a timeout or a transport error
//! would not show that the check sees the defect, so each control names the
//! phrase its check uses for the mismatch (`still observed:` for a state a
//! store never reached, or the check's own assertion text). The honest versions
//! of the same checks pass in `tests/cross_binding.rs`, so the failure is the
//! defect's and not the harness's; a final test shows a defective rig still
//! passes a check the defect cannot touch.
//!
//! The defects are `dcp-conformance`'s `Defective` stores, and, for forged
//! versions, a room with the draft-0 power-level table plus readers that trust
//! the sender (`MatrixConfig::reader_checks_senders = false`).

use std::sync::Arc;
use std::time::Duration;

use dcp_conformance::wrappers::{Defect, Defective};
use dcp_core::DocumentStore;
use dcp_interop::checks;
use dcp_interop::rig::synapse;
use dcp_interop::{Rig, RigOptions, Who};

fn defective(defect: Defect, store: Arc<dyn DocumentStore>) -> Arc<dyn DocumentStore> {
    Arc::new(Defective::new(store, defect))
}

/// How long a control waits for an effect that a defective store never shows.
const SETTLE: Duration = Duration::from_secs(6);

macro_rules! control {
    ($test:ident, $check:literal, $opts:expr, must_say = $phrase:literal, $why:literal) => {
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn $test() {
            let Some(syn) = synapse() else { return };
            let opts: RigOptions = $opts;
            let rig = Rig::new(&syn, opts).await.expect("rig");
            match (checks::by_name($check).run)(&rig).await {
                Err(f) => {
                    println!("{} failed as it must ({}): {f}", $check, $why);
                    assert!(
                        f.0.contains($phrase),
                        "{} failed, but not by the mismatch it should state ({:?} expected in the message): {f}",
                        $check,
                        $phrase
                    );
                }
                Ok(()) => panic!("{} PASSED on a rig with a defect ({}): the check cannot fail", $check, $why),
            }
        }
    };
}

// The Matrix reader never sees the newest version: a WOPI save never reaches it.
control!(
    a_matrix_reader_that_hides_the_newest_version_fails_the_wopi_to_matrix_check,
    "wopi_publish_is_read_by_a_matrix_user",
    RigOptions::honest().settle(SETTLE).native(|who, store, _| if who == Who::Viewer {
        defective(Defect::HideNewest, store)
    } else {
        store
    }),
    must_say = "still observed: heads",
    "the reader never shows the WOPI writer's version"
);

// The WOPI host's own view of the document hides the newest version: it never
// notices a native publish, so the stale PutFile is not a 409 case at all.
control!(
    a_host_over_a_store_that_hides_the_newest_version_fails_the_409_check,
    "native_publish_between_getfile_and_putfile_is_a_409",
    RigOptions::honest().settle(SETTLE).backing(|who, store, _| if who == Who::Editor {
        defective(Defect::HideNewest, store)
    } else {
        store
    }),
    must_say = "still observed: Version",
    "the host never sees the native version"
);

// The Matrix reader hides all but one head: the fork is not shown.
control!(
    a_matrix_reader_that_hides_a_sibling_head_fails_the_conflict_check,
    "native_offline_publish_shows_as_a_conflict_to_the_matrix_reader",
    RigOptions::honest().settle(SETTLE).native(|who, store, _| if who == Who::Viewer {
        defective(Defect::HideSiblingHeads, store)
    } else {
        store
    }),
    must_say = "still observed: heads",
    "the second head is hidden from the Matrix reader"
);

// A viewer's native handle that does its writes as the owner: roles are not
// enforced at the store.
control!(
    a_store_that_ignores_roles_fails_the_viewer_check,
    "viewer_token_cannot_write",
    RigOptions::honest().native(|who, store, h| if who == Who::Viewer {
        defective(Defect::IgnoreRoles { privileged: h.owner.clone() }, store)
    } else {
        store
    }),
    must_say = "must be Forbidden",
    "the viewer's native publish is carried out as the owner"
);

// Revoke does nothing: the removed member's token keeps working.
control!(
    a_revoke_that_does_nothing_fails_the_removed_member_check,
    "removed_members_wopi_token_gets_no_new_content_and_no_write",
    RigOptions::honest().native(|who, store, _| if who == Who::Owner {
        defective(Defect::RevokeIsNoop, store)
    } else {
        store
    }),
    must_say = "must be refused by the Matrix store",
    "the member is never removed, so their WOPI token still reads and writes"
);

// The host's clock never advances: a native lease never expires for WOPI.
control!(
    a_host_whose_clock_never_advances_fails_the_expiry_check,
    "native_lease_expiry_frees_the_wopi_lock",
    RigOptions::honest().settle(Duration::from_secs(10)).backing(|who, store, _| if who == Who::Editor {
        defective(Defect::FrozenClock, store)
    } else {
        store
    }),
    must_say = "still observed: LOCK still 409",
    "the lease never expires on the host's clock"
);

// Forged versions: the room has the draft-0 table (the server accepts the
// viewer's events) and the readers trust the sender. Both the Matrix readers
// and the WOPI readers (the host's MatrixStore is the same reader) must then
// show the forgery.
control!(
    readers_that_trust_the_sender_fail_the_forged_version_check_for_matrix_readers,
    "forged_version_is_dropped_by_matrix_readers",
    RigOptions::honest().draft0_power_levels_and_readers_that_trust_senders().settle(SETTLE),
    must_say = "a viewer's forged version was accepted by a Matrix reader",
    "no gate and no reader check: the forgery is a second head"
);

control!(
    readers_that_trust_the_sender_fail_the_forged_version_check_for_wopi_readers,
    "forged_version_is_dropped_by_wopi_readers",
    RigOptions::honest().draft0_power_levels_and_readers_that_trust_senders().settle(SETTLE),
    must_say = "through WOPI the heads are",
    "no gate and no reader check: WOPI sees two heads and a read-only document"
);

/// The defects above must not make every check fail: the same defective rig
/// (revoke is a no-op) passes a check the defect cannot reach, and a rig with
/// no reader checks still passes a check that does not depend on them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_defective_rig_still_passes_the_checks_the_defect_cannot_touch() {
    let Some(syn) = synapse() else { return };
    let opts = RigOptions::honest().native(|who, store, _| {
        if who == Who::Owner {
            defective(Defect::RevokeIsNoop, store)
        } else {
            store
        }
    });
    let rig = Rig::new(&syn, opts).await.expect("rig");
    if let Err(f) = checks::native_publish_opens_through_wopi_getfile(&rig).await {
        panic!("a check the defect cannot affect failed on the defective rig: {f}");
    }

    let opts = RigOptions::honest().draft0_power_levels_and_readers_that_trust_senders();
    let rig = Rig::new(&syn, opts).await.expect("rig");
    if let Err(f) = checks::wopi_publish_is_read_by_a_matrix_user(&rig).await {
        panic!("a check that does not depend on reader checks failed on the rig without them: {f}");
    }
}

/// Needs no Synapse: every check the controls name exists.
#[test]
fn the_controls_name_real_checks() {
    for name in [
        "wopi_publish_is_read_by_a_matrix_user",
        "native_publish_between_getfile_and_putfile_is_a_409",
        "native_offline_publish_shows_as_a_conflict_to_the_matrix_reader",
        "viewer_token_cannot_write",
        "removed_members_wopi_token_gets_no_new_content_and_no_write",
        "native_lease_expiry_frees_the_wopi_lock",
        "forged_version_is_dropped_by_matrix_readers",
        "forged_version_is_dropped_by_wopi_readers",
    ] {
        let _ = checks::by_name(name);
    }
}
