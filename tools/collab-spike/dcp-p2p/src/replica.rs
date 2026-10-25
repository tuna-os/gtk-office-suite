//! The replica: everything that lives on the store's own thread.
//!
//! A `P2pStore` runs one OS thread with a current-thread tokio runtime and a `LocalSet`; all the
//! state below is `Rc`/`RefCell` and no future in it has to be `Send`. That is deliberate: the
//! p2panda-spaces traits return `impl Future` without a `Send` bound, and the whole stack (iroh,
//! iroh-blobs, sqlx, p2panda) then runs on that one runtime.

use std::cell::{Cell, RefCell};
use std::sync::atomic::AtomicBool;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use dcp_core::{Role, StoreError};
use iroh::endpoint::Connection;
use iroh::EndpointAddr;
use p2panda_auth::group::GroupCrdtState;
use p2panda_auth::AccessLevel;
use p2panda_core::{Hash, Operation, SigningKey, VerifyingKey};
use p2panda_encryption::crypto::x25519::SecretKey as IdentitySecret;
use p2panda_encryption::Rng;
use p2panda_spaces::manager::Manager;
use p2panda_spaces::space::SpacesState;
use p2panda_spaces::{
    AuthMessage, Credentials, SpacesStoreState, StrongRemoveResolver,
};
use p2panda_store::groups::GroupsStore;
use p2panda_store::spaces::{SpacesStore, SqliteSpacesStore};
use p2panda_store::sqlite::SqliteStoreBuilder;
use p2panda_store::{SqliteError, SqliteStore, Transaction};
use tokio::sync::{mpsc, Notify};

use crate::acl::Acl;
use crate::clock::{Clock, SystemClock};
use crate::envelope::Envelope;
use crate::ext::DcpExt;
use crate::msg::DcpForge;
use crate::net::{node_id_of, Net, NetOptions};

pub fn tr(e: impl std::fmt::Display) -> StoreError {
    StoreError::Transport(e.to_string())
}

pub type DcpManager = Manager<SqliteSpacesStore<DcpExt>, DcpForge, (), StrongRemoveResolver<()>>;
pub type AuthState = GroupCrdtState<VerifyingKey, Hash, AuthMessage<()>, ()>;

/// p2panda-spaces 0.7.1 keeps its auth graph under this fixed id (`manager.rs`, private).
const GLOBAL_GROUPS_CONTEXT_ID: &[u8] = b"global-groups-context";

#[derive(Clone)]
pub struct P2pConfig {
    /// Socket address to bind. `127.0.0.1:0` keeps everything on loopback.
    pub bind: String,
    /// `None`: no relay. A relay map makes the node dialable through that relay.
    pub relay: Option<iroh::RelayMap>,
    /// DER certificates the relay's TLS certificate may chain to (a private CA). Empty: the
    /// embedded Mozilla roots. Only read when `relay` is set.
    pub relay_ca_der: Vec<Vec<u8>>,
    /// The clock the store reads: lease stamps, `now_ms`, entry timestamps.
    pub clock: Arc<dyn Clock>,
    /// How often the anti-entropy loop asks every peer of every document for news.
    pub sync_interval: Duration,
    /// Apply the membership rule when serving sync and blob requests. `false` exists only so the
    /// tests can show that encryption alone keeps a removed member out.
    pub enforce_acl: bool,
    /// How long a blob fetch keeps trying before it gives up (it starts again on the next entry).
    pub fetch_timeout: Duration,
    /// A device id; random when `None`.
    pub device: Option<String>,
    /// Apply the reader's authorisation rule (`auth_check.rs`) when listing entries. `false`
    /// exists only for the negative control of the forged-version journey.
    pub authorize: bool,
    /// Stamp a lease with the AUTHOR's clock reading instead of the reader's first-seen time.
    /// `true` exists only for the negative control of the clock-skew test.
    pub author_stamped_leases: bool,
}

impl Default for P2pConfig {
    fn default() -> Self {
        P2pConfig {
            bind: "127.0.0.1:0".into(),
            relay: None,
            relay_ca_der: Vec::new(),
            clock: Arc::new(SystemClock),
            sync_interval: Duration::from_millis(500),
            enforce_acl: true,
            fetch_timeout: Duration::from_secs(60),
            device: None,
            authorize: true,
            author_stamped_leases: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    /// BLAKE3 of the signed body: the version / lease / update identifier.
    pub id: String,
    pub author: VerifyingKey,
    pub ts_us: u64,
    /// When THIS replica first saw the entry (own writes: when written), on this replica's clock.
    pub seen_ms: u64,
    pub heads: BTreeSet<Hash>,
    pub env: Envelope,
}

#[derive(Clone, Debug)]
pub struct BlobFetch {
    pub blob: String,
    pub bytes: u64,
    /// Wall time of `remote().fetch` alone (connect included), in microseconds.
    pub micros: u64,
}

#[derive(Default, Clone, Debug)]
pub struct Counters {
    pub ops_received: u64,
    pub ops_invalid: u64,
    pub ops_rejected: u64,
    pub ops_applied: u64,
    pub entries_invalid: u64,
    pub sync_sessions: u64,
    pub sync_failures: u64,
    pub sync_denied: u64,
}

#[derive(Default)]
pub struct DocState {
    pub authors: BTreeSet<VerifyingKey>,
    pub sync_peers: BTreeSet<VerifyingKey>,
    pub members: Vec<(VerifyingKey, AccessLevel)>,
    pub applied: HashSet<Hash>,
    pub pending: HashMap<Hash, Operation<DcpExt>>,
    pub rejected: HashMap<Hash, String>,
    pub entries: Vec<Entry>,
    pub entry_ids: HashSet<String>,
    pub blob_ready: HashSet<String>,
    pub blob_fetching: HashSet<String>,
    pub lease_lamport: u64,
    pub blob_fetches: Vec<BlobFetch>,
    pub cache: Option<Rc<SpacesStoreState<()>>>,
    pub counters: Counters,
}

/// One document: its own database, its own p2panda-spaces manager. p2panda-spaces keeps ONE auth
/// graph per manager, and a group-creation message depends on the heads of that graph, so two
/// documents in one manager would make the second document's creation depend on operations of the
/// first (which its members never receive). One manager per document keeps documents independent.
pub struct Doc {
    pub id: String,
    pub space_id: Hash,
    pub creator: VerifyingKey,
    pub store: SqliteStore,
    pub spaces_store: SqliteSpacesStore<DcpExt>,
    pub forge: DcpForge,
    pub manager: DcpManager,
    pub lock: tokio::sync::Mutex<()>,
    /// Sessions that read this document's log hold it shared; a writer that must make several
    /// operations visible together (a role change is a removal and an addition) holds it exclusive
    /// while it creates them, and so does the receiver while it stores a batch.
    pub serving: tokio::sync::RwLock<()>,
    pub st: RefCell<DocState>,
}

impl Doc {
    /// Write the manager's returned state back (p2panda-spaces 0.7.1 returns states and leaves
    /// persisting them to the caller; its `*_persisted` helpers exist only behind `test_utils`).
    pub async fn persist(
        &self,
        groups_y: Option<&AuthState>,
        space_y: Option<SpacesState<()>>,
    ) -> Result<(), StoreError> {
        let permit = self.store.begin().await.map_err(tr)?;
        let res: Result<(), SqliteError> = async {
            if let Some(g) = groups_y {
                <SqliteSpacesStore<DcpExt> as GroupsStore<AuthMessage<()>, ()>>::set_groups_state_tx(
                    &self.spaces_store,
                    Hash::digest(GLOBAL_GROUPS_CONTEXT_ID),
                    g,
                )
                .await?;
            }
            if let Some(y) = space_y {
                let id = y.space_id;
                let st: SpacesStoreState<()> = y.into();
                <SqliteSpacesStore<DcpExt> as SpacesStore<SpacesStoreState<()>>>::set_space_state_tx(
                    &self.spaces_store,
                    &id,
                    &st,
                )
                .await?;
            }
            Ok(())
        }
        .await;
        match res {
            Ok(()) => {
                self.store.commit(permit).await.map_err(tr)?;
                self.st.borrow_mut().cache = None;
                Ok(())
            }
            Err(e) => {
                let _ = self.store.rollback(permit).await;
                Err(tr(e))
            }
        }
    }

    /// The space-local auth state (cached until the next `persist`).
    pub async fn space_state(&self) -> Result<Rc<SpacesStoreState<()>>, StoreError> {
        if let Some(s) = self.st.borrow().cache.clone() {
            return Ok(s);
        }
        let permit = self.store.begin().await.map_err(tr)?;
        let r = <SqliteSpacesStore<DcpExt> as SpacesStore<SpacesStoreState<()>>>::get_space_state_tx(
            &self.spaces_store,
            &self.space_id,
        )
        .await;
        self.store.commit(permit).await.map_err(tr)?;
        let state = r
            .map_err(tr)?
            .ok_or_else(|| StoreError::NotFound(format!("document {} has no state yet", self.id)))?;
        let rc = Rc::new(state);
        self.st.borrow_mut().cache = Some(rc.clone());
        Ok(rc)
    }

    /// Recompute the member list from p2panda-spaces and publish it to the blob/sync gate.
    pub async fn refresh_members(&self, acl: &Acl) -> Result<(), StoreError> {
        let Some(space) = self.manager.space(self.space_id).await.map_err(tr)? else {
            return Ok(());
        };
        let members = space.members().await.map_err(tr)?;
        let list: Vec<(VerifyingKey, AccessLevel)> =
            members.iter().map(|(id, a)| (*id, a.level.clone())).collect();
        let ids: HashSet<iroh::EndpointId> = list
            .iter()
            .filter_map(|(k, _)| node_id_of(k).ok())
            .collect();
        acl.set_members(&self.id, ids);
        self.st.borrow_mut().members = list;
        Ok(())
    }

    pub fn level_of(&self, who: &VerifyingKey) -> Option<AccessLevel> {
        self.st
            .borrow()
            .members
            .iter()
            .find(|(k, _)| k == who)
            .map(|(_, l)| l.clone())
    }

    /// The logs this side offers in a sync session: one per author it holds operations from.
    pub fn logs(&self, me: &VerifyingKey) -> BTreeMap<VerifyingKey, Vec<String>> {
        let st = self.st.borrow();
        let mut m: BTreeMap<VerifyingKey, Vec<String>> = BTreeMap::new();
        for a in st.authors.iter().chain(std::iter::once(me)) {
            m.insert(*a, vec![self.id.clone()]);
        }
        m
    }
}

pub fn role_of_level(l: &AccessLevel) -> Option<Role> {
    match l {
        AccessLevel::Manage => Some(Role::Owner),
        AccessLevel::Write => Some(Role::Editor),
        AccessLevel::Read => Some(Role::Viewer),
        AccessLevel::Pull => None,
    }
}

/// Both ends of one in-flight sync session per (document, peer).
#[derive(Default)]
pub struct Slot {
    pub running: bool,
    pub again: bool,
}

pub struct Inner {
    pub cfg: P2pConfig,
    pub key: SigningKey,
    pub me: VerifyingKey,
    pub principal: String,
    pub device: String,
    pub identity_secret: IdentitySecret,
    pub rng: Rng,
    pub acl: Arc<Acl>,
    pub net: Net,
    pub docs: RefCell<HashMap<String, Rc<Doc>>>,
    pub peers: RefCell<HashMap<VerifyingKey, EndpointAddr>>,
    pub conns: RefCell<HashMap<VerifyingKey, Connection>>,
    pub slots: RefCell<HashMap<(String, VerifyingKey), Slot>>,
    pub changed: Notify,
    pub truncate_next: Arc<AtomicBool>,
    pub online: Cell<bool>,
    /// What iroh reports about the paths of each connection (observation only).
    pub pathlog: crate::pathlog::SharedLog,
}

impl Inner {
    pub async fn new(cfg: P2pConfig) -> Result<Rc<Inner>, StoreError> {
        let key = SigningKey::generate();
        let me = key.verifying_key();
        let rng = Rng::default();
        let identity_secret = IdentitySecret::from_rng(&rng).map_err(tr)?;
        let device = match &cfg.device {
            Some(d) => d.clone(),
            None => {
                let r: [u8; 4] = rng.random_array().map_err(tr)?;
                format!("dev-{}", hex::encode(r))
            }
        };
        let acl = Arc::new(Acl::new(cfg.enforce_acl));
        let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();
        let net = Net::spawn(
            &key,
            &NetOptions {
                bind: cfg.bind.clone(),
                relay: cfg.relay.clone(),
                relay_ca_der: cfg.relay_ca_der.clone(),
            },
            acl.clone(),
            incoming_tx,
        )
        .await
        .map_err(StoreError::Transport)?;
        let inner = Rc::new(Inner {
            cfg,
            principal: crate::envelope::principal_of(&me),
            key,
            me,
            device,
            identity_secret,
            rng,
            acl,
            net,
            docs: RefCell::default(),
            peers: RefCell::default(),
            conns: RefCell::default(),
            slots: RefCell::default(),
            changed: Notify::new(),
            truncate_next: Arc::new(AtomicBool::new(false)),
            online: Cell::new(true),
            pathlog: Rc::default(),
        });
        tokio::task::spawn_local(inner.clone().accept_loop(incoming_rx));
        tokio::task::spawn_local(inner.clone().anti_entropy());
        Ok(inner)
    }

    pub fn now_ms(&self) -> u64 {
        self.cfg.clock.now_ms()
    }

    /// Stop the network and drop every document, inside the runtime that owns them.
    pub async fn shutdown(&self) {
        self.online.set(false);
        self.acl.set_online(false);
        self.conns.borrow_mut().clear();
        let _ = self.net.router.shutdown().await;
        self.docs.borrow_mut().clear();
    }

    pub fn doc(&self, id: &str) -> Result<Rc<Doc>, StoreError> {
        self.docs
            .borrow()
            .get(id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("document {id}")))
    }

    /// A fresh per-document database and manager. The prekeys are generated here: the contact card
    /// a peer needs before it can add this principal to the document is therefore per document.
    pub async fn new_doc(
        &self,
        id: String,
        space_id: Hash,
        creator: VerifyingKey,
    ) -> Result<Rc<Doc>, StoreError> {
        let store = SqliteStoreBuilder::memory().build().await.map_err(tr)?;
        let spaces_store = SqliteSpacesStore::<DcpExt>::new(store.clone());
        let forge = DcpForge::new(self.key.clone(), store.clone(), self.cfg.clock.clone());
        let credentials = Credentials::from_keys(self.key.clone(), self.identity_secret.clone());
        let rng = Rng::from_rng(&self.rng).map_err(tr)?;
        let manager = DcpManager::new(spaces_store.clone(), forge.clone(), credentials, rng)
            .map_err(tr)?;
        // Generates the pre-keys the first time (it forges nothing).
        manager.me().await.map_err(tr)?;
        let doc = Rc::new(Doc {
            id: id.clone(),
            space_id,
            creator,
            store,
            spaces_store,
            forge,
            manager,
            lock: tokio::sync::Mutex::new(()),
            serving: tokio::sync::RwLock::new(()),
            st: RefCell::new(DocState::default()),
        });
        doc.st.borrow_mut().authors.insert(self.me);
        self.docs.borrow_mut().insert(id, doc.clone());
        Ok(doc)
    }
}
