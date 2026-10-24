//! The reference binding: [`MemoryFactory`] runs the journeys against
//! `dcp_core::memory`, with a manual clock so lease expiry is instant.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dcp_core::memory::{ManualClock, MemoryBackend, MemoryConfig};

use crate::{Capabilities, Failure, Faults, Fixture, Forger, StoreFactory, Time};

pub const START_MS: u64 = 1_000_000;

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

/// Acts as the viewer: sends versions to the in-memory "server" around the
/// store ([`MemoryBackend::send_version_unchecked`]).
pub struct MemoryForger {
    pub backend: MemoryBackend,
    pub principal: String,
}

#[async_trait]
impl Forger for MemoryForger {
    async fn forge_version(
        &self,
        doc: &dcp_core::DocId,
        version: dcp_core::Version,
    ) -> Result<Option<String>, Failure> {
        match self.backend.send_version_unchecked(&self.principal, doc, version) {
            Ok(v) => Ok(Some(v.id)),
            Err(dcp_core::StoreError::Forbidden(_)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

pub struct MemoryFactory {
    config: MemoryConfig,
}

impl MemoryFactory {
    /// Compare-and-set leases, verified uploads: the correct reference store.
    pub fn new() -> Self {
        MemoryFactory {
            config: MemoryConfig::default(),
        }
    }

    /// The same, but `write_lease` lets two writers both succeed (last wins),
    /// as Matrix state events do.
    pub fn racy() -> Self {
        MemoryFactory {
            config: MemoryConfig {
                cas_leases: false,
                ..MemoryConfig::default()
            },
        }
    }

    /// The server does not gate the timeline (the draft-0 power-level table):
    /// a viewer's forged version is accepted, and only the reader's rules
    /// can drop it.
    pub fn ungated() -> Self {
        MemoryFactory {
            config: MemoryConfig {
                gates_timeline: false,
                ..MemoryConfig::default()
            },
        }
    }

    pub fn with_config(config: MemoryConfig) -> Self {
        MemoryFactory { config }
    }
}

impl Default for MemoryFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StoreFactory for MemoryFactory {
    fn name(&self) -> String {
        match (self.config.cas_leases, self.config.gates_timeline) {
            (true, true) => "memory".into(),
            (false, true) => "memory-racy-lease".into(),
            (true, false) => "memory-ungated-timeline".into(),
            (false, false) => "memory-racy-lease-ungated-timeline".into(),
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            racy_lease: !self.config.cas_leases,
            ..Capabilities::full()
        }
    }

    async fn fresh(&self) -> Result<Fixture, Failure> {
        let clock = Arc::new(ManualClock::new(START_MS));
        let backend = MemoryBackend::with_config(clock.clone(), self.config);
        let forger = MemoryForger {
            backend: backend.clone(),
            principal: "@viewer:mem".into(),
        };
        let mut fx = Fixture::standard(
            Arc::new(backend.store("@owner:mem", "OWNER-DEV")),
            Arc::new(backend.store("@editor:mem", "EDITOR-DEV")),
            Arc::new(backend.store("@viewer:mem", "VIEWER-DEV")),
            Arc::new(ManualTime(clock)),
            self.capabilities(),
            Some(Arc::new(MemoryFaults(backend))),
            Duration::ZERO,
        )
        .await?;
        fx.forger = Some(Arc::new(forger));
        Ok(fx)
    }
}
