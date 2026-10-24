//! Store wrappers that break things on purpose.
//!
//! [`CorruptingStore`] damages file bytes in transit, which the reader must
//! catch (spec test 3). [`Defective`] is a store with one named defect; the
//! mutation tests assert that the matching journey fails for each defect, so
//! the suite is known to be able to fail.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use dcp_core::{
    graph, DocId, DocumentStore, Lease, Meta, NewFile, Result, Role, Stamped, StoreError, Update,
    Version,
};
use parking_lot::Mutex;

use crate::fixture::{ALGORITHM, DOC_MODEL};

/// How file bytes are damaged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corruption {
    /// Keep the first half: a truncated download.
    Truncate,
    /// Flip one bit: same size, different hash.
    FlipBit,
    /// Add a trailing byte.
    Append,
    /// Nothing at all.
    Empty,
}

impl Corruption {
    pub const ALL: [Corruption; 4] = [
        Corruption::Truncate,
        Corruption::FlipBit,
        Corruption::Append,
        Corruption::Empty,
    ];

    pub fn apply(self, mut bytes: Vec<u8>) -> Vec<u8> {
        match self {
            Corruption::Truncate => bytes.truncate(bytes.len() / 2),
            Corruption::FlipBit => {
                if let Some(b) = bytes.first_mut() {
                    *b ^= 0x01;
                }
            }
            Corruption::Append => bytes.push(0),
            Corruption::Empty => bytes.clear(),
        }
        bytes
    }
}

/// Which fetches [`CorruptingStore`] damages.
#[derive(Clone, Debug)]
pub enum Target {
    Every,
    Version(String),
}

/// A store whose `fetch_file` returns damaged bytes. Everything else is the
/// inner store, so any store can be put under test with it.
pub struct CorruptingStore {
    inner: Arc<dyn DocumentStore>,
    corruption: Corruption,
    target: Target,
}

impl CorruptingStore {
    pub fn new(inner: Arc<dyn DocumentStore>, corruption: Corruption, target: Target) -> Self {
        CorruptingStore {
            inner,
            corruption,
            target,
        }
    }
}

#[async_trait]
impl DocumentStore for CorruptingStore {
    fn principal(&self) -> String {
        self.inner.principal()
    }
    fn device(&self) -> String {
        self.inner.device()
    }
    async fn create_document(&self, meta: &Meta, first: Option<NewFile>) -> Result<DocId> {
        self.inner.create_document(meta, first).await
    }
    async fn meta(&self, doc: &DocId) -> Result<Meta> {
        self.inner.meta(doc).await
    }
    async fn role(&self, doc: &DocId) -> Result<Role> {
        self.inner.role(doc).await
    }
    async fn grant(&self, doc: &DocId, principal: &str, role: Role) -> Result<()> {
        self.inner.grant(doc, principal, role).await
    }
    async fn revoke(&self, doc: &DocId, principal: &str) -> Result<()> {
        self.inner.revoke(doc, principal).await
    }
    async fn versions(&self, doc: &DocId) -> Result<Vec<Stamped<Version>>> {
        self.inner.versions(doc).await
    }
    async fn fetch_file(&self, doc: &DocId, version: &Stamped<Version>) -> Result<Vec<u8>> {
        let bytes = self.inner.fetch_file(doc, version).await?;
        let hit = match &self.target {
            Target::Every => true,
            Target::Version(id) => *id == version.id,
        };
        Ok(if hit {
            self.corruption.apply(bytes)
        } else {
            bytes
        })
    }
    async fn publish_version(
        &self,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
    ) -> Result<Stamped<Version>> {
        self.inner
            .publish_version(doc, parents, file, message)
            .await
    }
    async fn lease(&self, doc: &DocId) -> Result<Option<Stamped<Lease>>> {
        self.inner.lease(doc).await
    }
    async fn write_lease(&self, doc: &DocId, lease: Lease) -> Result<Stamped<Lease>> {
        self.inner.write_lease(doc, lease).await
    }
    async fn now_ms(&self, doc: &DocId) -> Result<u64> {
        self.inner.now_ms(doc).await
    }
    async fn publish_update(&self, doc: &DocId, update: Update) -> Result<Stamped<Update>> {
        self.inner.publish_update(doc, update).await
    }
    async fn updates(&self, doc: &DocId) -> Result<Vec<Stamped<Update>>> {
        self.inner.updates(doc).await
    }
}

/// The one thing a [`Defective`] store gets wrong.
#[derive(Clone)]
pub enum Defect {
    /// Writes (versions, updates, leases, role changes) are carried out with
    /// the authority of `privileged`, whatever role this handle has: roles
    /// are not enforced at the store (spec test 6).
    IgnoreRoles { privileged: Arc<dyn DocumentStore> },
    /// `write_lease` pretends to succeed even when refused, and `lease`
    /// returns this handle's own last write: the writer never reads back
    /// what the binding actually kept (lease rule 3, spec test 4).
    LeaseEcho,
    /// `revoke` reports success and removes nothing (spec test 7).
    RevokeIsNoop,
    /// `updates` returns the first two updates of each stream with their
    /// `seq` swapped (spec test 9, and update rule 2).
    ReorderUpdates,
    /// `versions` leaves out every head but the newest: a conflict is hidden
    /// (spec test 2).
    HideSiblingHeads,
    /// `versions` leaves out the newest version once there is more than one:
    /// a reader that never catches up (spec test 1).
    HideNewest,
    /// `now_ms` stops at the first value it returned: leases never expire
    /// (spec test 5).
    FrozenClock,
    /// `updates` reports every update as the supported algorithm and model:
    /// an unknown algorithm is not ignored (spec test 8).
    RelabelUpdates,
}

impl Defect {
    pub fn label(&self) -> &'static str {
        match self {
            Defect::IgnoreRoles { .. } => "ignores roles",
            Defect::LeaseEcho => "lease write without read-back",
            Defect::RevokeIsNoop => "revoke is a no-op",
            Defect::ReorderUpdates => "reorders updates",
            Defect::HideSiblingHeads => "hides all but the newest head",
            Defect::HideNewest => "never shows the newest version",
            Defect::FrozenClock => "clock never advances",
            Defect::RelabelUpdates => "relabels unknown algorithms as supported",
        }
    }
}

/// A store with a [`Defect`]. Not for use outside tests of the suite.
pub struct Defective {
    inner: Arc<dyn DocumentStore>,
    defect: Defect,
    echo: Mutex<HashMap<DocId, Stamped<Lease>>>,
    frozen: Mutex<Option<u64>>,
}

impl Defective {
    pub fn new(inner: Arc<dyn DocumentStore>, defect: Defect) -> Self {
        Defective {
            inner,
            defect,
            echo: Mutex::new(HashMap::new()),
            frozen: Mutex::new(None),
        }
    }

    /// Where writes go: the inner store, or the privileged one for
    /// [`Defect::IgnoreRoles`].
    fn writer(&self) -> &Arc<dyn DocumentStore> {
        match &self.defect {
            Defect::IgnoreRoles { privileged } => privileged,
            _ => &self.inner,
        }
    }
}

#[async_trait]
impl DocumentStore for Defective {
    fn principal(&self) -> String {
        self.inner.principal()
    }
    fn device(&self) -> String {
        self.inner.device()
    }
    async fn create_document(&self, meta: &Meta, first: Option<NewFile>) -> Result<DocId> {
        self.inner.create_document(meta, first).await
    }
    async fn meta(&self, doc: &DocId) -> Result<Meta> {
        self.inner.meta(doc).await
    }
    async fn role(&self, doc: &DocId) -> Result<Role> {
        self.inner.role(doc).await
    }
    async fn grant(&self, doc: &DocId, principal: &str, role: Role) -> Result<()> {
        self.writer().grant(doc, principal, role).await
    }
    async fn revoke(&self, doc: &DocId, principal: &str) -> Result<()> {
        if matches!(self.defect, Defect::RevokeIsNoop) {
            return Ok(());
        }
        self.writer().revoke(doc, principal).await
    }
    async fn versions(&self, doc: &DocId) -> Result<Vec<Stamped<Version>>> {
        let mut all = self.inner.versions(doc).await?;
        match self.defect {
            Defect::HideSiblingHeads => {
                let heads: Vec<String> = graph::heads(&all).iter().map(|h| h.id.clone()).collect();
                if let Some((_, others)) = heads.split_first() {
                    all.retain(|v| !others.contains(&v.id));
                }
            }
            Defect::HideNewest if all.len() > 1 => {
                all.sort_by(graph::newest_first);
                all.remove(0);
            }
            _ => {}
        }
        Ok(all)
    }
    async fn fetch_file(&self, doc: &DocId, version: &Stamped<Version>) -> Result<Vec<u8>> {
        self.inner.fetch_file(doc, version).await
    }
    async fn publish_version(
        &self,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
    ) -> Result<Stamped<Version>> {
        self.writer()
            .publish_version(doc, parents, file, message)
            .await
    }
    async fn lease(&self, doc: &DocId) -> Result<Option<Stamped<Lease>>> {
        if matches!(self.defect, Defect::LeaseEcho) {
            if let Some(mine) = self.echo.lock().get(doc) {
                return Ok(Some(mine.clone()));
            }
        }
        self.inner.lease(doc).await
    }
    async fn write_lease(&self, doc: &DocId, lease: Lease) -> Result<Stamped<Lease>> {
        if matches!(self.defect, Defect::LeaseEcho) {
            let now = self.inner.now_ms(doc).await?;
            let stamped = match self.inner.write_lease(doc, lease.clone()).await {
                Ok(s) => s,
                Err(StoreError::Conflict(_)) => {
                    Stamped::new("$echo", now, self.inner.principal(), lease)
                }
                Err(e) => return Err(e),
            };
            self.echo.lock().insert(doc.clone(), stamped.clone());
            return Ok(stamped);
        }
        self.writer().write_lease(doc, lease).await
    }
    async fn now_ms(&self, doc: &DocId) -> Result<u64> {
        let now = self.inner.now_ms(doc).await?;
        if matches!(self.defect, Defect::FrozenClock) {
            return Ok(*self.frozen.lock().get_or_insert(now));
        }
        Ok(now)
    }
    async fn publish_update(&self, doc: &DocId, update: Update) -> Result<Stamped<Update>> {
        self.writer().publish_update(doc, update).await
    }
    async fn updates(&self, doc: &DocId) -> Result<Vec<Stamped<Update>>> {
        let mut all = self.inner.updates(doc).await?;
        if matches!(self.defect, Defect::RelabelUpdates) {
            for u in &mut all {
                u.body.algorithm = ALGORITHM.into();
                u.body.doc_model = DOC_MODEL.into();
            }
        }
        if matches!(self.defect, Defect::ReorderUpdates) {
            let mut streams: BTreeMap<(String, Option<String>), Vec<usize>> = BTreeMap::new();
            for (i, u) in all.iter().enumerate() {
                streams
                    .entry((u.sender.clone(), u.device.clone()))
                    .or_default()
                    .push(i);
            }
            for mut idx in streams.into_values() {
                idx.sort_by_key(|&i| all[i].body.seq);
                if let [a, b, ..] = idx[..] {
                    let (sa, sb) = (all[a].body.seq, all[b].body.seq);
                    all[a].body.seq = sb;
                    all[b].body.seq = sa;
                }
            }
        }
        Ok(all)
    }
}
