//! Spec section 12 journeys (`dcp-conformance`) against the Matrix binding on a
//! real Synapse. Needs `DCP_SYNAPSE_URL`; fails without it unless
//! `DCP_SKIP_SYNAPSE=1`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use common::proxy::FaultProxy;
use common::*;
use dcp_conformance::{
    run_all, run_one, Capabilities, Failure, Fixture, Forger, Outcome, RealTime, StoreFactory,
};
use dcp_core::{DocId, DocumentStore, StoreError, Version};
use dcp_matrix::{MatrixConfig, MatrixStore};

const FORGED_JOURNEY: &str = "forged_version_from_a_viewer_is_not_accepted";

struct MatrixFactory {
    syn: Synapse,
    /// Applied to every user's config (the spec's literal power-level table,
    /// a reader without the sender rule).
    tweak: fn(&mut MatrixConfig),
    label: &'static str,
}

impl MatrixFactory {
    fn new(syn: Synapse) -> Self {
        MatrixFactory { syn, tweak: |_| {}, label: "" }
    }
}

/// The viewer's client sending a version around the store: straight to the
/// homeserver, with no role check (`MatrixStore::send_unchecked`).
struct ViewerForger {
    viewer: MatrixStore,
}

#[async_trait]
impl Forger for ViewerForger {
    async fn forge_version(&self, doc: &DocId, version: Version) -> Result<Option<String>, Failure> {
        let content = serde_json::to_value(&version).map_err(|e| Failure::new(format!("forged version: {e}")))?;
        match self.viewer.send_unchecked(doc, "org.tunaos.dcp.version", content).await {
            Ok(id) => Ok(Some(id)),
            Err(StoreError::Forbidden(_)) => Ok(None),
            Err(e) => Err(Failure::new(format!("forged send failed unexpectedly: {e}"))),
        }
    }
}

#[async_trait]
impl StoreFactory for MatrixFactory {
    fn name(&self) -> String {
        format!("matrix-synapse ({} rate limits){}", self.syn.profile, self.label)
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            grant_revoke: true,
            revocation: true,
            arbitrary_lease_writes: true,
            // State events have no compare-and-set (spec 8.1, lease rule 3).
            racy_lease: true,
            history: true,
            arbitrary_parents: true,
            updates: true,
            upload_faults: true,
            forgery: true,
        }
    }

    async fn fresh(&self) -> Result<Fixture, Failure> {
        let host = self.syn.url.trim_start_matches("http://").trim_end_matches('/').to_owned();
        let proxy = Arc::new(FaultProxy::start(&host).await);
        let url = proxy.url.clone();
        let mk = |prefix: &'static str| {
            let syn = self.syn.clone();
            let url = url.clone();
            let tweak = self.tweak;
            async move {
                syn.user_with(prefix, move |c| {
                    c.homeserver = url;
                    tweak(c);
                })
                .await
            }
        };
        let (owner, editor, viewer) = (mk("owner").await, mk("editor").await, mk("viewer").await);
        let forger = ViewerForger { viewer: viewer.clone() };
        let mut fx = Fixture::standard(
            Arc::new(owner) as Arc<dyn DocumentStore>,
            Arc::new(editor),
            Arc::new(viewer),
            Arc::new(RealTime),
            self.capabilities(),
            Some(proxy),
            Duration::from_secs(30),
        )
        .await?;
        fx.forger = Some(Arc::new(forger));
        Ok(fx)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spec_section_12_journeys_on_synapse() {
    let Some(syn) = require_synapse!() else { return };
    let factory = MatrixFactory::new(syn);
    let report = run_all(&factory).await;
    println!("{report}");
    report.assert_all_passed();
}

/// The spec's table taken literally (no `m.room.encrypted` entry): the server
/// accepts the viewer's forged event and only the reader's rules drop it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spec_section_12_journeys_with_a_literal_draft0_power_level_table() {
    let Some(syn) = require_synapse!() else { return };
    let factory = MatrixFactory {
        syn,
        tweak: |c| c.enforce_encrypted_pl = false,
        label: ", no m.room.encrypted power level",
    };
    let report = run_all(&factory).await;
    println!("{report}");
    report.assert_all_passed();
}

/// Negative control: with the server not gating and the readers not checking
/// senders, the forged version must get through, so the journey must fail.
/// Proves the journey can fail, and that the readers' rule is what held above.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forged_version_journey_fails_when_neither_server_nor_reader_gates() {
    let Some(syn) = require_synapse!() else { return };
    let factory = MatrixFactory {
        syn,
        tweak: |c| {
            c.enforce_encrypted_pl = false;
            c.sender_rule = dcp_matrix::SenderRule::Trust;
        },
        label: ", no m.room.encrypted power level, readers do not check senders",
    };
    let result = run_one(&factory, FORGED_JOURNEY).await;
    println!("{result:?}");
    assert!(matches!(result.outcome, Outcome::Failed(_)), "the negative control must fail: {:?}", result.outcome);
}

/// The same journeys, in a draft-0 room (nothing gates the timeline), under each
/// of the other at-event sender rules (Spike Q15). The default rule (the
/// hybrid) runs in the two tests above.
macro_rules! journeys_under_the_rule {
    ($($test:ident => $rule:ident),+ $(,)?) => {
        $(
            #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
            async fn $test() {
                let Some(syn) = require_synapse!() else { return };
                let factory = MatrixFactory {
                    syn,
                    tweak: |c| {
                        c.enforce_encrypted_pl = false;
                        c.sender_rule = dcp_matrix::SenderRule::$rule;
                    },
                    label: concat!(", no m.room.encrypted power level, sender rule ", stringify!($rule)),
                };
                let report = run_all(&factory).await;
                println!("{report}");
                report.assert_all_passed();
            }
        )+
    };
}

journeys_under_the_rule!(
    spec_section_12_journeys_in_a_draft0_room_under_the_context_rule => Context,
    spec_section_12_journeys_in_a_draft0_room_under_the_sdk_context_rule => SdkContext,
    spec_section_12_journeys_in_a_draft0_room_under_the_timeline_walk_rule => TimelineWalk,
    spec_section_12_journeys_in_a_draft0_room_under_the_prev_content_walk_rule => PrevContentWalk,
);
