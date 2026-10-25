//! The shared conformance journeys over a FEDERATION: the owner is alice on
//! `nc-a`; the editor and the viewer are bob and carol on a different Nextcloud,
//! `nc-b`, and reach the document only through Open Cloud Mesh shares that alice
//! made to `bob@nc-b` and `carol@nc-b` and that each of them accepted on `nc-b`.
//! Every request of the editor and the viewer goes to `nc-b`, which proxies it to
//! `nc-a` with the share's secret.
//!
//! No journey is hand-edited for this: it is the same `run_all` and the same
//! store, pointed at a different server.

mod common;

use common::*;
use dcp_conformance::run_all;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn journeys_over_a_federation() {
    if require("journeys_over_a_federation", &[url_a(), url_b()]).await.is_none() {
        return;
    }
    let report = run_all(&NcFactory::federated().await).await;
    println!("{report}");
    report.assert_skipped_exactly(&[
        "unknown_algorithm_falls_back_to_versions",
        "update_stream_gap_recovery",
    ]);
    // Every journey passes, `lease_expiry_releases` included: the federated fixture sets
    // `Fixture::lease_ms` to NEXTCLOUD_LEASE_MS, because at the default 4 s two server hops per
    // operation do not fit the journey's clock (`tests/lease_timing.rs` measures both).
    report.assert_no_failures();
}
