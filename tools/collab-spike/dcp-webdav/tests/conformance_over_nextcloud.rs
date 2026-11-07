//! The shared conformance journeys (spec section 12, `dcp-conformance`) run
//! against a real Nextcloud: `WebdavStore` over WebDAV and OCS sharing, three
//! accounts (alice owner, bob editor, carol viewer) on one instance.
//!
//! Capabilities are what the binding really does; the journeys that need a
//! capability it lacks are SKIPPED and pinned by `assert_skipped_exactly`, and
//! `docs/rfc/0004-spike-e-results.md` explains each. A test that needs Nextcloud
//! fails when it is missing unless `DCP_SKIP_NEXTCLOUD=1`.

mod common;

use common::*;
use dcp_conformance::{run_all, run_one, Outcome};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn journeys_over_nextcloud() {
    if require("journeys_over_nextcloud", &[url_a()]).await.is_none() {
        return;
    }
    let report = run_all(&NcFactory::single(false).await).await;
    println!("{report}");
    report.assert_no_failures();
    report.assert_skipped_exactly(&[
        // WebDAV has no ordered stream of small messages: L2 is not offered.
        "unknown_algorithm_falls_back_to_versions",
        "update_stream_gap_recovery",
    ]);
    let mut passed = report.passed();
    passed.sort_unstable();
    assert_eq!(
        passed,
        [
            "convergence_on_one_head",
            "corrupt_file_rejected",
            "forged_version_from_a_viewer_is_not_accepted",
            "lease_exclusion_and_readback",
            "lease_expiry_releases",
            "revocation_hides_new_content",
            "roles_enforced_at_store",
            "second_head_shown_not_hidden",
            "truncated_upload_never_published",
        ]
    );
}

/// Negative control: a viewer's handle that never shows the newest version
/// must fail convergence over Nextcloud as it does everywhere. If it passed,
/// the journeys would not be reading what they claim to read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_journeys_can_fail_over_nextcloud() {
    if require("the_journeys_can_fail_over_nextcloud", &[url_a()]).await.is_none() {
        return;
    }
    let r = run_one(&NcFactory::single(true).await, "convergence_on_one_head").await;
    assert!(matches!(r.outcome, Outcome::Failed(_)), "{:?}", r.outcome);
}

/// The skipped journeys say why they are skipped.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skipped_journeys_say_why() {
    if require("skipped_journeys_say_why", &[url_a()]).await.is_none() {
        return;
    }
    let f = NcFactory::single(false).await;
    for name in ["unknown_algorithm_falls_back_to_versions", "update_stream_gap_recovery"] {
        let r = run_one(&f, name).await;
        match r.outcome {
            Outcome::Skipped { missing } => assert_eq!(missing, ["updates"], "{name}"),
            other => panic!("{name}: {other:?}"),
        }
    }
}
