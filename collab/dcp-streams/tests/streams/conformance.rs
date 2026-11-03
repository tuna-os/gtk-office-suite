//! The spec section 12 journeys (`dcp-conformance`) through `StreamsStore`
//! against a real server. Every capability is true and the lease is a
//! compare-and-set (`Stream-Seq`), so every journey runs: nothing is skipped.

use std::sync::Arc;

use async_trait::async_trait;
use dcp_conformance::{
    run_all, Capabilities, Failure, Faults, Fixture, Forger, RealTime, StoreFactory,
};
use dcp_core::{DocId, Version};
use dcp_streams::wire::{Inner, Kind};
use dcp_streams::{Identity, StreamsConfig, StreamsStore};

use crate::common::*;
use crate::require_streams;

pub struct StreamsFactory {
    pub base: String,
    pub label: String,
    /// Negative control: every handle accepts records whatever their author's role.
    pub lenient: bool,
}

struct CutUploads(Arc<StreamsStore>);

impl Faults for CutUploads {
    fn truncate_next_upload(&self) {
        self.0.truncate_next_upload();
    }
}

struct ViewerForger(Arc<StreamsStore>);

#[async_trait]
impl Forger for ViewerForger {
    async fn forge_version(
        &self,
        doc: &DocId,
        version: Version,
    ) -> Result<Option<String>, Failure> {
        let st = self.0.http().clock.estimate().unwrap_or(0);
        let inner = serde_json::to_value(Inner {
            st,
            dev: None,
            body: version,
            hint: None,
        })
        .map_err(|e| Failure::new(format!("forged version: {e}")))?;
        self.0
            .send_unchecked(doc, Kind::Ver, &inner)
            .await
            .map_err(|e| Failure::new(format!("the forged send failed unexpectedly: {e}")))
    }
}

#[async_trait]
impl StoreFactory for StreamsFactory {
    fn name(&self) -> String {
        format!("durable-streams ({})", self.label)
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            grant_revoke: true,
            revocation: true,
            arbitrary_lease_writes: true,
            // Stream-Seq is an append compare-and-set: the second of two
            // writers over the same record is refused (409).
            racy_lease: false,
            history: true,
            arbitrary_parents: true,
            updates: true,
            upload_faults: true,
            forgery: true,
        }
    }

    async fn fresh(&self) -> Result<Fixture, Failure> {
        let mk = |device: &str| {
            let mut cfg = StreamsConfig::new(device);
            cfg.lenient_roles = self.lenient;
            Arc::new(StreamsStore::new(
                crate::common::http(&self.base),
                Identity::generate(),
                cfg,
            ))
        };
        let (owner, editor, viewer) = (mk("OWNER-DEV"), mk("EDITOR-DEV"), mk("VIEWER-DEV"));
        // Each handle pins its own model of the server clock (a second each,
        // in parallel).
        let (a, b, c) = tokio::join!(owner.calibrate(), editor.calibrate(), viewer.calibrate());
        a?;
        b?;
        c?;
        let mut fx = Fixture::standard(
            dynamic(&owner),
            dynamic(&editor),
            dynamic(&viewer),
            Arc::new(RealTime),
            self.capabilities(),
            Some(Arc::new(CutUploads(editor.clone()))),
            SETTLE,
        )
        .await?;
        fx.forger = Some(Arc::new(ViewerForger(viewer.clone())));
        Ok(fx)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spec_section_12_journeys_on_a_durable_streams_server() {
    let base = require_streams!();
    let factory = StreamsFactory {
        base,
        label: std::env::var("DCP_STREAMS_LABEL").unwrap_or_else(|_| "reference server".into()),
        lenient: false,
    };
    let report = run_all(&factory).await;
    println!("{report}");
    report.assert_all_passed();
}

/// Negative control: when no reader checks the role of an author (and the
/// server, having no access control, accepts the viewer's forgery), the forged
/// version becomes a head and the journey must fail. It proves the journey can
/// fail, and that the readers' rule is what held in the run above.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_forged_version_journey_fails_when_no_reader_checks_roles() {
    let base = require_streams!();
    let factory = StreamsFactory {
        base,
        label: "readers do not check roles".into(),
        lenient: true,
    };
    let result =
        dcp_conformance::run_one(&factory, "forged_version_from_a_viewer_is_not_accepted").await;
    println!("{result:?}");
    assert!(
        matches!(result.outcome, dcp_conformance::Outcome::Failed(_)),
        "the negative control must fail: {:?}",
        result.outcome
    );
}
