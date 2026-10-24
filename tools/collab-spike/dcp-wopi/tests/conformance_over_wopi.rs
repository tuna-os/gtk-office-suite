//! The shared conformance journeys (spec section 12, `dcp-conformance`) run
//! through WOPI: `WopiClientStore` -> HTTP -> `WopiHost` -> `MemoryStore`.
//!
//! The simulator states what WOPI cannot express through `Capabilities`. A
//! journey that needs a missing capability is SKIPPED and reported as such;
//! `assert_skipped_exactly` pins the list so a skip cannot appear or vanish
//! unnoticed, and `docs/rfc/0004-spike-d-results.md` explains each one.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dcp_conformance::wrappers::{Defect, Defective};
use dcp_conformance::{
    new_file, run_all, run_one, standard_meta, Capabilities, Failure, Faults, Fixture, Outcome,
    StoreFactory, Time, BASE_FILE,
};
use dcp_core::memory::{ManualClock, MemoryBackend};
use dcp_core::{DocumentStore, Role};
use dcp_wopi::{HostConfig, WopiClientStore, WopiHost};

struct ManualTime(Arc<ManualClock>);

#[async_trait]
impl Time for ManualTime {
    async fn pass(&self, ms: u64) {
        self.0.advance(ms);
    }
}

struct MemoryFaults(MemoryBackend);

impl Faults for MemoryFaults {
    fn truncate_next_upload(&self) {
        self.0.truncate_next_upload();
    }
}

/// How the backing handle behind the viewer's access token is built.
#[derive(Clone, Copy)]
enum ViewerBacking {
    Honest,
    /// A defective backing store that never shows the newest version.
    HideNewest,
}

struct WopiFactory {
    viewer_backing: ViewerBacking,
}

impl WopiFactory {
    fn new() -> Self {
        WopiFactory { viewer_backing: ViewerBacking::Honest }
    }
}

#[async_trait]
impl StoreFactory for WopiFactory {
    fn name(&self) -> String {
        match self.viewer_backing {
            ViewerBacking::Honest => "wopi (simulator -> WopiHost -> memory)".into(),
            ViewerBacking::HideNewest => "wopi with a defective backing store".into(),
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // WOPI has no permission-management call.
            grant_revoke: false,
            revocation: false,
            // The lock string is all WOPI carries of a lease: holder, base
            // version and duration cannot be written, so the lease journeys,
            // which assert them, cannot run. Lock behaviour is tested in
            // tests/host_semantics.rs and tests/client_store.rs.
            arbitrary_lease_writes: false,
            racy_lease: false,
            // Through the host's extensions (DcpVersions, ?dcp_version=).
            history: true,
            // A WOPI save has exactly one parent: merges are inexpressible.
            arbitrary_parents: false,
            updates: false,
            upload_faults: true,
        }
    }

    async fn fresh(&self) -> Result<Fixture, Failure> {
        let clock = Arc::new(ManualClock::new(1_000_000));
        let backend = MemoryBackend::new(clock.clone());
        let owner_m: Arc<dyn DocumentStore> = Arc::new(backend.store("@owner:mem", "OWNER-DEV"));
        let editor_m: Arc<dyn DocumentStore> = Arc::new(backend.store("@editor:mem", "EDITOR-DEV"));
        let viewer_m: Arc<dyn DocumentStore> = Arc::new(backend.store("@viewer:mem", "VIEWER-DEV"));

        // WOPI cannot grant, so the backing store sets the roles up.
        let doc = owner_m.create_document(&standard_meta(), Some(new_file(BASE_FILE))).await?;
        owner_m.grant(&doc, "@editor:mem", Role::Editor).await?;
        owner_m.grant(&doc, "@viewer:mem", Role::Viewer).await?;

        // The fixture's files are opaque bytes, not ODF packages.
        let host = WopiHost::new(HostConfig {
            validate_packages: false,
            log_stderr: false,
            ..HostConfig::default()
        });
        let server = host
            .serve("127.0.0.1:0".parse().unwrap())
            .await
            .map_err(|e| Failure::new(format!("cannot start the WOPI host: {e}")))?;

        let viewer_backing: Arc<dyn DocumentStore> = match self.viewer_backing {
            ViewerBacking::Honest => viewer_m,
            ViewerBacking::HideNewest => Arc::new(Defective::new(viewer_m, Defect::HideNewest)),
        };
        let mut sims: Vec<Arc<dyn DocumentStore>> = Vec::new();
        for (principal, device, backing) in [
            ("@owner:mem", "OWNER-DEV", owner_m),
            ("@editor:mem", "EDITOR-DEV", editor_m),
            ("@viewer:mem", "VIEWER-DEV", viewer_backing),
        ] {
            let sim = WopiClientStore::new(server.base_url(), principal, device);
            sim.attach(doc.clone(), host.register(doc.clone(), backing, principal));
            sims.push(Arc::new(sim));
        }
        let (viewer, editor, owner) = (sims.pop().unwrap(), sims.pop().unwrap(), sims.pop().unwrap());
        Fixture::from_existing(
            owner,
            editor,
            viewer,
            doc,
            Arc::new(ManualTime(clock)),
            self.capabilities(),
            Some(Arc::new(MemoryFaults(backend))),
            Duration::ZERO,
        )
        .await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn journeys_over_wopi() {
    let report = run_all(&WopiFactory::new()).await;
    report.assert_no_failures();
    report.assert_skipped_exactly(&[
        // WOPI saves have one parent; a merge needs two (spec 6.3).
        "second_head_shown_not_hidden",
        // Lease records carry holder, base_version and duration_ms.
        "lease_exclusion_and_readback",
        "lease_expiry_releases",
        // No grant or revoke in WOPI.
        "revocation_hides_new_content",
        // No update stream in WOPI (L2).
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
            "roles_enforced_at_store",
            "truncated_upload_never_published"
        ]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_journeys_can_fail_through_wopi() {
    // A backing store that hides the newest version: the viewer, reading
    // through WOPI, never converges. If this passed, the WOPI path would be
    // hiding what the store does wrong.
    let f = WopiFactory { viewer_backing: ViewerBacking::HideNewest };
    let r = run_one(&f, "convergence_on_one_head").await;
    assert!(matches!(r.outcome, Outcome::Failed(_)), "{:?}", r.outcome);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skipped_journeys_say_why() {
    let f = WopiFactory::new();
    for name in ["lease_exclusion_and_readback", "revocation_hides_new_content", "update_stream_gap_recovery"] {
        let r = run_one(&f, name).await;
        assert!(matches!(r.outcome, Outcome::Skipped { .. }), "{name}: {:?}", r.outcome);
    }
}
