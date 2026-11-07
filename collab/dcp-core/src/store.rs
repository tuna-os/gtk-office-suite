//! The one seam every binding implements. Conformance journeys (spec 12) are
//! written against this trait, so the same test runs against the in-memory
//! reference store, the Matrix binding, and the WOPI path.

use async_trait::async_trait;

use crate::model::{Covers, Lease, Meta, Role, Stamped, Update, Version};

pub type DocId = String;

/// Plaintext content handed to a store to publish. The store uploads it,
/// verifies the stored bytes, builds the [`crate::FileRef`] (hash included)
/// and only then publishes the version (spec version rule 2).
#[derive(Clone, Debug)]
pub struct NewFile {
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("rate limited, retry after {retry_after_ms:?} ms")]
    RateLimited { retry_after_ms: Option<u64> },
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("transport: {0}")]
    Transport(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// One authenticated principal's view of the documents a binding holds.
///
/// Implementations MUST enforce roles at the binding (spec conformance test
/// 6), not only in the caller. They MUST NOT return a version whose file they
/// have not been able to fetch and hash-check only when asked to: hash
/// checking is the caller's job via `dcp_core::integrity` (so a store cannot
/// hide a corrupt file by checking it itself).
#[async_trait]
pub trait DocumentStore: Send + Sync {
    /// The principal this store acts as, in the binding's native form.
    fn principal(&self) -> String;

    /// A device identifier, stable for the life of this store.
    fn device(&self) -> String;

    /// Create a document owned by this principal. `first` is published as the
    /// first version (`parents: []`) when given.
    async fn create_document(&self, meta: &Meta, first: Option<NewFile>) -> Result<DocId>;

    async fn meta(&self, doc: &DocId) -> Result<Meta>;

    /// This principal's role on the document, as the binding says it is.
    async fn role(&self, doc: &DocId) -> Result<Role>;

    /// Grant `role` to `principal` (owner only).
    async fn grant(&self, doc: &DocId, principal: &str, role: Role) -> Result<()>;

    /// Remove `principal`'s access (owner only). Spec test 7: a removed
    /// member must not read content published afterwards.
    async fn revoke(&self, doc: &DocId, principal: &str) -> Result<()>;

    /// Every retained version, in any order.
    async fn versions(&self, doc: &DocId) -> Result<Vec<Stamped<Version>>>;

    /// The raw bytes `version.file` points at, unverified.
    async fn fetch_file(&self, doc: &DocId, version: &Stamped<Version>) -> Result<Vec<u8>>;

    /// Upload, verify, then publish (spec version rule 2). `parents` are
    /// version IDs; the caller computed them from [`Self::versions`].
    async fn publish_version(
        &self,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
    ) -> Result<Stamped<Version>>;

    /// The current lease record, if any was ever written.
    async fn lease(&self, doc: &DocId) -> Result<Option<Stamped<Lease>>>;

    /// Write a lease record. Bindings without compare-and-set let two writers
    /// both succeed; callers resolve it by reading back (spec lease rule 3).
    async fn write_lease(&self, doc: &DocId, lease: Lease) -> Result<Stamped<Lease>>;

    /// The binding's own clock, in milliseconds (spec lease rule 1).
    async fn now_ms(&self, doc: &DocId) -> Result<u64>;

    /// L2. Publish an update (spec 4.4).
    async fn publish_update(&self, doc: &DocId, update: Update) -> Result<Stamped<Update>>;

    /// L2. Every retained update, in any order. A binding whose order is a stable
    /// append (a later call returns this call's list with newer updates added at
    /// the end) says so in its own docs; `collabd` names a prefix of the list in a
    /// version's `covers` and so needs that of the bindings it carries.
    async fn updates(&self, doc: &DocId) -> Result<Vec<Stamped<Update>>>;

    // The methods below are not in the spike's trait. `collabd` needs them and every
    // one has a default, so the bindings and the conformance wrappers that do not
    // need them are unchanged.

    /// Wait until `doc` may have news, or `max` has passed, whichever is first.
    /// A binding with a long poll (Durable Streams) or a sync loop (Matrix)
    /// returns as soon as something arrived; the default is a short sleep, so a
    /// caller that loops over `versions` and `updates` polls.
    async fn wait_for_change(&self, _doc: &DocId, max: std::time::Duration) -> Result<()> {
        tokio::time::sleep(max.min(std::time::Duration::from_millis(500))).await;
        Ok(())
    }

    /// L2, spec 7 rule 5. Upload `file` as the payload of an update (bytes over
    /// 34,000) and return the reference to put in `Payload::File`. Unlike a
    /// version's file, nothing is published with it.
    async fn put_update_file(&self, _doc: &DocId, _file: NewFile) -> Result<crate::FileRef> {
        Err(StoreError::Invalid(
            "this binding cannot carry an update payload as a file".into(),
        ))
    }

    /// The bytes `file` points at, unverified (the caller checks `size` and
    /// `sha256` with `dcp_core::integrity::verify`).
    async fn fetch_update_file(&self, _doc: &DocId, _file: &crate::FileRef) -> Result<Vec<u8>> {
        Err(StoreError::Invalid(
            "this binding cannot carry an update payload as a file".into(),
        ))
    }

    /// The records this binding read and refused (wrong signature, will not
    /// decrypt, author without the role, malformed), so a program can report them.
    async fn dropped_records(&self, _doc: &DocId) -> Vec<DroppedRecord> {
        Vec::new()
    }

    /// [`Self::publish_version`] for a program that holds a live replica: the
    /// version carries `covers` (spec 4.4 / 7 rule 4), the claim that this file
    /// holds every update of `covers.algorithm` up to and including
    /// `covers.through`. The caller is responsible for the claim being true (the
    /// sidecar names the end of the prefix of the log its replica has applied);
    /// nothing in the binding is told that a later reader may start from the end
    /// of the log, so no resume hint is written. With `covers: None` this is
    /// `publish_version`. A binding that cannot store `covers` refuses it.
    async fn publish_version_through(
        &self,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
        covers: Option<Covers>,
    ) -> Result<Stamped<Version>> {
        match covers {
            None => self.publish_version(doc, parents, file, message).await,
            Some(_) => Err(StoreError::Invalid(
                "this binding cannot write covers".into(),
            )),
        }
    }
}

/// A record a binding refused to read (spec 10: dropped and reported).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DroppedRecord {
    /// Which stream of the document it was in (`log`, `upd`, `lease`, `ctl`).
    pub stream: String,
    pub id: String,
    pub reason: String,
}
