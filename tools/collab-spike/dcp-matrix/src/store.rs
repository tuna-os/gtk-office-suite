//! `MatrixStore`: the Matrix binding of DCP draft 1 (spec 8.1) as a
//! [`DocumentStore`].
//!
//! One document is one end-to-end encrypted room of type
//! `org.tunaos.dcp.document`. Metadata and the lease are state events,
//! versions and updates are timeline events (so they are megolm-encrypted),
//! a version's file is an encrypted attachment. Everything the spec leaves to
//! "the binding" is decided here and measured in
//! `docs/rfc/0004-spike-a-results.md`.

use std::collections::HashMap;
use std::future::Future;
use std::io::Cursor;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dcp_core::{
    DocId, DocumentStore, FileRef, Lease, Locator, Meta, NewFile, Payload, Result, Role, Stamped,
    StoreError, Update, Version,
};
use matrix_sdk::authentication::matrix::MatrixSession;
use matrix_sdk::config::{RequestConfig, SyncSettings};
use matrix_sdk::media::{MediaFormat, MediaRequestParameters};
use matrix_sdk::room::MessagesOptions;
use matrix_sdk::ruma::api::client::room::{create_room, get_room_event};
use matrix_sdk::ruma::api::client::state::get_state_events;
use matrix_sdk::ruma::events::room::member::MembershipState;
use matrix_sdk::ruma::events::room::{EncryptedFile, MediaSource};
use matrix_sdk::ruma::events::AnyInitialStateEvent;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{assign, EventId, OwnedMxcUri, RoomId, RoomVersionId, UInt, UserId};
use matrix_sdk::store::RoomLoadSettings;
use matrix_sdk::{reqwest, Client, Room, RoomMemberships, RoomState, SessionMeta, SessionTokens};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::{watch, Mutex};

use crate::config::{ClockSource, MatrixConfig};
use crate::error::{classify, to_store, Class};
use crate::queue::{Job, RoomQueue, Stats, StatsSnapshot};
use crate::state::{self, Snapshot, ENCRYPTED, LEVEL_EDITOR, LEVEL_OWNER};
use crate::synapse::{self, Registered};

/// Everything needed to resume a device's session.
#[derive(Clone, Debug)]
pub struct Credentials {
    pub user_id: String,
    pub device_id: String,
    pub access_token: String,
}

impl From<&Registered> for Credentials {
    fn from(r: &Registered) -> Self {
        Credentials { user_id: r.user_id.clone(), device_id: r.device_id.clone(), access_token: r.access_token.clone() }
    }
}

/// What one pass over a room's history found. Tests and the measurements read
/// the counters; `versions()` / `updates()` only return the accepted items.
#[derive(Clone, Debug, Default)]
pub struct ReadReport {
    pub versions: Vec<Stamped<Version>>,
    pub updates: Vec<Stamped<Update>>,
    /// Encrypted events that could not be decrypted (no room key for this device).
    pub undecryptable: usize,
    /// DCP events from a sender below the editor level, in a room whose
    /// power-level table does not stop the server from accepting them.
    pub rejected_unauthorised: usize,
    /// DCP events whose content failed validation or named another author.
    pub rejected_invalid: usize,
    /// DCP events sent unencrypted into an encrypted room.
    pub rejected_plaintext: usize,
}

struct Item {
    id: String,
    ts: u64,
    sender: String,
    device: Option<String>,
    ty: String,
    content: Value,
    encrypted: bool,
}

struct Inner {
    client: Client,
    cfg: MatrixConfig,
    user_id: String,
    device_id: String,
    http: reqwest::Client,
    stats: Arc<Stats>,
    queues: Mutex<HashMap<String, Arc<RoomQueue>>>,
    sync_task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    sync_ticks: watch::Sender<u64>,
    init_warnings: std::sync::Mutex<Vec<String>>,
    /// `(origin_server_ts of our newest own event, local instant we learned it)`.
    clock_sample: std::sync::Mutex<Option<(u64, Instant)>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Ok(mut t) = self.sync_task.lock() {
            if let Some(t) = t.take() {
                t.abort();
            }
        }
    }
}

/// One principal's device, as a DCP [`DocumentStore`] over Matrix.
#[derive(Clone)]
pub struct MatrixStore {
    inner: Arc<Inner>,
}

impl MatrixStore {
    /// Restore a session, start syncing, wait for the first sync (which uploads
    /// this device's keys) and set up cross-signing, which matrix-sdk needs
    /// before it will share room history with a newly invited member.
    pub async fn connect(cfg: MatrixConfig, creds: &Credentials) -> Result<MatrixStore> {
        let user_id = UserId::parse(&creds.user_id).map_err(|e| StoreError::Invalid(format!("user id: {e}")))?;
        // matrix-sdk retries 429 and network errors forever by default; the
        // binding's send queue does that itself so that it can count them.
        #[allow(unused_mut)]
        let mut builder = Client::builder()
            .homeserver_url(&cfg.homeserver)
            .request_config(RequestConfig::default().disable_retry());
        #[cfg(feature = "sqlite")]
        if let Some(dir) = &cfg.store_dir {
            builder = builder.sqlite_store(dir, None);
        }
        let client = builder.build().await.map_err(|e| StoreError::Transport(format!("client build: {e}")))?;
        let session = MatrixSession {
            meta: SessionMeta { user_id: user_id.clone(), device_id: creds.device_id.as_str().into() },
            tokens: SessionTokens { access_token: creds.access_token.clone(), refresh_token: None },
        };
        client
            .matrix_auth()
            .restore_session(session, RoomLoadSettings::default())
            .await
            .map_err(|e| to_store(e, "restore session"))?;

        let (ticks, _) = watch::channel(0u64);
        let inner = Arc::new(Inner {
            client: client.clone(),
            http: reqwest::Client::new(),
            user_id: creds.user_id.clone(),
            device_id: creds.device_id.clone(),
            stats: Arc::new(Stats::default()),
            queues: Mutex::new(HashMap::new()),
            sync_task: std::sync::Mutex::new(None),
            sync_ticks: ticks,
            init_warnings: std::sync::Mutex::new(Vec::new()),
            clock_sample: std::sync::Mutex::new(None),
            cfg,
        });
        let store = MatrixStore { inner };

        if store.inner.cfg.background_sync {
            let task = tokio::spawn(sync_loop(
                client.clone(),
                store.inner.cfg.sync_timeout,
                store.inner.sync_ticks.clone(),
            ));
            *store.inner.sync_task.lock().expect("sync_task lock") = Some(task);
            let mut rx = store.inner.sync_ticks.subscribe();
            tokio::time::timeout(Duration::from_secs(60), async {
                while *rx.borrow_and_update() == 0 {
                    if rx.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await
            .map_err(|_| StoreError::Transport("first sync did not complete in 60 s".into()))?;
        } else {
            client
                .sync_once(SyncSettings::new().timeout(Duration::ZERO))
                .await
                .map_err(|e| to_store(e, "first sync"))?;
        }

        match client.encryption().bootstrap_cross_signing_if_needed(None).await {
            Err(e) => store.warn(format!("cross-signing bootstrap failed: {e}")),
            // matrix-sdk shares history on invite only once it knows its own
            // identity, which needs one more /keys/query after the upload.
            Ok(()) => {
                if let Err(e) = client.encryption().request_user_identity(&user_id).await {
                    store.warn(format!("own identity query failed: {e}"));
                }
            }
        }
        Ok(store)
    }

    /// The underlying matrix-sdk client, for measurements and experiments that
    /// need to go below the DCP surface.
    pub fn client(&self) -> &Client {
        &self.inner.client
    }

    pub fn config(&self) -> &MatrixConfig {
        &self.inner.cfg
    }

    pub fn stats(&self) -> StatsSnapshot {
        self.inner.stats.snapshot()
    }

    /// Non-fatal problems seen while connecting (for example a failed
    /// cross-signing bootstrap).
    pub fn init_warnings(&self) -> Vec<String> {
        self.inner.init_warnings.lock().map(|w| w.clone()).unwrap_or_default()
    }

    fn warn(&self, w: String) {
        if let Ok(mut v) = self.inner.init_warnings.lock() {
            v.push(w);
        }
    }

    /// Wait for one more sync response to be processed (background sync), or run
    /// one (manual mode).
    pub async fn sync_now(&self) -> Result<()> {
        if self.inner.cfg.background_sync {
            let mut rx = self.inner.sync_ticks.subscribe();
            rx.borrow_and_update();
            tokio::time::timeout(self.inner.cfg.sync_timeout + Duration::from_secs(30), rx.changed())
                .await
                .map_err(|_| StoreError::Transport("no sync completed".into()))?
                .map_err(|_| StoreError::Transport("sync loop stopped".into()))
        } else {
            self.inner
                .client
                .sync_once(SyncSettings::new().timeout(Duration::ZERO))
                .await
                .map(|_| ())
                .map_err(|e| to_store(e, "sync"))
        }
    }

    async fn poll_until<F, Fut>(&self, timeout: Duration, mut f: F) -> bool
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = bool>,
    {
        let end = Instant::now() + timeout;
        loop {
            if f().await {
                return true;
            }
            if Instant::now() >= end {
                return false;
            }
            if !self.inner.cfg.background_sync {
                let _ = self.inner.client.sync_once(SyncSettings::new().timeout(Duration::ZERO)).await;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Retry on `M_LIMIT_EXCEEDED` only, honouring `retry_after_ms`.
    async fn retry_rl<T, E, F, Fut>(&self, mut f: F) -> std::result::Result<T, matrix_sdk::Error>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = std::result::Result<T, E>>,
        E: Into<matrix_sdk::Error>,
    {
        let cfg = &self.inner.cfg;
        let start = Instant::now();
        let mut backoff = cfg.backoff_min;
        loop {
            let e: matrix_sdk::Error = match f().await {
                Ok(v) => return Ok(v),
                Err(e) => e.into(),
            };
            let Class::RateLimited(suggested) = classify(&e) else { return Err(e) };
            self.inner.stats.rate_limited.fetch_add(1, Ordering::Relaxed);
            let wait = suggested.unwrap_or(backoff);
            backoff = (backoff * 2).min(cfg.backoff_max);
            if start.elapsed() + wait > cfg.send_deadline {
                return Err(e);
            }
            self.inner.stats.waited_ms.fetch_add(wait.as_millis() as u64, Ordering::Relaxed);
            tokio::time::sleep(wait).await;
        }
    }

    /// The room for `doc`, joining on first use if this principal was invited
    /// (granting is inviting; there is no separate accept call in DCP). A
    /// principal that was removed gets `Forbidden` because the server refuses
    /// the join.
    async fn room_for(&self, doc: &str) -> Result<Room> {
        let rid = RoomId::parse(doc).map_err(|e| StoreError::Invalid(format!("doc id {doc:?}: {e}")))?;
        let client = &self.inner.client;
        if let Some(room) = client.get_room(&rid) {
            if room.state() == RoomState::Joined {
                return Ok(room);
            }
        }
        // Joining by room ID works as soon as the server has the invite, but
        // matrix-sdk imports the inviter's shared history only if it had seen the
        // invite by /sync *before* the join (it records who invited us then).
        // Join too early and every version published before the invite stays
        // undecryptable (measured, see the results doc). So wait for our own
        // sync to deliver the invite. A room we already know we left is given
        // a short wait (re-invite), an unknown one a longer one.
        let known_left = client.get_room(&rid).is_some();
        let wait = if known_left { Duration::from_secs(2) } else { Duration::from_secs(10) };
        self.poll_until(wait, || async {
            client.get_room(&rid).is_some_and(|r| matches!(r.state(), RoomState::Invited | RoomState::Joined))
        })
        .await;
        if let Some(room) = client.get_room(&rid) {
            if room.state() == RoomState::Joined {
                return Ok(room);
            }
        }
        self.retry_rl(|| client.join_room_by_id(&rid))
            .await
            .map_err(|e| to_store(e, &format!("join {doc}")))
    }

    async fn queue_for(&self, room: &Room) -> Arc<RoomQueue> {
        let mut qs = self.inner.queues.lock().await;
        qs.entry(room.room_id().to_string())
            .or_insert_with(|| {
                Arc::new(RoomQueue::spawn(room.clone(), self.inner.cfg.clone(), self.inner.stats.clone()))
            })
            .clone()
    }

    async fn snapshot_of(&self, room: &Room) -> Result<Snapshot> {
        let client = &self.inner.client;
        let rid = room.room_id().to_owned();
        let resp = self
            .retry_rl(|| async { client.send(get_state_events::v3::Request::new(rid.clone())).await })
            .await
            .map_err(|e| to_store(e, "read room state"))?;
        let events: Vec<Value> =
            resp.room_state.iter().filter_map(|r| serde_json::from_str(r.json().get()).ok()).collect();
        let snap = Snapshot::from_events(&events);
        if snap.room_type.as_deref() != Some(state::ROOM_TYPE) {
            return Err(StoreError::NotFound(format!("{} is not a DCP document room", room.room_id())));
        }
        Ok(snap)
    }

    /// The server-authoritative view of a document room (state, power levels,
    /// membership). Public for tests and measurements.
    pub async fn snapshot(&self, doc: &DocId) -> Result<Snapshot> {
        let room = self.room_for(doc).await?;
        self.snapshot_of(&room).await
    }

    /// `origin_server_ts` and sender of an event, straight from the server.
    async fn event_stamp(&self, room: &Room, event_id: &str) -> Result<(u64, String)> {
        let eid = EventId::parse(event_id).map_err(|e| StoreError::Invalid(format!("event id: {e}")))?;
        let client = &self.inner.client;
        let rid = room.room_id().to_owned();
        let resp = self
            .retry_rl(|| async { client.send(get_room_event::v3::Request::new(rid.clone(), eid.clone())).await })
            .await
            .map_err(|e| to_store(e, "read event"))?;
        let v: Value = serde_json::from_str(resp.event.json().get()).map_err(|e| StoreError::Transport(e.to_string()))?;
        let ts = v["origin_server_ts"].as_u64().ok_or_else(|| StoreError::Transport("event without origin_server_ts".into()))?;
        if let Ok(mut c) = self.inner.clock_sample.lock() {
            *c = Some((ts, Instant::now()));
        }
        Ok((ts, v["sender"].as_str().unwrap_or_default().to_owned()))
    }

    /// Refuse to encrypt while this device still counts a member the homeserver
    /// says is gone (see `MatrixConfig::guard_stale_members`).
    async fn ensure_members_current(&self, room: &Room, snap: &Snapshot) -> Result<()> {
        if !self.inner.cfg.guard_stale_members {
            return Ok(());
        }
        let departed = |local: Vec<String>| -> Vec<String> {
            local
                .into_iter()
                .filter(|u| !matches!(snap.membership.get(u).map(String::as_str), Some("join") | Some("invite")))
                .collect()
        };
        let local_view = || async {
            room.members_no_sync(RoomMemberships::ACTIVE)
                .await
                .map(|ms| ms.into_iter().map(|m| m.user_id().to_string()).collect::<Vec<_>>())
                .unwrap_or_default()
        };
        if departed(local_view().await).is_empty() {
            return Ok(());
        }
        let ok = self.poll_until(Duration::from_secs(15), || async { departed(local_view().await).is_empty() }).await;
        if ok {
            Ok(())
        } else {
            Err(StoreError::Transport("this device still lists a removed member; refusing to encrypt for them".into()))
        }
    }

    fn require_editor(&self, snap: &Snapshot, what: &str) -> Result<()> {
        let role = snap.power.role_of(&self.inner.user_id);
        if role >= Role::Editor {
            Ok(())
        } else {
            Err(StoreError::Forbidden(format!("{what}: role {role:?} is below editor")))
        }
    }

    fn require_owner(&self, snap: &Snapshot, what: &str) -> Result<()> {
        let role = snap.power.role_of(&self.inner.user_id);
        if role >= Role::Owner {
            Ok(())
        } else {
            Err(StoreError::Forbidden(format!("{what}: role {role:?} is not owner")))
        }
    }

    /// One backwards pass over the room timeline: every DCP item that could be
    /// decrypted, plus how many encrypted events could not be.
    async fn read_timeline(&self, room: &Room) -> Result<(Vec<Item>, usize)> {
        let mut items = Vec::new();
        let mut utd = 0usize;
        let mut from: Option<String> = None;
        for _ in 0..100_000 {
            let page = self
                .retry_rl(|| {
                    let mut opts = MessagesOptions::backward();
                    opts.limit = UInt::from(100u32);
                    opts.from = from.clone();
                    room.messages(opts)
                })
                .await
                .map_err(|e| to_store(e, "read history"))?;
            for ev in &page.chunk {
                if ev.kind.is_utd() {
                    utd += 1;
                    continue;
                }
                let Ok(raw) = serde_json::from_str::<Value>(ev.raw().json().get()) else { continue };
                let Some(ty) = raw["type"].as_str() else { continue };
                if !ty.starts_with("org.tunaos.dcp.") {
                    continue;
                }
                let (Some(id), Some(sender)) = (raw["event_id"].as_str(), raw["sender"].as_str()) else { continue };
                items.push(Item {
                    id: id.to_owned(),
                    ts: raw["origin_server_ts"].as_u64().unwrap_or(0),
                    sender: sender.to_owned(),
                    device: ev.encryption_info().and_then(|i| i.sender_device.as_ref().map(|d| d.to_string())),
                    ty: ty.to_owned(),
                    content: raw["content"].clone(),
                    encrypted: ev.encryption_info().is_some(),
                });
            }
            match page.end {
                Some(t) if !page.chunk.is_empty() => from = Some(t),
                _ => break,
            }
        }
        Ok((items, utd))
    }

    /// `read_timeline`, waiting up to `utd_wait` for room keys that were sent
    /// but have not reached this device yet.
    async fn read_timeline_waiting(&self, room: &Room) -> Result<(Vec<Item>, usize)> {
        let end = Instant::now() + self.inner.cfg.utd_wait;
        loop {
            let (items, utd) = self.read_timeline(room).await?;
            if utd == 0 || Instant::now() >= end {
                return Ok((items, utd));
            }
            if !self.inner.cfg.background_sync {
                let _ = self.inner.client.sync_once(SyncSettings::new().timeout(Duration::ZERO)).await;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// Read versions and updates in one pass and say what was filtered out.
    pub async fn read_report(&self, doc: &DocId) -> Result<ReadReport> {
        let room = self.room_for(doc).await?;
        let snap = self.snapshot_of(&room).await?;
        let (items, utd) = self.read_timeline_waiting(&room).await?;
        let mut rep = ReadReport { undecryptable: utd, ..Default::default() };
        // When the server gates `m.room.encrypted` at the editor level it has
        // already refused unauthorised senders at send time, and a later
        // demotion or removal must not hide what the member legitimately
        // published. When it does not (the spec's table taken literally), the
        // only defence is the reader's check against the *current* levels.
        let gated = snap.power.server_gates_encrypted_timeline();
        for it in items {
            if it.ty != state::VERSION && it.ty != state::UPDATE {
                continue;
            }
            if snap.encrypted && !it.encrypted {
                rep.rejected_plaintext += 1;
                continue;
            }
            if self.inner.cfg.reader_checks_senders && !gated && snap.power.level_of(&it.sender) < LEVEL_EDITOR {
                rep.rejected_unauthorised += 1;
                continue;
            }
            let mut invalid = false;
            if it.ty == state::VERSION {
                match dcp_core::schema::parse::<Version>(dcp_core::schema::Kind::Version, &it.content) {
                    Ok(v) if v.created_by.principal == it.sender => {
                        let mut s = Stamped::new(it.id, it.ts, it.sender, v);
                        if let Some(d) = it.device {
                            s = s.with_device(d);
                        }
                        rep.versions.push(s);
                    }
                    _ => invalid = true,
                }
            } else {
                match dcp_core::schema::parse::<Update>(dcp_core::schema::Kind::Update, &it.content) {
                    Ok(u) => {
                        let mut s = Stamped::new(it.id, it.ts, it.sender, u);
                        if let Some(d) = it.device {
                            s = s.with_device(d);
                        }
                        rep.updates.push(s);
                    }
                    Err(_) => invalid = true,
                }
            }
            if invalid {
                rep.rejected_invalid += 1;
                self.inner.stats.ignored_invalid.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(rep)
    }

    /// Send a timeline event with no role check and no queue: what a malicious
    /// or buggy member's client could do. The homeserver's answer is the
    /// result. For tests of server-side enforcement.
    pub async fn send_unchecked(&self, doc: &DocId, event_type: &str, content: Value) -> Result<String> {
        let room = self.room_for(doc).await?;
        room.send_raw(event_type, content)
            .await
            .map(|r| r.response.event_id.to_string())
            .map_err(|e| to_store(e, "unchecked send"))
    }

    /// Enqueue a timeline event on the room's ordered, rate-limited send queue
    /// with no preflight request (no role check, no state read, no timestamp
    /// read-back). The queue keeps retrying network failures and 429s until its
    /// deadline, so this is what survives an outage that begins after the call.
    /// For tests and measurements.
    pub async fn send_queued(&self, doc: &DocId, event_type: &str, content: Value) -> Result<String> {
        let room = self.room_for(doc).await?;
        let q = self.queue_for(&room).await;
        q.send(Job::Timeline { event_type: event_type.to_owned(), content }).await
    }

    /// Send a state event with no role check and no queue. For tests.
    pub async fn send_state_unchecked(
        &self,
        doc: &DocId,
        event_type: &str,
        state_key: &str,
        content: Value,
    ) -> Result<String> {
        let room = self.room_for(doc).await?;
        room.send_state_event_raw(event_type, state_key, content)
            .await
            .map(|r| r.event_id.to_string())
            .map_err(|e| to_store(e, "unchecked state send"))
    }

    async fn publish_timeline<T: serde::Serialize>(
        &self,
        room: &Room,
        event_type: &str,
        body: &T,
    ) -> Result<(String, u64)> {
        let content = serde_json::to_value(body).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let q = self.queue_for(room).await;
        let id = q.send(Job::Timeline { event_type: event_type.to_owned(), content }).await?;
        let (ts, _) = self.event_stamp(room, &id).await?;
        Ok((id, ts))
    }

    async fn wait_member(&self, room: &Room, user: &UserId, want: &[MembershipState], absent_ok: bool) -> bool {
        self.poll_until(Duration::from_secs(20), || async {
            match room.get_member_no_sync(user).await {
                Ok(Some(m)) => want.contains(m.membership()),
                Ok(None) => absent_ok,
                Err(_) => false,
            }
        })
        .await
    }

    fn power_level_override(&self) -> Value {
        let mut events = json!({
            "m.room.name": 50,
            "m.room.power_levels": 100,
            "m.room.history_visibility": 100,
            "m.room.canonical_alias": 50,
            "m.room.avatar": 50,
            "m.room.tombstone": 100,
            "m.room.server_acl": 100,
            "m.room.encryption": 100,
        });
        events[state::META] = json!(100);
        events[state::LEASE] = json!(50);
        events[state::VERSION] = json!(50);
        events[state::UPDATE] = json!(50);
        events[state::CLOCK] = json!(50);
        if self.inner.cfg.enforce_encrypted_pl {
            events[ENCRYPTED] = json!(50);
        }
        json!({
            "events": events,
            "users_default": 0,
            "events_default": 0,
            "state_default": 100,
            "invite": 100,
            "kick": 100,
            "ban": 100,
            "redact": 50,
        })
    }

    async fn upload_verified(&self, file: &NewFile) -> Result<FileRef> {
        let client = &self.inner.client;
        let enc = self
            .retry_rl(|| async {
                let mut cur = Cursor::new(file.bytes.as_slice());
                client.upload_encrypted_file(&mut cur).await
            })
            .await
            .map_err(|e| to_store(e, "upload"))?;
        // Version rule 2: fetch what was stored and check it before publishing.
        let back = client
            .media()
            .get_media_content(
                &MediaRequestParameters { source: MediaSource::Encrypted(Box::new(enc.clone())), format: MediaFormat::File },
                false,
            )
            .await
            .map_err(|e| to_store(e, "verify upload"))?;
        let want = hex::encode(Sha256::digest(&file.bytes));
        if back.len() != file.bytes.len() || hex::encode(Sha256::digest(&back)) != want {
            return Err(StoreError::Transport("uploaded file does not read back identically; not publishing".into()));
        }
        Ok(FileRef {
            mime: file.mime.clone(),
            size: file.bytes.len() as u64,
            sha256: want,
            locator: Locator::MatrixEncryptedMedia {
                file: serde_json::to_value(&enc).map_err(|e| StoreError::Invalid(e.to_string()))?,
            },
        })
    }

    async fn publish_version_in(
        &self,
        room: &Room,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
    ) -> Result<Stamped<Version>> {
        let file_ref = self.upload_verified(&file).await?;
        let version = Version {
            dcp: 1,
            parents,
            file: file_ref,
            created_by: dcp_core::Author { principal: self.inner.user_id.clone(), device: self.inner.device_id.clone() },
            message,
            covers: None,
        };
        let (id, ts) = self.publish_timeline(room, state::VERSION, &version).await?;
        Ok(Stamped::new(id, ts, self.inner.user_id.clone(), version).with_device(self.inner.device_id.clone()))
    }
}

async fn sync_loop(client: Client, timeout: Duration, ticks: watch::Sender<u64>) {
    let settings = SyncSettings::new().timeout(timeout);
    let mut n = 0u64;
    loop {
        match client.sync_once(settings.clone()).await {
            Ok(_) => {
                n += 1;
                ticks.send_replace(n);
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
}

#[async_trait]
impl DocumentStore for MatrixStore {
    fn principal(&self) -> String {
        self.inner.user_id.clone()
    }

    fn device(&self) -> String {
        self.inner.device_id.clone()
    }

    async fn create_document(&self, meta: &Meta, first: Option<NewFile>) -> Result<DocId> {
        let meta_json = serde_json::to_value(meta).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let initial_state: Vec<Raw<AnyInitialStateEvent>> = vec![
            raw_json(json!({"type": "m.room.encryption", "state_key": "", "content": {
                "algorithm": "m.megolm.v1.aes-sha2", "rotation_period_ms": 604_800_000u64, "rotation_period_msgs": 100
            }}))?,
            raw_json(json!({"type": "m.room.history_visibility", "state_key": "", "content": {"history_visibility": "shared"}}))?,
            raw_json(json!({"type": state::META, "state_key": "", "content": meta_json}))?,
        ];
        let room_version = match &self.inner.cfg.room_version {
            Some(v) => Some(
                serde_json::from_value::<RoomVersionId>(json!(v))
                    .map_err(|e| StoreError::Invalid(format!("room version: {e}")))?,
            ),
            None => None,
        };
        let request = assign!(create_room::v3::Request::new(), {
            name: Some(meta.title.clone()),
            preset: Some(create_room::v3::RoomPreset::PrivateChat),
            creation_content: Some(raw_json(json!({"type": state::ROOM_TYPE, "m.federate": false}))?),
            initial_state: initial_state,
            power_level_content_override: Some(raw_json(self.power_level_override())?),
            room_version: room_version,
        });
        let client = &self.inner.client;
        let room = self
            .retry_rl(|| client.create_room(request.clone()))
            .await
            .map_err(|e| to_store(e, "create room"))?;
        // Make sure the SDK knows the room is encrypted before the first send.
        let encrypted = self
            .poll_until(Duration::from_secs(20), || async {
                room.latest_encryption_state().await.map(|s| s.is_encrypted()).unwrap_or(false)
            })
            .await;
        if !encrypted {
            return Err(StoreError::Transport("new room did not report encryption".into()));
        }
        if let Some(f) = first {
            self.publish_version_in(&room, vec![], f, None).await?;
        }
        Ok(room.room_id().to_string())
    }

    async fn meta(&self, doc: &DocId) -> Result<Meta> {
        let snap = self.snapshot(doc).await?;
        snap.meta.map(|m| m.body).ok_or_else(|| StoreError::NotFound("no valid org.tunaos.dcp.meta".into()))
    }

    async fn role(&self, doc: &DocId) -> Result<Role> {
        let snap = self.snapshot(doc).await?;
        match snap.membership.get(&self.inner.user_id).map(String::as_str) {
            Some("join") => Ok(snap.power.role_of(&self.inner.user_id)),
            other => Err(StoreError::Forbidden(format!("membership is {other:?}"))),
        }
    }

    async fn grant(&self, doc: &DocId, principal: &str, role: Role) -> Result<()> {
        let level = match role {
            Role::Viewer => None,
            Role::Editor => Some(LEVEL_EDITOR),
            Role::Owner => Some(LEVEL_OWNER),
            Role::Commenter => {
                return Err(StoreError::Invalid("commenter is reserved until the comment event is defined (spec 8.1)".into()))
            }
        };
        let uid = UserId::parse(principal).map_err(|e| StoreError::Invalid(format!("principal: {e}")))?;
        let room = self.room_for(doc).await?;
        let snap = self.snapshot_of(&room).await?;
        self.require_owner(&snap, "grant")?;

        match snap.membership.get(principal).map(String::as_str) {
            Some("join") | Some("invite") => {}
            Some("ban") => return Err(StoreError::Forbidden(format!("{principal} is banned"))),
            _ => {
                self.retry_rl(|| room.invite_user_by_id(&uid)).await.map_err(|e| to_store(e, "invite"))?;
            }
        }

        let current = snap.power.users.get(principal).copied();
        if current != level {
            let mut content = snap.power_levels_content.clone();
            if !content["users"].is_object() {
                content["users"] = json!({});
            }
            match level {
                Some(l) => content["users"][principal] = json!(l),
                None => {
                    content["users"].as_object_mut().map(|o| o.remove(principal));
                }
            }
            let q = self.queue_for(&room).await;
            q.send(Job::State {
                event_type: "m.room.power_levels".into(),
                state_key: String::new(),
                content,
            })
            .await?;
        }
        // Do not return before our own view includes the new member: the next
        // publish encrypts for the members this device knows about.
        if !self.wait_member(&room, &uid, &[MembershipState::Invite, MembershipState::Join], false).await {
            return Err(StoreError::Transport(format!("local view never showed {principal} as invited")));
        }
        Ok(())
    }

    async fn revoke(&self, doc: &DocId, principal: &str) -> Result<()> {
        let uid = UserId::parse(principal).map_err(|e| StoreError::Invalid(format!("principal: {e}")))?;
        let room = self.room_for(doc).await?;
        let snap = self.snapshot_of(&room).await?;
        self.require_owner(&snap, "revoke")?;
        if snap.power.level_of(principal) >= LEVEL_OWNER {
            return Err(StoreError::Forbidden("an owner cannot be removed by this call".into()));
        }
        self.retry_rl(|| room.kick_user(&uid, Some("dcp: access revoked")))
            .await
            .map_err(|e| to_store(e, "kick"))?;
        if snap.power.users.contains_key(principal) {
            let mut content = snap.power_levels_content.clone();
            content["users"].as_object_mut().map(|o| o.remove(principal));
            let q = self.queue_for(&room).await;
            q.send(Job::State { event_type: "m.room.power_levels".into(), state_key: String::new(), content }).await?;
        }
        // The group session is rotated only if this device knows the member is
        // gone when it next encrypts. Wait for our own view to say so.
        if !self.wait_member(&room, &uid, &[MembershipState::Leave, MembershipState::Ban], true).await {
            return Err(StoreError::Transport(format!("local view still lists {principal} as a member")));
        }
        Ok(())
    }

    async fn versions(&self, doc: &DocId) -> Result<Vec<Stamped<Version>>> {
        Ok(self.read_report(doc).await?.versions)
    }

    async fn fetch_file(&self, doc: &DocId, version: &Stamped<Version>) -> Result<Vec<u8>> {
        // Matrix media is not access-controlled per room: anyone holding the mxc
        // URI and the key can download. The real protection is that a removed
        // device never receives the keys to the event that carries them. This
        // check is the binding refusing on a principal's behalf, nothing more.
        self.room_for(doc).await?;
        let source = match &version.body.file.locator {
            Locator::MatrixEncryptedMedia { file } => {
                let f: EncryptedFile = serde_json::from_value(file.clone())
                    .map_err(|e| StoreError::Invalid(format!("encrypted file object: {e}")))?;
                MediaSource::Encrypted(Box::new(f))
            }
            Locator::MatrixMedia { url } => MediaSource::Plain(OwnedMxcUri::from(url.as_str())),
            other => return Err(StoreError::Invalid(format!("locator {other:?} is not a Matrix locator"))),
        };
        self.inner
            .client
            .media()
            .get_media_content(&MediaRequestParameters { source, format: MediaFormat::File }, false)
            .await
            .map_err(|e| to_store(e, "download file"))
    }

    async fn publish_version(
        &self,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
    ) -> Result<Stamped<Version>> {
        let room = self.room_for(doc).await?;
        let snap = self.snapshot_of(&room).await?;
        self.require_editor(&snap, "publish version")?;
        self.ensure_members_current(&room, &snap).await?;
        self.publish_version_in(&room, parents, file, message).await
    }

    async fn lease(&self, doc: &DocId) -> Result<Option<Stamped<Lease>>> {
        Ok(self.snapshot(doc).await?.lease)
    }

    async fn write_lease(&self, doc: &DocId, lease: Lease) -> Result<Stamped<Lease>> {
        let room = self.room_for(doc).await?;
        let snap = self.snapshot_of(&room).await?;
        self.require_editor(&snap, "write lease")?;
        // Writes exactly what it is given. Synapse does not persist a state
        // event whose content equals the sender's current one for that key (it
        // answers with the old event ID and timestamp); spec 4.3 rule 8 makes
        // that impossible by requiring the caller's `renewal` to differ from the
        // record it replaces, so nothing is rewritten here.
        let content = serde_json::to_value(&lease).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let q = self.queue_for(&room).await;
        let id = q
            .send(Job::State { event_type: state::LEASE.into(), state_key: String::new(), content })
            .await?;
        let (ts, _) = self.event_stamp(&room, &id).await?;
        Ok(Stamped::new(id, ts, self.inner.user_id.clone(), lease))
    }

    async fn now_ms(&self, doc: &DocId) -> Result<u64> {
        match self.inner.cfg.clock {
            ClockSource::Calibrated => {
                let sample = self.inner.clock_sample.lock().ok().and_then(|c| *c);
                match sample {
                    Some((ts, at)) => Ok(ts + at.elapsed().as_millis() as u64),
                    None => synapse::server_date_ms(&self.inner.http, &self.inner.cfg.homeserver).await,
                }
            }
            ClockSource::ServerDate => synapse::server_date_ms(&self.inner.http, &self.inner.cfg.homeserver).await,
            ClockSource::ProbeEvent => {
                let room = self.room_for(doc).await?;
                let snap = self.snapshot_of(&room).await?;
                self.require_editor(&snap, "clock probe")?;
                let (_, ts) = self.publish_timeline(&room, state::CLOCK, &json!({"dcp": 1})).await?;
                Ok(ts)
            }
        }
    }

    async fn publish_update(&self, doc: &DocId, update: Update) -> Result<Stamped<Update>> {
        if let Payload::Base64 { data, size } = &update.payload {
            let decoded = base64_len(data)?;
            if decoded as u64 != *size {
                return Err(StoreError::Invalid(format!("payload.size {size} but the data decodes to {decoded} bytes")));
            }
            if decoded > self.inner.cfg.max_inline_payload {
                return Err(StoreError::Invalid(format!(
                    "inline payload of {decoded} bytes exceeds the {} byte limit; use encoding \"file\"",
                    self.inner.cfg.max_inline_payload
                )));
            }
        }
        let room = self.room_for(doc).await?;
        let snap = self.snapshot_of(&room).await?;
        self.require_editor(&snap, "publish update")?;
        self.ensure_members_current(&room, &snap).await?;
        let (id, ts) = self.publish_timeline(&room, state::UPDATE, &update).await?;
        Ok(Stamped::new(id, ts, self.inner.user_id.clone(), update).with_device(self.inner.device_id.clone()))
    }

    async fn updates(&self, doc: &DocId) -> Result<Vec<Stamped<Update>>> {
        Ok(self.read_report(doc).await?.updates)
    }
}

fn base64_len(data: &str) -> Result<usize> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map(|b| b.len())
        .map_err(|e| StoreError::Invalid(format!("payload.data is not base64: {e}")))
}

/// A typed `Raw` straight from JSON (there is no `Value`-to-event cast).
fn raw_json<T>(v: Value) -> Result<Raw<T>> {
    Ok(Raw::from_json(serde_json::value::to_raw_value(&v).map_err(|e| StoreError::Invalid(e.to_string()))?))
}
