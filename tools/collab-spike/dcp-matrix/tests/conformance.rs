//! Spec section 12 journeys (`dcp-conformance`) against the Matrix binding on a
//! real Synapse. Needs `DCP_SYNAPSE_URL`; fails without it unless
//! `DCP_SKIP_SYNAPSE=1`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use common::proxy::FaultProxy;
use common::*;
use dcp_conformance::{run_all, Capabilities, Failure, Fixture, RealTime, StoreFactory};
use dcp_core::DocumentStore;

struct MatrixFactory {
    syn: Synapse,
}

#[async_trait]
impl StoreFactory for MatrixFactory {
    fn name(&self) -> String {
        format!("matrix-synapse ({} rate limits)", self.syn.profile)
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
        }
    }

    async fn fresh(&self) -> Result<Fixture, Failure> {
        let host = self.syn.url.trim_start_matches("http://").trim_end_matches('/').to_owned();
        let proxy = Arc::new(FaultProxy::start(&host).await);
        let url = proxy.url.clone();
        let mk = |prefix: &'static str| {
            let syn = self.syn.clone();
            let url = url.clone();
            async move { syn.user_with(prefix, move |c| c.homeserver = url).await }
        };
        let (owner, editor, viewer) = (mk("owner").await, mk("editor").await, mk("viewer").await);
        Fixture::standard(
            Arc::new(owner) as Arc<dyn DocumentStore>,
            Arc::new(editor),
            Arc::new(viewer),
            Arc::new(RealTime),
            self.capabilities(),
            Some(proxy),
            Duration::from_secs(30),
        )
        .await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spec_section_12_journeys_on_synapse() {
    let Some(syn) = require_synapse!() else { return };
    let factory = MatrixFactory { syn };
    let report = run_all(&factory).await;
    println!("{report}");
    report.assert_all_passed();
}
