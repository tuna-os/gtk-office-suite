//! The suite must be able to fail. Each test here runs a journey against a
//! store that has ONE deliberate defect and asserts that journey fails; the
//! first tests assert the whole suite passes on the correct stores, so a
//! failure below is attributable to the defect and not to the harness.

use std::sync::Arc;

use async_trait::async_trait;
use dcp_conformance::wrappers::{CorruptingStore, Corruption, Defect, Defective, Target};
use dcp_conformance::{
    journey_names, run_all, run_one, run_selected, Capabilities, Failure, Fixture, MemoryFactory,
    Outcome, StoreFactory,
};
use dcp_core::memory::MemoryConfig;
use dcp_core::DocumentStore;

type Handle = Arc<dyn DocumentStore>;

fn wrap(h: &Handle, defect: Defect) -> Handle {
    Arc::new(Defective::new(h.clone(), defect))
}

/// The memory reference store with a defect injected into some handles.
struct Mutant {
    label: &'static str,
    base: MemoryFactory,
    apply: Box<dyn Fn(&mut Fixture) + Send + Sync>,
}

impl Mutant {
    fn new(label: &'static str, apply: impl Fn(&mut Fixture) + Send + Sync + 'static) -> Self {
        Mutant {
            label,
            base: MemoryFactory::new(),
            apply: Box::new(apply),
        }
    }

    fn on(mut self, base: MemoryFactory) -> Self {
        self.base = base;
        self
    }
}

#[async_trait]
impl StoreFactory for Mutant {
    fn name(&self) -> String {
        format!("memory + defect: {}", self.label)
    }
    fn capabilities(&self) -> Capabilities {
        self.base.capabilities()
    }
    async fn fresh(&self) -> Result<Fixture, Failure> {
        let mut fx = self.base.fresh().await?;
        (self.apply)(&mut fx);
        Ok(fx)
    }
}

async fn assert_caught(factory: &dyn StoreFactory, journey: &str) {
    let r = run_one(factory, journey).await;
    match r.outcome {
        Outcome::Failed(why) => eprintln!("caught: {journey} on {}: {why}", factory.name()),
        other => panic!(
            "{journey} did NOT fail on {} (got {other:?}): the suite cannot see this defect",
            factory.name()
        ),
    }
}

// ---- the correct stores pass the whole suite ----------------------------

#[tokio::test]
async fn whole_suite_passes_on_the_memory_store() {
    let report = run_all(&MemoryFactory::new()).await;
    report.assert_all_passed();
    assert_eq!(report.results.len(), 11);
}

#[tokio::test]
async fn whole_suite_passes_when_the_server_does_not_gate_and_the_reader_does() {
    // The draft-0 power-level table: the server accepts a viewer's forged
    // version, the reader drops it.
    let report = run_all(&MemoryFactory::ungated()).await;
    report.assert_all_passed();
}

#[tokio::test]
async fn whole_suite_passes_on_the_memory_store_with_racy_leases() {
    // Two lease writers both succeed; only the read-back resolves it.
    let report = run_all(&MemoryFactory::racy()).await;
    report.assert_all_passed();
}

#[tokio::test]
async fn every_journey_is_individually_callable_by_name() {
    let f = MemoryFactory::new();
    for name in journey_names() {
        let r = run_one(&f, name).await;
        assert_eq!(r.outcome, Outcome::Passed, "{name}");
    }
}

// ---- reports: skipped is never passed -----------------------------------

struct Limited {
    inner: MemoryFactory,
    caps: Capabilities,
}

#[async_trait]
impl StoreFactory for Limited {
    fn name(&self) -> String {
        "memory-limited".into()
    }
    fn capabilities(&self) -> Capabilities {
        self.caps
    }
    async fn fresh(&self) -> Result<Fixture, Failure> {
        let mut fx = self.inner.fresh().await?;
        fx.caps = self.caps;
        Ok(fx)
    }
}

#[tokio::test]
async fn a_missing_capability_is_reported_as_skipped_not_passed() {
    let f = Limited {
        inner: MemoryFactory::new(),
        caps: Capabilities {
            updates: false,
            revocation: false,
            ..Capabilities::full()
        },
    };
    let report = run_all(&f).await;
    report.assert_no_failures();
    report.assert_skipped_exactly(&[
        "revocation_hides_new_content",
        "unknown_algorithm_falls_back_to_versions",
        "update_stream_gap_recovery",
    ]);
    assert_eq!(report.passed().len(), 8);
    assert!(
        matches!(&report.get("update_stream_gap_recovery").unwrap().outcome, Outcome::Skipped { missing } if missing == &["updates"])
    );
}

#[test]
#[should_panic(expected = "skipped journeys differ")]
fn declaring_the_wrong_skips_fails() {
    let report = dcp_conformance::Report {
        binding: "x".into(),
        results: vec![dcp_conformance::JourneyResult {
            name: "revocation_hides_new_content",
            spec_test: "7",
            outcome: Outcome::Skipped {
                missing: vec!["revocation"],
            },
        }],
    };
    report.assert_skipped_exactly(&[]);
}

#[tokio::test]
#[should_panic(expected = "no journey named")]
async fn a_misspelled_journey_name_panics_instead_of_selecting_nothing() {
    run_selected(&MemoryFactory::new(), &["convergance_on_one_head"]).await;
}

// ---- one defect, one journey that must notice ---------------------------

#[tokio::test]
async fn store_that_ignores_roles_fails_roles_enforced_at_store() {
    let f = Mutant::new("ignores roles", |fx| {
        fx.viewer = wrap(
            &fx.viewer,
            Defect::IgnoreRoles {
                privileged: fx.owner.clone(),
            },
        );
    });
    assert_caught(&f, "roles_enforced_at_store").await;
}

#[tokio::test]
async fn store_that_skips_the_upload_hash_check_fails_truncated_upload_never_published() {
    let f = Mutant::new("skips the hash check of uploads", |_| {}).on(MemoryFactory::with_config(
        MemoryConfig {
            verify_uploads: false,
            ..MemoryConfig::default()
        },
    ));
    assert_caught(&f, "truncated_upload_never_published").await;
}

#[tokio::test]
async fn lease_write_without_read_back_fails_lease_exclusion_on_a_cas_store() {
    let f = Mutant::new("lease write without read-back (cas backend)", |fx| {
        fx.editor = wrap(&fx.editor, Defect::LeaseEcho);
        fx.owner = wrap(&fx.owner, Defect::LeaseEcho);
    });
    assert_caught(&f, "lease_exclusion_and_readback").await;
}

#[tokio::test]
async fn lease_write_without_read_back_fails_lease_exclusion_on_a_racy_store() {
    let f = Mutant::new("lease write without read-back (racy backend)", |fx| {
        fx.editor = wrap(&fx.editor, Defect::LeaseEcho);
        fx.owner = wrap(&fx.owner, Defect::LeaseEcho);
    })
    .on(MemoryFactory::racy());
    assert_caught(&f, "lease_exclusion_and_readback").await;
}

#[tokio::test]
async fn revoke_that_is_a_noop_fails_revocation() {
    let f = Mutant::new("revoke is a no-op", |fx| {
        fx.owner = wrap(&fx.owner, Defect::RevokeIsNoop);
    });
    assert_caught(&f, "revocation_hides_new_content").await;
}

#[tokio::test]
async fn store_that_reorders_updates_fails_update_stream_gap_recovery() {
    let f = Mutant::new("reorders updates", |fx| {
        fx.viewer = wrap(&fx.viewer, Defect::ReorderUpdates);
    });
    assert_caught(&f, "update_stream_gap_recovery").await;
}

#[tokio::test]
async fn store_that_hides_a_sibling_head_fails_second_head_shown() {
    let f = Mutant::new("hides all but the newest head", |fx| {
        fx.viewer = wrap(&fx.viewer, Defect::HideSiblingHeads);
    });
    assert_caught(&f, "second_head_shown_not_hidden").await;
}

#[tokio::test]
async fn store_that_never_shows_the_newest_version_fails_convergence() {
    let f = Mutant::new("never shows the newest version", |fx| {
        fx.editor = wrap(&fx.editor, Defect::HideNewest);
    });
    assert_caught(&f, "convergence_on_one_head").await;
}

#[tokio::test]
async fn store_whose_clock_never_advances_fails_lease_expiry() {
    let f = Mutant::new("clock never advances", |fx| {
        fx.owner = wrap(&fx.owner, Defect::FrozenClock);
        fx.editor = wrap(&fx.editor, Defect::FrozenClock);
    });
    assert_caught(&f, "lease_expiry_releases").await;
}

#[tokio::test]
async fn store_that_relabels_unknown_algorithms_fails_the_fallback_journey() {
    let f = Mutant::new("relabels unknown algorithms", |fx| {
        fx.viewer = wrap(&fx.viewer, Defect::RelabelUpdates);
    });
    assert_caught(&f, "unknown_algorithm_falls_back_to_versions").await;
}

/// A defect must break the journey it targets and NOT be an accident of the
/// harness: the same defective fixture still passes journeys it cannot affect.
#[tokio::test]
async fn a_defect_does_not_fail_unrelated_journeys() {
    let f = Mutant::new("revoke is a no-op", |fx| {
        fx.owner = wrap(&fx.owner, Defect::RevokeIsNoop);
    });
    for name in [
        "convergence_on_one_head",
        "second_head_shown_not_hidden",
        "corrupt_file_rejected",
        "lease_expiry_releases",
    ] {
        assert_eq!(run_one(&f, name).await.outcome, Outcome::Passed, "{name}");
    }
}

#[tokio::test]
async fn store_that_serves_damaged_files_fails_the_corruption_journey() {
    // Journey 3 reads through the reader pipeline, which verifies size and
    // sha256: a store whose files do not match its own references is caught,
    // whatever the damage.
    for corruption in Corruption::ALL {
        let f = Mutant::new("serves damaged files", move |fx| {
            fx.editor = Arc::new(CorruptingStore::new(
                fx.editor.clone(),
                corruption,
                Target::Every,
            ));
        });
        assert_caught(&f, "corrupt_file_rejected").await;
    }
}

// ---- the forged-version journey ------------------------------------------

#[tokio::test]
async fn a_reader_that_does_not_check_senders_fails_the_forged_version_journey() {
    // The server does not gate (draft-0 table) and the reader does not drop
    // the viewer's version either: the forgery becomes a second head.
    let f = MemoryFactory::with_config(MemoryConfig {
        gates_timeline: false,
        reader_checks_senders: false,
        ..MemoryConfig::default()
    });
    assert_caught(&f, "forged_version_from_a_viewer_is_not_accepted").await;
}

#[tokio::test]
async fn a_gated_server_passes_the_forged_version_journey_by_refusing() {
    // Same journey, the other way to pass: the gate refuses both forgeries.
    let f = MemoryFactory::new();
    assert_eq!(
        run_one(&f, "forged_version_from_a_viewer_is_not_accepted").await.outcome,
        Outcome::Passed
    );
    let fx = f.fresh().await.unwrap();
    let base = fx.owner.versions(&fx.doc).await.unwrap().remove(0);
    let forged = dcp_core::Version {
        dcp: 1,
        parents: vec![fx.first_version.clone()],
        file: base.body.file.clone(),
        created_by: dcp_core::Author {
            principal: fx.viewer.principal(),
            device: fx.viewer.device(),
        },
        message: None,
        covers: None,
    };
    assert_eq!(fx.forger.unwrap().forge_version(&fx.doc, forged).await.unwrap(), None);
}
