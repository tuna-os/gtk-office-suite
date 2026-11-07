//! `P2pStore`: the `DocumentStore` of the peer-to-peer binding.
//!
//! One store is one principal: one Ed25519 key that is the iroh endpoint id, the p2panda author and
//! the p2panda-auth actor, on one OS thread that owns the whole stack (see `replica.rs`). The
//! methods here only send a request to that thread and wait for the answer.

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use async_trait::async_trait;
use dcp_core::{
    DocId, DocumentStore, Lease, Meta, NewFile, Result, Role, Stamped, StoreError, Update, Version,
};
use iroh::EndpointAddr;
use p2panda_core::VerifyingKey;
use tokio::sync::{mpsc, oneshot};

use crate::clock::Clock;
use crate::envelope::{parse_principal, principal_of};
use crate::net::key_of;
use crate::replica::{tr, Counters, Inner, P2pConfig};

type Job = Box<dyn FnOnce(Rc<Inner>) -> Pin<Box<dyn Future<Output = ()>>> + Send>;

pub struct P2pStore {
    principal: String,
    device: String,
    jobs: mpsc::UnboundedSender<Job>,
    addr: EndpointAddr,
    clock: Arc<dyn Clock>,
    truncate_next: Arc<std::sync::atomic::AtomicBool>,
}

impl P2pStore {
    /// Start a store: bind an iroh endpoint, start the thread. With the default configuration it
    /// listens on loopback only and uses no relay and no external service.
    pub async fn spawn(cfg: P2pConfig) -> Result<P2pStore> {
        let clock = cfg.clock.clone();
        let (job_tx, mut job_rx) = mpsc::unbounded_channel::<Job>();
        type Ready = Result<(String, String, EndpointAddr, Arc<std::sync::atomic::AtomicBool>)>;
        let (ready_tx, ready_rx) = oneshot::channel::<Ready>();
        std::thread::Builder::new()
            .name("dcp-p2p".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("a current-thread tokio runtime");
                let local = tokio::task::LocalSet::new();
                local.block_on(&rt, async move {
                    match Inner::new(cfg).await {
                        Err(e) => {
                            let _ = ready_tx.send(Err(e));
                        }
                        Ok(inner) => {
                            let _ = ready_tx.send(Ok((
                                inner.principal.clone(),
                                inner.device.clone(),
                                inner.net.addr(),
                                inner.truncate_next.clone(),
                            )));
                            while let Some(job) = job_rx.recv().await {
                                tokio::task::spawn_local(job(inner.clone()));
                            }
                            // Every handle is gone: close the network and the databases here,
                            // where the runtime that owns them is still running.
                            inner.shutdown().await;
                        }
                    }
                });
                // Dropping the tasks that still hold the node needs a runtime context (sqlx's
                // pool returns its connections with a spawn).
                let _enter = rt.enter();
                drop(local);
            })
            .map_err(tr)?;
        let (principal, device, addr, truncate_next) = ready_rx
            .await
            .map_err(|_| tr("the store thread died while starting"))??;
        Ok(P2pStore {
            principal,
            device,
            jobs: job_tx,
            addr,
            clock,
            truncate_next,
        })
    }

    async fn run<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Rc<Inner>) -> Pin<Box<dyn Future<Output = Result<T>>>> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let job: Job = Box::new(move |inner| {
            Box::pin(async move {
                let r = f(inner).await;
                let _ = tx.send(r);
            })
        });
        self.jobs
            .send(job)
            .map_err(|_| tr("the store thread has stopped"))?;
        rx.await
            .map_err(|_| tr("the store thread dropped the request (it panicked)"))?
    }

    // ------------------------------------------------------------ binding-specific API

    /// Where other nodes dial this one: its endpoint id and loopback socket (and relay).
    pub fn addr(&self) -> EndpointAddr {
        self.addr.clone()
    }

    /// Tell this node where a peer can be dialled. The peer's key is the address's endpoint id.
    pub async fn add_peer(&self, addr: EndpointAddr) -> Result<()> {
        let key: VerifyingKey = key_of(&addr.id).map_err(tr)?;
        self.run(move |i| {
            Box::pin(async move {
                i.peers.borrow_mut().insert(key, addr);
                i.changed.notify_one();
                Ok(())
            })
        })
        .await
    }

    /// Start (or continue) a document this principal was invited to. Returns the contact card the
    /// creator must register (`register_card`) before it can grant this principal a role.
    pub async fn join(&self, doc: &DocId, creator: &str) -> Result<Vec<u8>> {
        let (doc, creator) = (doc.clone(), creator.to_owned());
        self.run(move |i| Box::pin(async move { i.join_document(&doc, &creator).await }))
            .await
    }

    pub async fn card(&self, doc: &DocId) -> Result<Vec<u8>> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                i.card(&d).await
            })
        })
        .await
    }

    /// Register another principal's contact card for `doc`; returns whose it was.
    pub async fn register_card(&self, doc: &DocId, card: Vec<u8>) -> Result<String> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                let who = i.register_card(&d, &card).await?;
                Ok(principal_of(&who))
            })
        })
        .await
    }

    /// Go offline (refuse every connection, close the open ones, dial nobody) or come back.
    pub async fn set_online(&self, online: bool) -> Result<()> {
        self.run(move |i| {
            Box::pin(async move {
                i.set_online(online);
                Ok(())
            })
        })
        .await
    }

    pub async fn counters(&self, doc: &DocId) -> Result<Counters> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                let c = d.st.borrow().counters.clone();
                Ok(c)
            })
        })
        .await
    }

    /// What iroh reports about the paths of every connection this node has dialled or accepted
    /// (initiator side of iroh-blobs fetches and both sides of sync connections), the paths open
    /// now, and iroh's socket counters. Observation only.
    pub async fn net_report(&self) -> Result<crate::pathlog::NetReport> {
        self.run(move |i| Box::pin(async move { Ok(crate::pathlog::report(&i.pathlog, &i.net.endpoint)) }))
            .await
    }

    /// Wait until the endpoint has a home relay (`Endpoint::online`), at most `max`. `false` on
    /// timeout.
    pub async fn wait_online(&self, max: std::time::Duration) -> Result<bool> {
        self.run(move |i| {
            Box::pin(async move { Ok(tokio::time::timeout(max, i.net.endpoint.online()).await.is_ok()) })
        })
        .await
    }

    /// Run a sync with `peer` now and return what moved each way.
    pub async fn sync_now(&self, doc: &DocId, peer: &str) -> Result<crate::net::SessionStats> {
        let (doc, peer) = (doc.clone(), peer.to_owned());
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                let key = parse_principal(&peer).ok_or_else(|| StoreError::Invalid("principal".into()))?;
                i.sync_once(&d, key).await.map_err(StoreError::Transport)
            })
        })
        .await
    }

    /// The members this node currently believes the document has, with their roles.
    pub async fn members(&self, doc: &DocId) -> Result<Vec<(String, Option<Role>)>> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                let st = d.st.borrow();
                Ok(st
                    .members
                    .iter()
                    .map(|(k, l)| (principal_of(k), crate::replica::role_of_level(l)))
                    .collect())
            })
        })
        .await
    }

    /// Entries accepted so far, of any kind (decrypted, signed, not yet authorised).
    pub async fn entry_count(&self, doc: &DocId) -> Result<usize> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                let n = d.st.borrow().entries.len();
                Ok(n)
            })
        })
        .await
    }

    /// Operations applied / waiting for a dependency / refused.
    pub async fn op_counts(&self, doc: &DocId) -> Result<(usize, usize, usize)> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                let st = d.st.borrow();
                Ok((st.applied.len(), st.pending.len(), st.rejected.len()))
            })
        })
        .await
    }

    /// Timings of the blob fetches this node has made for `doc`.
    pub async fn blob_fetches(&self, doc: &DocId) -> Result<Vec<crate::replica::BlobFetch>> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                let v = d.st.borrow().blob_fetches.clone();
                Ok(v)
            })
        })
        .await
    }

    /// What a hostile member does: publish a version entry around every role check.
    pub async fn forge_version(&self, doc: &DocId, version: Version) -> Result<String> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                i.forge_version(&d, version).await
            })
        })
        .await
    }

    /// Fault injection: the next file stored is cut in half before it reaches the blob store.
    pub fn truncate_next_upload(&self) {
        self.truncate_next.store(true, Ordering::SeqCst);
    }

    /// Size of the persisted p2panda-spaces state (CBOR bytes).
    pub async fn space_state_bytes(&self, doc: &DocId) -> Result<usize> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                i.space_state_bytes(&d).await
            })
        })
        .await
    }

    /// The encoded headers of every operation of the document this node holds.
    pub async fn raw_op_headers(&self, doc: &DocId) -> Result<Vec<Vec<u8>>> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                i.raw_op_headers(&d).await
            })
        })
        .await
    }

    /// Get a blob from `peer` by hash alone, around every version entry (a test hook for what a
    /// removed member could try). Returns the sealed bytes.
    pub async fn fetch_raw_blob(&self, peer: &str, blake3_hex: &str) -> Result<Vec<u8>> {
        let (peer, hex) = (peer.to_owned(), blake3_hex.to_owned());
        self.run(move |i| {
            Box::pin(async move {
                let key = parse_principal(&peer).ok_or_else(|| StoreError::Invalid("principal".into()))?;
                i.fetch_raw_blob(key, &hex).await
            })
        })
        .await
    }

    /// The sealed bytes of a blob as this node holds them (what a provider would serve).
    pub async fn raw_blob(&self, blake3_hex: &str) -> Result<Option<Vec<u8>>> {
        let hex = blake3_hex.to_owned();
        self.run(move |i| {
            Box::pin(async move {
                let h: iroh_blobs::Hash = std::str::FromStr::from_str(&hex).map_err(tr)?;
                if !i.net.blobs.has(h).await.map_err(tr)? {
                    return Ok(None);
                }
                let b = i.net.blobs.get_bytes(h).await.map_err(tr)?;
                Ok(Some(b.to_vec()))
            })
        })
        .await
    }
}

#[async_trait]
impl DocumentStore for P2pStore {
    fn principal(&self) -> String {
        self.principal.clone()
    }

    fn device(&self) -> String {
        self.device.clone()
    }

    async fn create_document(&self, meta: &Meta, first: Option<NewFile>) -> Result<DocId> {
        let meta = meta.clone();
        self.run(move |i| Box::pin(async move { i.create_document(meta, first).await }))
            .await
    }

    async fn meta(&self, doc: &DocId) -> Result<Meta> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                i.meta(&d).await
            })
        })
        .await
    }

    async fn role(&self, doc: &DocId) -> Result<Role> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                i.role(&d)
            })
        })
        .await
    }

    async fn grant(&self, doc: &DocId, principal: &str, role: Role) -> Result<()> {
        let (doc, principal) = (doc.clone(), principal.to_owned());
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                i.grant(&d, &principal, role).await
            })
        })
        .await
    }

    async fn revoke(&self, doc: &DocId, principal: &str) -> Result<()> {
        let (doc, principal) = (doc.clone(), principal.to_owned());
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                i.revoke(&d, &principal).await
            })
        })
        .await
    }

    async fn versions(&self, doc: &DocId) -> Result<Vec<Stamped<Version>>> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                i.versions(&d).await
            })
        })
        .await
    }

    async fn fetch_file(&self, doc: &DocId, version: &Stamped<Version>) -> Result<Vec<u8>> {
        let (doc, version) = (doc.clone(), version.clone());
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                i.fetch_file(&d, &version).await
            })
        })
        .await
    }

    async fn publish_version(
        &self,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
    ) -> Result<Stamped<Version>> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                i.publish_version(&d, parents, file, message).await
            })
        })
        .await
    }

    async fn lease(&self, doc: &DocId) -> Result<Option<Stamped<Lease>>> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                i.lease(&d).await
            })
        })
        .await
    }

    async fn write_lease(&self, doc: &DocId, lease: Lease) -> Result<Stamped<Lease>> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                i.write_lease(&d, lease).await
            })
        })
        .await
    }

    /// There is no binding clock in a peer-to-peer binding: this is the local clock (see the
    /// results document: lease stamps are taken on this same clock, so it never has to agree with
    /// anyone else's).
    async fn now_ms(&self, _doc: &DocId) -> Result<u64> {
        Ok(self.clock.now_ms())
    }

    async fn publish_update(&self, doc: &DocId, update: Update) -> Result<Stamped<Update>> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                i.publish_update(&d, update).await
            })
        })
        .await
    }

    async fn updates(&self, doc: &DocId) -> Result<Vec<Stamped<Update>>> {
        let doc = doc.clone();
        self.run(move |i| {
            Box::pin(async move {
                let d = i.doc(&doc)?;
                i.updates(&d).await
            })
        })
        .await
    }
}
