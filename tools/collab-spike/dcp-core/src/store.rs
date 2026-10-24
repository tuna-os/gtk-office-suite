//! The one seam every binding implements. Conformance journeys (spec 12) are
//! written against this trait, so the same test runs against the in-memory
//! reference store, the Matrix binding, and the WOPI path.

use async_trait::async_trait;

use crate::model::{Lease, Meta, Role, Stamped, Update, Version};

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

    /// L2. Every retained update, in any order.
    async fn updates(&self, doc: &DocId) -> Result<Vec<Stamped<Update>>>;
}
