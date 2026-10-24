//! What a binding provides to run the journeys: a [`StoreFactory`] that makes
//! a fresh, standard [`Fixture`], and the [`Capabilities`] it honestly has.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dcp_core::{DocId, DocumentStore, Level, Meta, NewFile, Role};

use crate::Failure;

/// The MIME type of every fixture file.
pub const MIME: &str = "application/vnd.oasis.opendocument.text";
/// The content of the one version a standard fixture starts with.
pub const BASE_FILE: &[u8] = b"DCP conformance base file";
/// The only update algorithm and document model the journeys use. The toy
/// algorithm `append/1` on model `text/1` appends each payload to the state.
pub const ALGORITHM: &str = "append/1";
pub const DOC_MODEL: &str = "text/1";

/// What a binding can do. A journey whose requirement is `false` is reported
/// as skipped (printed, and counted apart from passes); it never passes.
///
/// Set a flag to `false` only for a real, documented limitation of the
/// binding, and say so in its results document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    /// `grant` and `revoke` work (otherwise the fixture's roles are given).
    pub grant_revoke: bool,
    /// Content published after a `revoke` is unreadable to the revoked
    /// principal (spec test 7; encrypted bindings). Needs `grant_revoke`.
    pub revocation: bool,
    /// `write_lease` takes any [`dcp_core::Lease`] record (lease journeys).
    pub arbitrary_lease_writes: bool,
    /// `write_lease` has no compare-and-set: two writers both succeed and
    /// only the read-back resolves it (Matrix state events). When true the
    /// lease journey also asserts the race really happened.
    pub racy_lease: bool,
    /// `versions` lists the full version graph, not only the head.
    pub history: bool,
    /// `publish_version` honours the parents the caller names, so forks and
    /// merges can be made (spec tests 2 and 4 rule 6).
    pub arbitrary_parents: bool,
    /// L2: `publish_update` and `updates` work.
    pub updates: bool,
    /// The fixture carries [`Faults`] that can truncate an upload.
    pub upload_faults: bool,
    /// The fixture carries a [`Forger`]: a way to send a version as the
    /// viewer around the store's own checks (the forged-version journey).
    pub forgery: bool,
}

impl Capabilities {
    /// Every capability except `racy_lease`, which describes a limitation.
    pub fn full() -> Self {
        Capabilities {
            grant_revoke: true,
            revocation: true,
            arbitrary_lease_writes: true,
            racy_lease: false,
            history: true,
            arbitrary_parents: true,
            updates: true,
            upload_faults: true,
            forgery: true,
        }
    }
}

/// How a binding lets time pass, so a lease can expire. An in-memory store
/// moves its injected clock; a real binding sleeps.
#[async_trait]
pub trait Time: Send + Sync {
    async fn pass(&self, ms: u64);
}

/// Real time.
pub struct RealTime;

#[async_trait]
impl Time for RealTime {
    async fn pass(&self, ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

/// Fault injection a binding may offer so that "a truncated upload is never
/// published" (spec test 3) can be exercised.
pub trait Faults: Send + Sync {
    /// The next file upload to the binding's storage is cut short.
    fn truncate_next_upload(&self);
}

/// What a hostile member's client does: send a version event to the binding's
/// server as the fixture's **viewer**, skipping every check the store makes
/// (role, parents, upload verification, author). The server's own gate is the
/// only thing that can refuse it; if it does not, only the reader's rules
/// (spec 8.1: `created_by.principal` equals the sender, and senders below the
/// editor level are dropped when the server does not gate) stand between the
/// forged version and the user.
#[async_trait]
pub trait Forger: Send + Sync {
    /// `Ok(Some(id))`: the server accepted the event and gave it this id.
    /// `Ok(None)`: the server refused it. Any other failure is an `Err`.
    async fn forge_version(&self, doc: &DocId, version: dcp_core::Version) -> Result<Option<String>, Failure>;
}

/// A document in a known state, with a principal for each role.
///
/// Contract for [`StoreFactory::fresh`]: `doc` exists; `owner` is its owner,
/// `editor` an editor and `viewer` a viewer, each a handle for a distinct
/// principal and device; and it has exactly one version (`first_version`,
/// `parents: []`) whose file is [`BASE_FILE`] of type [`MIME`]. The metadata
/// has levels L1 and L2 and algorithm [`ALGORITHM`]. Use [`Fixture::standard`]
/// to get all of that from three handles.
pub struct Fixture {
    pub owner: Arc<dyn DocumentStore>,
    pub editor: Arc<dyn DocumentStore>,
    pub viewer: Arc<dyn DocumentStore>,
    pub doc: DocId,
    pub first_version: String,
    pub time: Arc<dyn Time>,
    pub caps: Capabilities,
    pub faults: Option<Arc<dyn Faults>>,
    /// Set by a binding whose `capabilities().forgery` is true; it acts as
    /// `viewer`. `None` from the constructors: assign it after building.
    pub forger: Option<Arc<dyn Forger>>,
    /// How long a journey waits for an effect to reach another handle
    /// (eventual consistency over a network). Zero for in-process stores.
    pub settle: Duration,
}

pub fn standard_meta() -> Meta {
    Meta {
        dcp: 1,
        title: "DCP conformance document".into(),
        mime: MIME.into(),
        levels: vec![Level::L1, Level::L2],
        algorithms: Some(vec![ALGORITHM.into()]),
        editor_services: None,
    }
}

pub fn new_file(bytes: &[u8]) -> NewFile {
    NewFile {
        mime: MIME.into(),
        bytes: bytes.to_vec(),
    }
}

impl Fixture {
    /// Build the standard fixture from three handles on one binding: `owner`
    /// creates the document with [`BASE_FILE`] and grants the other two.
    /// When `caps.grant_revoke` is false the binding set the roles up itself,
    /// and `existing` names the document it made.
    pub async fn standard(
        owner: Arc<dyn DocumentStore>,
        editor: Arc<dyn DocumentStore>,
        viewer: Arc<dyn DocumentStore>,
        time: Arc<dyn Time>,
        caps: Capabilities,
        faults: Option<Arc<dyn Faults>>,
        settle: Duration,
    ) -> Result<Fixture, Failure> {
        let doc = owner
            .create_document(&standard_meta(), Some(new_file(BASE_FILE)))
            .await?;
        owner.grant(&doc, &editor.principal(), Role::Editor).await?;
        owner.grant(&doc, &viewer.principal(), Role::Viewer).await?;
        let fx =
            Fixture::from_existing(owner, editor, viewer, doc, time, caps, faults, settle).await?;
        Ok(fx)
    }

    /// Wrap a document the binding already set up to the fixture contract.
    #[allow(clippy::too_many_arguments)]
    pub async fn from_existing(
        owner: Arc<dyn DocumentStore>,
        editor: Arc<dyn DocumentStore>,
        viewer: Arc<dyn DocumentStore>,
        doc: DocId,
        time: Arc<dyn Time>,
        caps: Capabilities,
        faults: Option<Arc<dyn Faults>>,
        settle: Duration,
    ) -> Result<Fixture, Failure> {
        let mut fx = Fixture {
            owner,
            editor,
            viewer,
            doc,
            first_version: String::new(),
            time,
            caps,
            faults,
            forger: None,
            settle,
        };
        let versions = fx.owner.versions(&fx.doc).await?;
        if versions.len() != 1 || !versions[0].body.parents.is_empty() {
            return Err(Failure::new(format!(
                "fixture contract: exactly one parentless version expected, found {}",
                versions.len()
            )));
        }
        fx.first_version = versions[0].id.clone();
        Ok(fx)
    }

    /// Poll `probe` until it yields `Some`, for at most [`Self::settle`]; `None`
    /// if it never did. With a zero settle time it is tried exactly once.
    pub async fn settle_soft<T, F, Fut>(&self, mut probe: F) -> Result<Option<T>, Failure>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<Option<T>, Failure>>,
    {
        let deadline = tokio::time::Instant::now() + self.settle;
        loop {
            if let Some(v) = probe().await? {
                return Ok(Some(v));
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(None);
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Like [`Self::settle_soft`] but a timeout is a failure. Use it for
    /// anything that has to propagate between handles; never to wait for
    /// something that must NOT happen.
    pub async fn settle<T, F, Fut>(&self, what: &str, probe: F) -> Result<T, Failure>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<Option<T>, Failure>>,
    {
        self.settle_soft(probe).await?.ok_or_else(|| {
            if self.settle.is_zero() {
                Failure::new(format!("expected at once, but not so: {what}"))
            } else {
                Failure::new(format!(
                    "timed out after {:?} waiting for: {what}",
                    self.settle
                ))
            }
        })
    }
}

/// Makes a fresh [`Fixture`] per journey, on a binding under test.
#[async_trait]
pub trait StoreFactory: Send + Sync {
    /// A label for reports, such as `memory` or `matrix-synapse`.
    fn name(&self) -> String;

    /// What the binding can do. Read before a fixture is made, so an
    /// unsupported journey is skipped without building anything.
    fn capabilities(&self) -> Capabilities;

    async fn fresh(&self) -> Result<Fixture, Failure>;
}
