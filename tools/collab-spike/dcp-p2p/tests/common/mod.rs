//! Shared harness: nodes on loopback, a mesh, and the conformance factory.
#![allow(dead_code)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dcp_conformance::fixture::Faults;
use dcp_conformance::{
    new_file, standard_meta, Capabilities, Failure, Fixture, Forger, RealTime, StoreFactory,
    BASE_FILE,
};
use dcp_core::{DocId, DocumentStore, Role, StoreError, Version};
use dcp_p2p::{Clock, P2pConfig, P2pStore, SkewedClock};

pub fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
}

/// A configuration for tests: loopback, quick anti-entropy.
pub fn cfg() -> P2pConfig {
    P2pConfig {
        sync_interval: Duration::from_millis(200),
        ..P2pConfig::default()
    }
}

pub async fn node(cfg: P2pConfig) -> Arc<P2pStore> {
    Arc::new(P2pStore::spawn(cfg).await.expect("a node starts"))
}

/// Everyone learns everyone's address.
pub async fn mesh(nodes: &[&Arc<P2pStore>]) {
    for a in nodes {
        for b in nodes {
            if a.principal() != b.principal() {
                a.add_peer(b.addr()).await.expect("add_peer");
            }
        }
    }
}

/// Poll `probe` until it yields `Some`, at most `max`. Panics with `what` on timeout.
pub async fn wait_for<T, F, Fut>(what: &str, max: Duration, mut probe: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let deadline = Instant::now() + max;
    loop {
        if let Some(v) = probe().await {
            return v;
        }
        if Instant::now() >= deadline {
            panic!("timed out after {max:?} waiting for: {what}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// `soft`: like `wait_for` but returns `None` on timeout.
pub async fn wait_soft<T, F, Fut>(max: Duration, mut probe: F) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let deadline = Instant::now() + max;
    loop {
        if let Some(v) = probe().await {
            return Some(v);
        }
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Invite `guest` to `doc` (created by `owner`): the guest joins and hands back its card, the
/// owner registers it. Does not grant a role.
pub async fn invite(owner: &Arc<P2pStore>, guest: &Arc<P2pStore>, doc: &DocId) {
    let card = guest.join(doc, &owner.principal()).await.expect("join");
    owner.register_card(doc, card).await.expect("register_card");
}

/// Three nodes, one document with one version, roles granted, and everything synced. Returns
/// (owner, editor, viewer, doc).
pub async fn trio(
    owner_cfg: P2pConfig,
    editor_cfg: P2pConfig,
    viewer_cfg: P2pConfig,
) -> (Arc<P2pStore>, Arc<P2pStore>, Arc<P2pStore>, DocId) {
    let owner = node(owner_cfg).await;
    let editor = node(editor_cfg).await;
    let viewer = node(viewer_cfg).await;
    mesh(&[&owner, &editor, &viewer]).await;
    let doc = owner
        .create_document(&standard_meta(), Some(new_file(BASE_FILE)))
        .await
        .expect("create_document");
    invite(&owner, &editor, &doc).await;
    invite(&owner, &viewer, &doc).await;
    owner.grant(&doc, &editor.principal(), Role::Editor).await.expect("grant editor");
    owner.grant(&doc, &viewer.principal(), Role::Viewer).await.expect("grant viewer");
    for (n, role) in [(&editor, Role::Editor), (&viewer, Role::Viewer)] {
        wait_for(&format!("{} has role {role:?} and the first version", n.principal()), Duration::from_secs(30), || async {
            if n.role(&doc).await.ok()? != role {
                return None;
            }
            n.meta(&doc).await.ok()?;
            (n.versions(&doc).await.ok()?.len() == 1).then_some(())
        })
        .await;
    }
    (owner, editor, viewer, doc)
}

pub async fn default_trio() -> (Arc<P2pStore>, Arc<P2pStore>, Arc<P2pStore>, DocId) {
    trio(cfg(), cfg(), cfg()).await
}

// ----------------------------------------------------------------------------- conformance

pub struct EditorFaults(pub Arc<P2pStore>);

impl Faults for EditorFaults {
    fn truncate_next_upload(&self) {
        self.0.truncate_next_upload();
    }
}

/// The viewer's client sending a version around the store: it signs and encrypts the entry itself
/// (it holds the group key) without the role check. There is no server to refuse it: the readers'
/// authorisation rule is the only gate, so the answer is always "accepted by the network".
pub struct ViewerForger(pub Arc<P2pStore>);

#[async_trait]
impl Forger for ViewerForger {
    async fn forge_version(&self, doc: &DocId, version: Version) -> Result<Option<String>, Failure> {
        match self.0.forge_version(doc, version).await {
            Ok(id) => Ok(Some(id)),
            Err(StoreError::Forbidden(_)) => Ok(None),
            Err(e) => Err(Failure::new(format!("forging failed unexpectedly: {e}"))),
        }
    }
}

pub struct P2pFactory {
    pub label: &'static str,
    pub owner: P2pConfig,
    pub editor: P2pConfig,
    pub viewer: P2pConfig,
}

impl P2pFactory {
    pub fn new() -> Self {
        P2pFactory {
            label: "",
            owner: cfg(),
            editor: cfg(),
            viewer: cfg(),
        }
    }

    pub fn skewed(owner_ms: i64, editor_ms: i64) -> Self {
        let mut f = P2pFactory::new();
        f.owner.clock = Arc::new(SkewedClock { offset_ms: owner_ms });
        f.editor.clock = Arc::new(SkewedClock { offset_ms: editor_ms });
        f
    }

    pub fn with_all(mut self, f: impl Fn(&mut P2pConfig)) -> Self {
        f(&mut self.owner);
        f(&mut self.editor);
        f(&mut self.viewer);
        self
    }
}

#[async_trait]
impl StoreFactory for P2pFactory {
    fn name(&self) -> String {
        format!("p2p-iroh-p2panda{}", self.label)
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            grant_revoke: true,
            revocation: true,
            arbitrary_lease_writes: true,
            // A lease write is an append to the author's own log: two writers both succeed and the
            // deterministic winner rule (Lamport value, then entry id) picks one afterwards.
            racy_lease: true,
            history: true,
            arbitrary_parents: true,
            updates: true,
            // The "upload" a fault can cut short is the writer's import into its own blob store;
            // there is no remote upload step in a peer-to-peer binding.
            upload_faults: true,
            forgery: true,
        }
    }

    async fn fresh(&self) -> Result<Fixture, Failure> {
        let (owner, editor, viewer, doc) = trio(self.owner.clone(), self.editor.clone(), self.viewer.clone()).await;
        let mut fx = Fixture::from_existing(
            owner,
            editor.clone(),
            viewer.clone(),
            doc,
            Arc::new(RealTime),
            self.capabilities(),
            Some(Arc::new(EditorFaults(editor))),
            Duration::from_secs(30),
        )
        .await?;
        fx.forger = Some(Arc::new(ViewerForger(viewer)));
        Ok(fx)
    }
}

pub fn skewed_clock(ms: i64) -> Arc<dyn Clock> {
    Arc::new(SkewedClock { offset_ms: ms })
}
