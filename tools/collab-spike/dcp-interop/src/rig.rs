//! One document room on a real Synapse, served two ways.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use dcp_conformance::{new_file, standard_meta, Failure, BASE_FILE};
use dcp_core::view::{read_view, View};
use dcp_core::{DocId, DocumentStore, Role};
use dcp_matrix::{MatrixConfig, MatrixStore};
use dcp_wopi::{HostConfig, RunningHost, WopiClientStore, WopiHost};

use crate::common::proxy::FaultProxy;
use crate::common::{self, Synapse};
use crate::raw::RawWopi;

/// The three principals of a rig. Each is a distinct Matrix user and device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Owner,
    Editor,
    Viewer,
}

impl Who {
    pub const ALL: [Who; 3] = [Who::Owner, Who::Editor, Who::Viewer];

    fn index(self) -> usize {
        match self {
            Who::Owner => 0,
            Who::Editor => 1,
            Who::Viewer => 2,
        }
    }
}

/// The real `MatrixStore` of each principal, before any wrapper.
pub struct Handles {
    pub owner: Arc<MatrixStore>,
    pub editor: Arc<MatrixStore>,
    pub viewer: Arc<MatrixStore>,
}

impl Handles {
    pub fn get(&self, who: Who) -> &Arc<MatrixStore> {
        match who {
            Who::Owner => &self.owner,
            Who::Editor => &self.editor,
            Who::Viewer => &self.viewer,
        }
    }
}

/// Wraps one principal's store, to put a [`dcp_conformance::wrappers::Defective`]
/// in front of it. The honest rig wraps nothing.
pub type Wrap = fn(Who, Arc<dyn DocumentStore>, &Handles) -> Arc<dyn DocumentStore>;

fn unwrapped(_: Who, s: Arc<dyn DocumentStore>, _: &Handles) -> Arc<dyn DocumentStore> {
    s
}

/// Changes every principal's `MatrixConfig` before it connects.
pub type Configure = fn(&mut MatrixConfig);

fn default_config(_: &mut MatrixConfig) {}

#[derive(Clone, Copy)]
pub struct RigOptions {
    /// Applied to the handle a test calls natively (`Rig::native`).
    pub native: Wrap,
    /// Applied to the handle the WOPI host serves each token from.
    pub backing: Wrap,
    /// Applied to every principal's `MatrixConfig`.
    pub config: Configure,
    /// The port the WOPI host binds on all interfaces; 0 is an ephemeral
    /// loopback port. A real Collabora is told this port.
    pub wopi_port: u16,
    /// The bytes of the first version. `None`: opaque conformance bytes
    /// (`BASE_FILE`). A real Collabora needs a real document.
    pub first_file: Option<fn() -> Vec<u8>>,
    /// Spec version rule 3: the host refuses a `PutFile` that is not a readable
    /// ODF/OOXML package. Off for opaque bytes.
    pub validate_packages: bool,
    /// How long a check waits for an effect to reach another handle.
    pub settle: Duration,
}

impl RigOptions {
    pub fn honest() -> Self {
        RigOptions {
            native: unwrapped,
            backing: unwrapped,
            config: default_config,
            wopi_port: 0,
            first_file: None,
            validate_packages: false,
            settle: Duration::from_secs(30),
        }
    }

    /// For a real Collabora: the host listens on `port`, the first version is
    /// the document `first_file` builds, and the host validates packages.
    pub fn for_collabora(mut self, port: u16, first_file: fn() -> Vec<u8>) -> Self {
        self.wopi_port = port;
        self.first_file = Some(first_file);
        self.validate_packages = true;
        self
    }

    /// The room is created with the literal draft-0 power-level table: no
    /// `events["m.room.encrypted"]` entry, so the server does not gate the
    /// timeline and only the readers' rules can drop a viewer's forged version.
    pub fn draft0_power_levels(mut self) -> Self {
        self.config = |c| c.enforce_encrypted_pl = false;
        self
    }

    /// As [`Self::draft0_power_levels`], and no reader checks the sender's
    /// level either (a deliberate defect: the negative control).
    pub fn draft0_power_levels_and_readers_that_trust_senders(mut self) -> Self {
        self.config = |c| {
            c.enforce_encrypted_pl = false;
            c.reader_checks_senders = false;
        };
        self
    }

    pub fn native(mut self, w: Wrap) -> Self {
        self.native = w;
        self
    }

    pub fn backing(mut self, w: Wrap) -> Self {
        self.backing = w;
        self
    }

    /// A broken store makes a check time out; a short settle keeps a negative
    /// control from waiting the full 30 s.
    pub fn settle(mut self, d: Duration) -> Self {
        self.settle = d;
        self
    }
}

/// The Synapse profile every interop test is written for: the binding's own
/// 3 events/s client limiter is the only pacing, as in the spike-A journeys.
pub fn require_relaxed(syn: &Synapse) {
    assert_eq!(
        syn.profile, "relaxed",
        "the interop tests need Synapse started with `scripts/synapse-up.sh relaxed` (DCP_SYNAPSE_PROFILE=relaxed), got {:?}",
        syn.profile
    );
}

/// `Some` when a Synapse is configured; `None` only for an explicit
/// `DCP_SKIP_SYNAPSE=1` (printed as SKIPPED by [`common::synapse`]). Panics
/// otherwise, so a missing Synapse fails the run.
pub fn synapse() -> Option<Synapse> {
    let syn = common::synapse()?;
    require_relaxed(&syn);
    Some(syn)
}

pub struct Rig {
    pub syn: Synapse,
    /// Between every client (and the WOPI host's backing stores) and Synapse;
    /// it can truncate the next media upload.
    pub proxy: Arc<FaultProxy>,
    pub handles: Handles,
    native: [Arc<dyn DocumentStore>; 3],
    pub host: WopiHost,
    server: RunningHost,
    tokens: [String; 3],
    pub doc: DocId,
    /// The one version the document starts with (`BASE_FILE`, no parents).
    pub first: String,
    pub settle: Duration,
    backing: Wrap,
}

impl Rig {
    /// A fresh document on `syn`: the owner creates it with [`BASE_FILE`] as
    /// its first version, invites an editor and a viewer, and both have joined
    /// and read that version before this returns. A WOPI host is listening and
    /// has one access token per principal, each served from that principal's
    /// own `MatrixStore`.
    pub async fn new(syn: &Synapse, opts: RigOptions) -> Result<Rig, Failure> {
        let host_port = syn.url.trim_start_matches("http://").trim_end_matches('/').to_owned();
        let proxy = Arc::new(FaultProxy::start(&host_port).await);
        let url = proxy.url.clone();
        let configure = opts.config;
        let mk = |prefix: &'static str| {
            let url = url.clone();
            async move {
                syn.user_with(prefix, move |c| {
                    c.homeserver = url;
                    configure(c);
                })
                .await
            }
        };
        let handles = Handles {
            owner: Arc::new(mk("owner").await),
            editor: Arc::new(mk("editor").await),
            viewer: Arc::new(mk("viewer").await),
        };

        let first_bytes = opts.first_file.map_or_else(|| BASE_FILE.to_vec(), |f| f());
        let doc = handles.owner.create_document(&standard_meta(), Some(new_file(&first_bytes))).await?;
        handles.owner.grant(&doc, &handles.editor.principal(), Role::Editor).await?;
        handles.owner.grant(&doc, &handles.viewer.principal(), Role::Viewer).await?;
        let versions = handles.owner.versions(&doc).await?;
        if versions.len() != 1 || !versions[0].body.parents.is_empty() {
            return Err(Failure::new(format!(
                "rig: expected exactly one parentless version, found {}",
                versions.len()
            )));
        }
        let first = versions[0].id.clone();

        let wrap = |w: Wrap, who: Who| w(who, handles.get(who).clone() as Arc<dyn DocumentStore>, &handles);
        let native = [wrap(opts.native, Who::Owner), wrap(opts.native, Who::Editor), wrap(opts.native, Who::Viewer)];

        let host = WopiHost::new(HostConfig {
            // The conformance fixtures' files are opaque bytes, not ODF packages.
            validate_packages: opts.validate_packages,
            ..HostConfig::default()
        });
        let addr = if opts.wopi_port == 0 { "127.0.0.1:0".to_owned() } else { format!("0.0.0.0:{}", opts.wopi_port) };
        let server = host
            .serve(addr.parse().expect("socket addr"))
            .await
            .map_err(|e| Failure::new(format!("cannot start the WOPI host on {addr}: {e}")))?;
        let token = |who: Who| host.register(doc.clone(), wrap(opts.backing, who), handles.get(who).principal());
        let tokens = [token(Who::Owner), token(Who::Editor), token(Who::Viewer)];

        let rig = Rig {
            syn: syn.clone(),
            proxy,
            handles,
            native,
            host,
            server,
            tokens,
            doc,
            first,
            settle: opts.settle,
            backing: opts.backing,
        };
        // Join and read before the first check: the editor and the viewer
        // join Matrix rooms lazily, and a late join loses history.
        for who in [Who::Editor, Who::Viewer] {
            let first = rig.first.clone();
            rig.wait_head(who, &first).await?;
        }
        Ok(rig)
    }

    /// A second document with NO version yet, owned by the owner, with the
    /// editor invited and joined, and an editor's WOPI client for it. (Spec
    /// 4.3: a lease on it omits `base_version`.)
    pub async fn empty_document(&self) -> Result<(DocId, RawWopi), Failure> {
        let owner = &self.handles.owner;
        let doc = owner.create_document(&standard_meta(), None).await?;
        owner.grant(&doc, &self.principal(Who::Editor), Role::Editor).await?;
        let editor = self.handles.editor.clone();
        editor.role(&doc).await?; // joins
        let token = self.host.register(
            doc.clone(),
            (self.backing)(Who::Editor, editor.clone() as Arc<dyn DocumentStore>, &self.handles),
            editor.principal(),
        );
        let raw = RawWopi::new(&self.base_url(), &doc, &token);
        Ok((doc, raw))
    }

    /// The principal's handle as a native caller sees it.
    pub fn native(&self, who: Who) -> &Arc<dyn DocumentStore> {
        &self.native[who.index()]
    }

    /// The Matrix user ID.
    pub fn principal(&self, who: Who) -> String {
        self.handles.get(who).principal()
    }

    /// The access token the WOPI host serves `who` from.
    pub fn token(&self, who: Who) -> &str {
        &self.tokens[who.index()]
    }

    pub fn base_url(&self) -> String {
        self.server.base_url()
    }

    /// A `WopiClientStore` for `who`: a DCP store that speaks only WOPI.
    pub fn sim(&self, who: Who) -> WopiClientStore {
        let sim = WopiClientStore::new(self.base_url(), self.principal(who), self.handles.get(who).device());
        sim.attach(self.doc.clone(), self.token(who));
        sim
    }

    /// A step-by-step WOPI client for `who`.
    pub fn raw(&self, who: Who) -> RawWopi {
        RawWopi::new(&self.base_url(), &self.doc, self.token(who))
    }

    /// Poll `probe` until it yields `Ok(Ok(..))`, for at most `self.settle`.
    /// `Ok(Err(observed))` says what the probe saw instead; when time runs out
    /// the failure names it ("expected X; still observed: Y"), so a broken
    /// store fails by a stated mismatch rather than by an opaque timeout.
    pub async fn until<T, F, Fut>(&self, what: &str, mut probe: F) -> Result<T, Failure>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<Result<T, String>, Failure>>,
    {
        let deadline = tokio::time::Instant::now() + self.settle;
        loop {
            let observed = match probe().await? {
                Ok(v) => return Ok(v),
                Err(observed) => observed,
            };
            if tokio::time::Instant::now() >= deadline {
                return Err(Failure::new(format!(
                    "expected {what}; still observed: {observed} (after {:?})",
                    self.settle
                )));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// The reader pipeline (spec 4.2, section 6) over `who`'s native handle.
    pub async fn view(&self, who: Who) -> Result<View, Failure> {
        Ok(read_view(&**self.native(who), &self.doc).await?)
    }

    /// Wait until `who` (natively) shows exactly one head, `id`.
    pub async fn wait_head(&self, who: Who, id: &str) -> Result<View, Failure> {
        let what = format!("{} to see the single head {id}", self.principal(who));
        self.until(&what, || async {
            let v = self.view(who).await?;
            if v.head_ids() == [id] {
                return Ok(Ok(v));
            }
            Ok(Err(format!("heads {:?}", v.head_ids())))
        })
        .await
    }

    /// Wait until `who` (natively) shows `n` heads.
    pub async fn wait_heads(&self, who: Who, n: usize) -> Result<View, Failure> {
        let what = format!("{} to see {n} heads", self.principal(who));
        self.until(&what, || async {
            let v = self.view(who).await?;
            if v.heads.len() == n {
                return Ok(Ok(v));
            }
            Ok(Err(format!("heads {:?}", v.head_ids())))
        })
        .await
    }

    /// Every version ID `store` lists, sorted.
    pub async fn version_ids(&self, store: &dyn DocumentStore) -> Result<Vec<String>, Failure> {
        let mut ids: Vec<String> = store.versions(&self.doc).await?.into_iter().map(|v| v.id).collect();
        ids.sort();
        Ok(ids)
    }
}
