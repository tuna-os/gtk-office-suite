//! The shared section-12 journeys (`dcp-conformance`) against both stacks on
//! one real Synapse, relaxed or default rate limits (`DCP_SYNAPSE_PROFILE`):
//! `MatrixStore`, and `WopiClientStore -> WopiHost -> MatrixStore`. Needs
//! `DCP_SYNAPSE_URL`; fails without it unless `DCP_SKIP_SYNAPSE=1`. Every
//! journey is timed (`TIMING` lines, and `DCP_MEASURE_OUT`); on the default
//! profile see `dcp_interop::profile` for the deadline and for how a journey
//! that really fails there is recorded rather than weakened.
//!
//! The skip lists are asserted exactly. A journey that silently stops running
//! (or one that starts running because a capability was declared) fails here.

use std::sync::Arc;

use dcp_conformance::wrappers::{Defect, Defective};
use dcp_conformance::{journey_names, run_one, Outcome};
use dcp_core::DocumentStore;
use dcp_interop::factories::{InteropFactory, Stack};
use dcp_interop::profile::{self, control_settle, expected_failure, run_all_timed};
use dcp_interop::rig::synapse;
use dcp_interop::{RigOptions, Who};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_journey_passes_on_matrix() {
    let Some(syn) = synapse() else { return };
    let report = run_all_timed(&InteropFactory::new(syn.clone(), Stack::Matrix)).await;
    println!("{report}");
    profile::judge_report(&syn, "matrix", &report);
    // MatrixStore claims every capability: nothing is skipped.
    report.assert_skipped_exactly(&[]);
    let expected: Vec<&str> =
        journey_names().into_iter().filter(|n| expected_failure(&syn, &format!("matrix/{n}")).is_none()).collect();
    assert_eq!(report.passed(), expected, "every journey, in spec order, must pass (but those recorded as failing on the default profile)");
    assert_eq!(journey_names().len(), 11);
}

/// What WOPI cannot express, and why (docs/rfc/0004-spike-d-results.md,
/// "Conformance journeys over WOPI"). The same list as over the in-memory
/// store: the stack behind the host does not change what WOPI can carry.
const WOPI_SKIPPED: &[&str] = &[
    // A WOPI save has one parent; a merge needs two (spec 6.3).
    "second_head_shown_not_hidden",
    // The lease records carry holder, base_version and duration_ms; WOPI
    // carries only the lock string.
    "lease_exclusion_and_readback",
    "lease_expiry_releases",
    // No grant or revoke in WOPI.
    "revocation_hides_new_content",
    // No update stream in WOPI (L2).
    "unknown_algorithm_falls_back_to_versions",
    "update_stream_gap_recovery",
];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_journeys_wopi_can_express_pass_over_a_matrix_host() {
    let Some(syn) = synapse() else { return };
    let report = run_all_timed(&InteropFactory::new(syn.clone(), Stack::WopiOverMatrix)).await;
    println!("{report}");
    profile::judge_report(&syn, "wopi", &report);
    report.assert_skipped_exactly(WOPI_SKIPPED);
    let mut passed = report.passed();
    passed.sort_unstable();
    let mut expected: Vec<&str> = vec![
        "convergence_on_one_head",
        "corrupt_file_rejected",
        // The forger is a viewer on the Matrix room; every reader is a
        // WOPI client over the host.
        "forged_version_from_a_viewer_is_not_accepted",
        "roles_enforced_at_store",
        "truncated_upload_never_published",
    ];
    expected.retain(|n| expected_failure(&syn, &format!("wopi/{n}")).is_none());
    assert_eq!(passed, expected, "the journeys WOPI can express must pass (but those recorded as failing on the default profile)");
    // Every journey is accounted for: passed, failed (recorded) or skipped, none lost.
    assert_eq!(report.results.len(), journey_names().len());
}

/// The path must be able to fail: the viewer's backing store never shows the
/// newest version, so the viewer, reading through WOPI, never converges.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_journeys_can_fail_through_wopi_over_matrix() {
    let Some(syn) = synapse() else { return };
    let opts = RigOptions::honest().settle(control_settle(&syn)).backing(|who, store, _| {
        if who == Who::Viewer {
            Arc::new(Defective::new(store, Defect::HideNewest)) as Arc<dyn DocumentStore>
        } else {
            store
        }
    });
    let f = InteropFactory::new(syn, Stack::WopiOverMatrix).with_options(opts);
    let r = run_one(&f, "convergence_on_one_head").await;
    assert!(matches!(r.outcome, Outcome::Failed(_)), "{:?}", r.outcome);
}

/// The room has the literal draft-0 power-level table, so Synapse accepts a
/// viewer's forged version and only the readers' rules can drop it. Every
/// journey still passes, on both stacks: the Matrix readers drop it, and so
/// do WOPI readers over the host.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_forged_version_journey_passes_on_both_stacks_when_the_server_does_not_gate() {
    let Some(syn) = synapse() else { return };
    for stack in [Stack::Matrix, Stack::WopiOverMatrix] {
        let f = InteropFactory::new(syn.clone(), stack).with_options(RigOptions::honest().draft0_power_levels());
        let r = run_one(&f, "forged_version_from_a_viewer_is_not_accepted").await;
        assert_eq!(r.outcome, Outcome::Passed, "{stack:?}");
    }
}

/// The negative control for the above: the same draft-0 room, and readers
/// that trust the sender. The forgery becomes a second head and the journey
/// must fail, on both stacks.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_forged_version_journey_fails_when_neither_the_server_nor_the_readers_check() {
    let Some(syn) = synapse() else { return };
    for stack in [Stack::Matrix, Stack::WopiOverMatrix] {
        let opts = RigOptions::honest()
            .draft0_power_levels_and_readers_that_trust_senders()
            .settle(control_settle(&syn));
        let f = InteropFactory::new(syn.clone(), stack).with_options(opts);
        let r = run_one(&f, "forged_version_from_a_viewer_is_not_accepted").await;
        match &r.outcome {
            Outcome::Failed(why) => {
                println!("{stack:?}: failed as it must: {why}");
                assert!(why.0.contains("forged version was accepted"), "{stack:?}: failed for another reason: {why}");
            }
            other => panic!("{stack:?}: the journey did not fail with an accepted forgery: {other:?}"),
        }
    }
}
