//! Spec section 12 journeys (`dcp-conformance`) against the peer-to-peer binding: three iroh
//! nodes on loopback, no relay, no external service.

mod common;

use common::*;
use dcp_conformance::{run_all, run_one, Outcome, StoreFactory};

const FORGED: &str = "forged_version_from_a_viewer_is_not_accepted";

/// Every journey runs, and none is skipped: this binding declares every capability.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spec_section_12_journeys_on_iroh_and_p2panda() {
    init_logging();
    let factory = P2pFactory::new();
    let report = run_all(&factory).await;
    report.assert_no_failures();
    report.assert_skipped_exactly(&[]);
    assert_eq!(report.passed().len(), report.results.len());
}

/// Negative control: with the readers' authorisation rule off, the forged-version journey must
/// fail. It proves the journey can fail and that this rule (not the transport) is what holds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forged_version_journey_fails_when_readers_do_not_authorise() {
    init_logging();
    let factory = P2pFactory::new().with_all(|c| c.authorize = false);
    let r = run_one(&factory, FORGED).await;
    assert!(
        matches!(r.outcome, Outcome::Failed(_)),
        "the forged-version journey should fail without reader authorisation, got {:?}",
        r.outcome
    );
    eprintln!("negative control: {:?}", r.outcome);
    let _ = factory.name();
}
