//! Negative controls: the cross-binding checks must be able to fail.
//!
//! Each test wraps one handle in one of `dcp-conformance`'s `Defective` stores
//! and asserts that the check aimed at that defect returns a `Failure`. The
//! honest versions of the same checks pass in `tests/cross_binding.rs`, so the
//! failure is the defect's and not the harness's. A final test shows the same
//! defective rig still passes a check the defect cannot touch.

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

macro_rules! mutant {
    ($test:ident, $check:literal, $slot:ident, $who:expr, |$h:ident| $defect:expr, $why:literal) => {
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn $test() {
            let Some(syn) = synapse() else { return };
            let opts = RigOptions::honest().settle(Duration::from_secs(8)).$slot(|who, store, $h| {
                if who == $who {
                    defective($defect, store)
                } else {
                    store
                }
            });
            let rig = Rig::new(&syn, opts).await.expect("rig");
            let outcome = (checks::by_name($check).run)(&rig).await;
            match outcome {
                Err(f) => println!("{} failed as it must ({}): {f}", $check, $why),
                Ok(()) => panic!("{} PASSED on a rig with a defective store ({}): the check cannot fail", $check, $why),
            }
        }
    };
}

// The Matrix reader never sees the newest version: a WOPI save never reaches it.
mutant!(
    a_matrix_reader_that_hides_the_newest_version_fails_the_wopi_to_matrix_check,
    "wopi_publish_is_read_by_a_matrix_user",
    native,
    Who::Viewer,
    |_h| Defect::HideNewest,
    "the reader never shows the WOPI writer's version"
);

// The WOPI host's own view of the document hides the newest version: it never
// notices a native publish, so the stale PutFile is not a 409 case at all.
mutant!(
    a_host_over_a_store_that_hides_the_newest_version_fails_the_409_check,
    "native_publish_between_getfile_and_putfile_is_a_409",
    backing,
    Who::Editor,
    |_h| Defect::HideNewest,
    "the host never sees the native version"
);

// The Matrix reader hides all but one head: the fork is not shown.
mutant!(
    a_matrix_reader_that_hides_a_sibling_head_fails_the_conflict_check,
    "native_offline_publish_shows_as_a_conflict_to_the_matrix_reader",
    native,
    Who::Viewer,
    |_h| Defect::HideSiblingHeads,
    "the second head is hidden from the Matrix reader"
);

// A viewer's native handle that does its writes as the owner: roles are not
// enforced at the store.
mutant!(
    a_store_that_ignores_roles_fails_the_viewer_check,
    "viewer_token_cannot_write",
    native,
    Who::Viewer,
    |h| Defect::IgnoreRoles { privileged: h.owner.clone() },
    "the viewer's native publish is carried out as the owner"
);

// Revoke does nothing: the removed member's token keeps working.
mutant!(
    a_revoke_that_does_nothing_fails_the_removed_member_check,
    "removed_members_wopi_token_gets_no_new_content_and_no_write",
    native,
    Who::Owner,
    |_h| Defect::RevokeIsNoop,
    "the member is never removed, so their WOPI token still reads and writes"
);

// The host's clock never advances: a native lease never expires for WOPI.
mutant!(
    a_host_whose_clock_never_advances_fails_the_expiry_check,
    "native_lease_expiry_frees_the_wopi_lock",
    backing,
    Who::Editor,
    |_h| Defect::FrozenClock,
    "the lease never expires on the host's clock"
);

/// The defects above must not make every check fail: the same defective rig
/// (revoke is a no-op) passes a check the defect cannot reach.
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
}

/// Needs no Synapse: every check named above exists.
#[test]
fn the_controls_name_real_checks() {
    for name in [
        "wopi_publish_is_read_by_a_matrix_user",
        "native_publish_between_getfile_and_putfile_is_a_409",
        "native_offline_publish_shows_as_a_conflict_to_the_matrix_reader",
        "viewer_token_cannot_write",
        "removed_members_wopi_token_gets_no_new_content_and_no_write",
        "native_lease_expiry_frees_the_wopi_lock",
    ] {
        let _ = checks::by_name(name);
    }
}
