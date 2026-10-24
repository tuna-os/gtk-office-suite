//! The shared section-12 journeys (`dcp-conformance`) against both stacks on
//! one real Synapse (relaxed rate limits): `MatrixStore`, and
//! `WopiClientStore -> WopiHost -> MatrixStore`. Needs `DCP_SYNAPSE_URL`; fails
//! without it unless `DCP_SKIP_SYNAPSE=1`.
//!
//! The skip lists are asserted exactly. A journey that silently stops running
//! (or one that starts running because a capability was declared) fails here.

use std::time::Duration;

use std::sync::Arc;

use dcp_conformance::wrappers::{Defect, Defective};
use dcp_conformance::{journey_names, run_all, run_one, Outcome};
use dcp_core::DocumentStore;
use dcp_interop::factories::{InteropFactory, Stack};
use dcp_interop::rig::synapse;
use dcp_interop::{RigOptions, Who};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_journey_passes_on_matrix() {
    let Some(syn) = synapse() else { return };
    let report = run_all(&InteropFactory::new(syn, Stack::Matrix)).await;
    println!("{report}");
    report.assert_no_failures();
    // MatrixStore claims every capability: nothing is skipped.
    report.assert_skipped_exactly(&[]);
    assert_eq!(report.passed(), journey_names(), "every journey, in spec order, must pass");
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
    let report = run_all(&InteropFactory::new(syn, Stack::WopiOverMatrix)).await;
    println!("{report}");
    report.assert_no_failures();
    report.assert_skipped_exactly(WOPI_SKIPPED);
    let mut passed = report.passed();
    passed.sort_unstable();
    assert_eq!(
        passed,
        [
            "convergence_on_one_head",
            "corrupt_file_rejected",
            "roles_enforced_at_store",
            "truncated_upload_never_published"
        ]
    );
    // Every journey is accounted for: passed or skipped, none lost.
    assert_eq!(report.passed().len() + report.skipped().len(), journey_names().len());
}

/// The path must be able to fail: the viewer's backing store never shows the
/// newest version, so the viewer, reading through WOPI, never converges.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_journeys_can_fail_through_wopi_over_matrix() {
    let Some(syn) = synapse() else { return };
    let opts = RigOptions::honest().settle(Duration::from_secs(8)).backing(|who, store, _| {
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
