//! `dcp-conformance` factories for the two stacks on one Synapse.

use std::sync::Arc;

use async_trait::async_trait;
use dcp_conformance::{Capabilities, Failure, Fixture, RealTime, StoreFactory};
use dcp_core::DocumentStore;

use crate::common::Synapse;
use crate::rig::{Rig, RigOptions, Who};

/// The journeys of spec section 12 are run against one of these stacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stack {
    /// `MatrixStore` alone: every handle is a Matrix user.
    Matrix,
    /// `WopiClientStore -> HTTP -> WopiHost -> MatrixStore`: every handle is a
    /// WOPI client simulator, and the host behind it stores in Matrix. Roles
    /// and membership are set up natively, because WOPI has no way to.
    WopiOverMatrix,
}

pub struct InteropFactory {
    pub syn: Synapse,
    pub stack: Stack,
    pub opts: RigOptions,
}

impl InteropFactory {
    pub fn new(syn: Synapse, stack: Stack) -> Self {
        InteropFactory { syn, stack, opts: RigOptions::honest() }
    }

    pub fn with_options(mut self, opts: RigOptions) -> Self {
        self.opts = opts;
        self
    }
}

#[async_trait]
impl StoreFactory for InteropFactory {
    fn name(&self) -> String {
        match self.stack {
            Stack::Matrix => format!("matrix-synapse ({} rate limits)", self.syn.profile),
            Stack::WopiOverMatrix => {
                format!("wopi (simulator -> WopiHost -> MatrixStore, {} rate limits)", self.syn.profile)
            }
        }
    }

    fn capabilities(&self) -> Capabilities {
        match self.stack {
            // As spike A's own conformance test declares it.
            Stack::Matrix => Capabilities {
                grant_revoke: true,
                revocation: true,
                arbitrary_lease_writes: true,
                // State events have no compare-and-set (spec 8.1, lease rule 3).
                racy_lease: true,
                history: true,
                arbitrary_parents: true,
                updates: true,
                upload_faults: true,
            },
            // As spike D's own conformance test declares it for WOPI over
            // memory; the stack behind the host does not change what WOPI can
            // express.
            Stack::WopiOverMatrix => Capabilities {
                grant_revoke: false,
                revocation: false,
                arbitrary_lease_writes: false,
                racy_lease: false,
                history: true,
                arbitrary_parents: false,
                updates: false,
                upload_faults: true,
            },
        }
    }

    async fn fresh(&self) -> Result<Fixture, Failure> {
        let rig = Rig::new(&self.syn, self.opts).await?;
        let handles: [Arc<dyn DocumentStore>; 3] = match self.stack {
            Stack::Matrix => Who::ALL.map(|w| rig.native(w).clone()),
            Stack::WopiOverMatrix => Who::ALL.map(|w| Arc::new(rig.sim(w)) as Arc<dyn DocumentStore>),
        };
        let [owner, editor, viewer] = handles;
        Fixture::from_existing(
            owner,
            editor,
            viewer,
            rig.doc.clone(),
            Arc::new(RealTime),
            self.capabilities(),
            Some(rig.proxy.clone()),
            rig.settle,
        )
        .await
    }
}
