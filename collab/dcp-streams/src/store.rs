//! [`StreamsStore`]: a [`DocumentStore`] over a Durable Streams server that
//! stores bytes and nothing else. See `docs/rfc/0004-spike-b-results.md` for
//! the layout and why.
//!
//! One document is these streams (`<base>` is the server's stream root):
//!
//! ```text
//! <base>/dcp/<doc>/ctl                    control: one signed EpochRecord per epoch
//! <base>/dcp/<doc>/<prefix>/log           meta and versions of epoch n   (JSON mode)
//! <base>/dcp/<doc>/<prefix>/upd           updates of epoch n             (JSON mode)
//! <base>/dcp/<doc>/<prefix>/lease         lease records of epoch n       (JSON mode, Stream-Seq)
//! <base>/dcp/<doc>/f/<id>                 one encrypted file, created closed (octet stream)
//! ```
//!
//! An epoch is a roster and a key. Every change of roster (a grant, a role
//! change, a removal) is a new epoch with a fresh key; the old epoch's streams
//! are sealed (closed with an owner-signed last message).

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use dcp_core::schema::{self, Kind as SchemaKind};
use dcp_core::{
    graph, integrity, lease as leasemod, Covers, DocId, DocumentStore, DroppedRecord, FileRef,
    Lease, LeaseState, Locator, Meta, NewFile, Result, Role, Stamped, StoreError, Update, Version,
};
use serde::Serialize;
use serde_json::Value;

use crate::crypto::{self, b64, Identity};
use crate::doc::{prev_key_aad, DocCache, Dropped, LeaseInner, LeaseRec};
use crate::http::{Http, HttpError, JSON, OCTETS, START};
use crate::wire::{
    blob_aad, EpochRecord, Inner, Kind, Member, Record, Sealed, UpdHint, BLOB_HEADER, BLOB_MAGIC,
    WIRE,
};

#[derive(Clone, Debug)]
pub struct StreamsConfig {
    pub device: String,
    /// Skip the updates a covering version says it includes (client snapshot).
    pub use_snapshots: bool,
    /// The largest file this client will upload as one blob.
    pub max_file_bytes: usize,
    /// NEGATIVE CONTROL, never set outside the tests that prove the suite can
    /// fail: accept a record from any roster member whatever its role.
    pub lenient_roles: bool,
    /// NEGATIVE CONTROL: rotate the roster but keep the old epoch key, which
    /// is what "revocation without rotation" would be.
    pub reuse_epoch_key: bool,
    /// CRASH SIMULATION: a rotation that stops after its epoch record is in
    /// the control stream and before it seals the old epoch's streams.
    pub crash_before_sealing: bool,
}

impl StreamsConfig {
    pub fn new(device: &str) -> Self {
        StreamsConfig {
            device: device.to_owned(),
            use_snapshots: true,
            max_file_bytes: 256 * 1024 * 1024,
            lenient_roles: false,
            reuse_epoch_key: false,
            crash_before_sealing: false,
        }
    }
}

pub struct StreamsStore {
    http: Http,
    me: Identity,
    cfg: StreamsConfig,
    docs: tokio::sync::Mutex<HashMap<String, DocCache>>,
    truncate_next: AtomicBool,
}

fn conv(e: HttpError) -> StoreError {
    match e {
        HttpError::NotFound => StoreError::NotFound("stream".into()),
        HttpError::Conflict { closed } => StoreError::Conflict(format!(
            "the server refused the append (409{})",
            if closed { ", stream closed" } else { "" }
        )),
        HttpError::Auth(s) => {
            StoreError::Forbidden(format!("the server's access control said {s}"))
        }
        HttpError::TooLarge => StoreError::Invalid("the server refused the size (413)".into()),
        other => StoreError::Transport(other.to_string()),
    }
}

fn is_sealed(e: &HttpError) -> bool {
    matches!(e, HttpError::Conflict { closed: true })
}

/// What a published version says about the update log.
enum Covering<'a> {
    /// Nothing: an ordinary version.
    No,
    /// Every update now in the latest epoch's `upd` stream, with a resume hint
    /// (`publish_covering_version`, the client snapshot).
    WholeLog(&'a str),
    /// The caller's claim, as given (`publish_version_through`), no hint.
    Through(Covers),
}

fn new_prefix(n: u64) -> String {
    format!("e{n}-{}", hex::encode(crypto::random::<6>()))
}

fn seq_header(seq: u64) -> String {
    format!("{seq:016x}")
}

impl StreamsStore {
    pub fn new(http: Http, me: Identity, cfg: StreamsConfig) -> Self {
        StreamsStore {
            http,
            me,
            cfg,
            docs: tokio::sync::Mutex::new(HashMap::new()),
            truncate_next: AtomicBool::new(false),
        }
    }

    fn cache_of<'a>(&self, docs: &'a mut HashMap<String, DocCache>, doc: &str) -> &'a mut DocCache {
        docs.entry(doc.to_owned()).or_insert_with(|| {
            let mut c = DocCache::new(doc);
            c.lenient_roles = self.cfg.lenient_roles;
            c
        })
    }

    pub fn http(&self) -> &Http {
        &self.http
    }

    pub fn identity(&self) -> &Identity {
        &self.me
    }

    /// Fault injection for spec test 3: the next file upload is cut in half
    /// on its way to the server.
    pub fn truncate_next_upload(&self) {
        self.truncate_next.store(true, Ordering::SeqCst);
    }

    /// Pin this client's model of the server clock (see `clock.rs`).
    pub async fn calibrate(&self) -> Result<()> {
        self.http.calibrate().await.map_err(conv)
    }

    async fn now(&self) -> Result<u64> {
        self.http.calibrate().await.map_err(conv)?;
        self.http
            .clock
            .estimate()
            .ok_or_else(|| StoreError::Transport("the server has sent no Date header".into()))
    }

    // ------------------------------------------------------------ reading

    async fn refresh_ctl(&self, c: &mut DocCache) -> Result<()> {
        let path = DocCache::ctl_path(&c.doc);
        let (items, next, _) =
            self.http
                .read_all(&path, &c.ctl_offset)
                .await
                .map_err(|e| match e {
                    HttpError::NotFound => StoreError::NotFound(format!("document {}", c.doc)),
                    e => conv(e),
                })?;
        for v in items {
            c.ingest_epoch(&self.me, v);
        }
        c.ctl_offset = next;
        Ok(())
    }

    async fn refresh_log(&self, c: &mut DocCache, idx: usize) -> Result<()> {
        if c.epochs[idx].key.is_none() || c.epochs[idx].log.closed {
            return Ok(());
        }
        let path = c.epochs[idx].path(&c.doc, "log");
        let at = c.epochs[idx].log.offset.clone();
        let now = self.now().await?;
        match self.http.read_all(&path, &at).await {
            Ok((items, next, closed)) => {
                for v in items {
                    c.ingest_log(idx, v, now);
                }
                c.epochs[idx].log.offset = next;
                c.epochs[idx].log.closed = closed;
                Ok(())
            }
            // The epoch's streams are made before its record is appended, so
            // this is a server that lost them, not a rotation in progress.
            Err(HttpError::NotFound) => Ok(()),
            Err(e) => Err(conv(e)),
        }
    }

    /// The offset a reader that has read nothing of epoch `idx`'s `upd` stream
    /// may start at: the one a covering version recorded, if a client wrote one.
    fn snapshot_hint(c: &DocCache, idx: usize) -> Option<String> {
        let n = c.epochs[idx].n();
        c.epochs
            .iter()
            .flat_map(|e| {
                e.versions.iter().filter_map(move |v| {
                    let h = e.hints.get(&v.id)?;
                    (h.epoch == n && v.body.covers.is_some())
                        .then(|| (v.ts_ms, v.id.clone(), h.offset.clone()))
                })
            })
            .max_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)))
            .map(|x| x.2)
    }

    async fn refresh_upd(&self, c: &mut DocCache, idx: usize) -> Result<()> {
        if c.epochs[idx].key.is_none() || c.epochs[idx].upd.closed {
            return Ok(());
        }
        let path = c.epochs[idx].path(&c.doc, "upd");
        let mut at = c.epochs[idx].upd.offset.clone();
        let now = self.now().await?;
        let mut hinted = false;
        if at == START && self.cfg.use_snapshots {
            if let Some(h) = Self::snapshot_hint(c, idx) {
                // Offsets sort lexicographically (protocol section 8), so a hint
                // beyond the tail is a lie or a lost stream: read from the start.
                // A hint that is inside the stream cannot be checked; it is
                // trusted as far as the editor who wrote it.
                match self.http.head(&path).await {
                    Ok(head) if h <= head.next => {
                        at = h;
                        hinted = true;
                    }
                    _ => {}
                }
            }
        }
        let got = match self.http.read_all(&path, &at).await {
            // A hint the server does not accept is ignored: read it all.
            Err(HttpError::Status(400, _)) | Err(HttpError::Gone) if hinted => {
                self.http.read_all(&path, START).await
            }
            other => other,
        };
        match got {
            Ok((items, next, closed)) => {
                for v in items {
                    c.ingest_upd(idx, v, now);
                }
                c.epochs[idx].upd.offset = next;
                c.epochs[idx].upd.closed = closed;
                Ok(())
            }
            Err(HttpError::NotFound) => Ok(()),
            Err(e) => Err(conv(e)),
        }
    }

    async fn refresh_lease(&self, c: &mut DocCache, idx: usize) -> Result<()> {
        if c.epochs[idx].key.is_none() || c.epochs[idx].lease.closed {
            return Ok(());
        }
        let path = c.epochs[idx].path(&c.doc, "lease");
        let at = c.epochs[idx].lease.offset.clone();
        let now = self.now().await?;
        match self.http.read_all(&path, &at).await {
            Ok((items, next, closed)) => {
                for v in items {
                    c.ingest_lease(idx, v, now);
                }
                c.epochs[idx].lease.offset = next;
                c.epochs[idx].lease.closed = closed;
                Ok(())
            }
            Err(HttpError::NotFound) => Ok(()),
            Err(e) => Err(conv(e)),
        }
    }

    async fn refresh_all_logs(&self, c: &mut DocCache) -> Result<()> {
        for i in 0..c.epochs.len() {
            self.refresh_log(c, i).await?;
        }
        Ok(())
    }

    /// Sync the control stream and return the index of the latest epoch,
    /// requiring this principal to be in its roster with at least `need`.
    async fn current(&self, c: &mut DocCache, need: Role, what: &str) -> Result<usize> {
        self.refresh_ctl(c).await?;
        let Some(last) = c.epochs.len().checked_sub(1) else {
            return Err(StoreError::NotFound(format!(
                "document {} has no valid epoch",
                c.doc
            )));
        };
        let me = self.me.principal();
        match c.epochs[last].role_of(&me) {
            None => Err(StoreError::Forbidden(format!(
                "{me} is not a member of {} (epoch {last}): cannot {what}",
                c.doc
            ))),
            Some(r) if r < need => Err(StoreError::Forbidden(format!(
                "{what} needs the {need:?} role, {me} is {r:?}"
            ))),
            Some(_) if c.epochs[last].key.is_none() => Err(StoreError::Forbidden(format!(
                "{me} is in the roster of epoch {last} but holds no key for it"
            ))),
            Some(_) => Ok(last),
        }
    }

    // ------------------------------------------------------------ writing

    fn make_record<T: Serialize>(&self, c: &DocCache, idx: usize, kind: Kind, inner: &T) -> Record {
        let es = &c.epochs[idx];
        let key = es.key.as_ref().expect("a writer holds the epoch key");
        let me = self.me.principal();
        let plain = serde_json::to_vec(inner).expect("inner serialises");
        let (nonce, ct) = crypto::seal(key, &Record::aad(&c.doc, kind, es.n(), &me), &plain);
        let mut rec = Record {
            v: WIRE,
            k: kind,
            e: es.n(),
            a: me,
            n: b64(&nonce),
            c: b64(&ct),
            s: String::new(),
            x: None,
        };
        rec.s = b64(&self.me.sign(&rec.signed_bytes(&c.doc)));
        rec
    }

    fn make_seal(&self, doc: &str, epoch: u64) -> Record {
        let mut rec = Record {
            v: WIRE,
            k: Kind::Seal,
            e: epoch,
            a: self.me.principal(),
            n: String::new(),
            c: String::new(),
            s: String::new(),
            x: Some(epoch + 1),
        };
        rec.s = b64(&self.me.sign(&rec.signed_bytes(doc)));
        rec
    }

    fn json(rec: &impl Serialize) -> Vec<u8> {
        serde_json::to_vec(rec).expect("a record serialises")
    }

    /// `POST` one record to `stream` of epoch `idx`.
    async fn post(
        &self,
        c: &DocCache,
        idx: usize,
        stream: &str,
        rec: &Record,
        seq: Option<u64>,
    ) -> std::result::Result<(), HttpError> {
        let path = c.epochs[idx].path(&c.doc, stream);
        let s = seq.map(seq_header);
        self.http
            .append(&path, JSON, Self::json(rec), s.as_deref(), false)
            .await
            .map(|_| ())
    }

    /// Upload one file as an encrypted, closed stream and read it back.
    async fn upload_file(&self, c: &DocCache, idx: usize, bytes: &[u8]) -> Result<String> {
        if bytes.len() > self.cfg.max_file_bytes {
            return Err(StoreError::Invalid(format!(
                "a file of {} bytes exceeds this client's limit of {}",
                bytes.len(),
                self.cfg.max_file_bytes
            )));
        }
        let es = &c.epochs[idx];
        let key = es.key.as_ref().expect("a writer holds the epoch key");
        let file_id = b64(&crypto::random::<12>());
        let (nonce, ct) = crypto::seal(key, &blob_aad(&c.doc, &file_id, es.n()), bytes);
        let mut blob = Vec::with_capacity(BLOB_HEADER + nonce.len() + ct.len());
        blob.extend_from_slice(BLOB_MAGIC);
        blob.extend_from_slice(&es.n().to_be_bytes());
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ct);
        if self.truncate_next.swap(false, Ordering::SeqCst) {
            blob.truncate(blob.len() / 2);
        }
        let path = format!("dcp/{}/f/{file_id}", c.doc);
        self.http
            .create(&path, OCTETS, Some(blob), true)
            .await
            .map_err(conv)?;
        // Version rule 2: read back what the server holds and check it.
        let stored = self.http.read_bytes(&path).await.map_err(conv)?;
        let verdict = Self::open_blob(c, &file_id, &stored)
            .map(|plain| plain == bytes)
            .unwrap_or(false);
        if !verdict {
            let _ = self.http.delete(&path).await;
            return Err(StoreError::Transport(
                "upload failed verification (the stored bytes do not decrypt to the file), not published"
                    .into(),
            ));
        }
        Ok(file_id)
    }

    fn open_blob(c: &DocCache, file_id: &str, blob: &[u8]) -> Result<Vec<u8>> {
        if blob.len() < BLOB_HEADER + crypto::NONCE_LEN + crypto::TAG_LEN
            || &blob[..5] != BLOB_MAGIC
        {
            return Err(StoreError::Invalid("not a DCP file blob".into()));
        }
        let epoch = u64::from_be_bytes(blob[5..13].try_into().expect("8 bytes"));
        let Some(es) = c.epochs.get(epoch as usize) else {
            return Err(StoreError::Invalid(format!(
                "file of unknown epoch {epoch}"
            )));
        };
        let Some(key) = es.key.as_ref() else {
            return Err(StoreError::Forbidden(format!(
                "no key for epoch {epoch}: this principal was not a member then or since"
            )));
        };
        let nonce = &blob[BLOB_HEADER..BLOB_HEADER + crypto::NONCE_LEN];
        let ct = &blob[BLOB_HEADER + crypto::NONCE_LEN..];
        crypto::open(key, nonce, &blob_aad(&c.doc, file_id, epoch), ct)
            .map_err(|e| StoreError::Invalid(format!("file does not decrypt: {e}")))
    }

    fn file_id_of(doc: &str, locator: &Locator) -> Result<String> {
        // `url` is not accepted: its bytes would have to hash to `sha256`, and these are ciphertext
        // (spec 4.2.1, draft 1 update 2).
        let Locator::DurableStream { href } = locator else {
            return Err(StoreError::Invalid(
                "this binding only reads `durable-stream` locators that name a stream of the document"
                    .into(),
            ));
        };
        // Scheme, host and base path are ignored on purpose: a version's
        // locator must never send this client anywhere but its own server.
        let tail = href
            .rsplit_once(&format!("/dcp/{doc}/f/"))
            .map(|(_, t)| t)
            .filter(|t| {
                !t.is_empty()
                    && t.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            })
            .ok_or_else(|| {
                StoreError::Invalid(format!("locator {href} is not a file stream of {doc}"))
            })?;
        Ok(tail.to_owned())
    }

    fn href(&self, doc: &str, file_id: &str) -> String {
        let base = self.http.base();
        let rest = base
            .strip_prefix("http://")
            .or_else(|| base.strip_prefix("https://"))
            .unwrap_or(base);
        format!("https://{rest}dcp/{doc}/f/{file_id}")
    }

    // ------------------------------------------------------------ rotation

    /// Move the document to a new epoch with `roster`. Owner only.
    async fn rotate(&self, c: &mut DocCache, roster: BTreeMap<String, Role>) -> Result<()> {
        let latest = self.current(c, Role::Owner, "change the roster").await?;
        self.refresh_log(c, latest).await?;
        self.complete_seals(c).await;
        let meta = c.epochs[latest]
            .meta
            .clone()
            .ok_or_else(|| StoreError::Invalid("the current epoch has no metadata".into()))?;
        let old = &c.epochs[latest];
        let n1 = old.n() + 1;
        let prefix = new_prefix(n1);
        let key = if self.cfg.reuse_epoch_key {
            old.key.clone().expect("an owner holds the key")
        } else {
            crypto::new_epoch_key()
        };
        let mut wraps = Vec::new();
        for p in roster.keys() {
            wraps.push(
                crypto::wrap(p, &c.doc, n1, &key)
                    .map_err(|e| StoreError::Invalid(format!("cannot wrap for {p}: {e}")))?,
            );
        }
        let old_key = old.key.as_ref().expect("an owner holds the key");
        let (pn, pc) = crypto::seal(&key, &prev_key_aad(&c.doc, n1), &**old_key);
        let mut rec = EpochRecord {
            v: WIRE,
            doc: c.doc.clone(),
            n: n1,
            prefix: prefix.clone(),
            prev: Some(old.id.clone()),
            root: None,
            roster: roster
                .iter()
                .map(|(p, r)| Member {
                    p: p.clone(),
                    r: *r,
                })
                .collect(),
            wraps,
            prev_key: Some(Sealed {
                n: b64(&pn),
                c: b64(&pc),
            }),
            a: self.me.principal(),
            s: String::new(),
        };
        rec.s = b64(&self.me.sign(&rec.signed_bytes()));
        let new_id = rec.id();

        // 1. the new epoch's streams, with its metadata, before anyone can see the epoch.
        let st = self.now().await?;
        let meta_rec = {
            // The record is encrypted under the NEW key, so build it by hand.
            let inner = Inner {
                st,
                dev: None,
                body: meta,
                hint: None,
            };
            let plain = serde_json::to_vec(&inner).expect("meta serialises");
            let me = self.me.principal();
            let (nonce, ct) = crypto::seal(&key, &Record::aad(&c.doc, Kind::Meta, n1, &me), &plain);
            let mut r = Record {
                v: WIRE,
                k: Kind::Meta,
                e: n1,
                a: me,
                n: b64(&nonce),
                c: b64(&ct),
                s: String::new(),
                x: None,
            };
            r.s = b64(&self.me.sign(&r.signed_bytes(&c.doc)));
            r
        };
        let base = format!("dcp/{}/{prefix}", c.doc);
        self.http
            .create(
                &format!("{base}/log"),
                JSON,
                Some(Self::json(&vec![meta_rec])),
                false,
            )
            .await
            .map_err(conv)?;
        for s in ["upd", "lease"] {
            self.http
                .create(&format!("{base}/{s}"), JSON, None, false)
                .await
                .map_err(conv)?;
        }

        // 2. the epoch record: from here the document is in the new epoch.
        self.http
            .append(
                &DocCache::ctl_path(&c.doc),
                JSON,
                Self::json(&rec),
                None,
                false,
            )
            .await
            .map_err(conv)?;

        // 3. who won? (two owners can rotate at once; the first valid record
        //    in stream order counts and the other finds out here)
        self.refresh_ctl(c).await?;
        if c.epochs.get(n1 as usize).map(|e| e.id.as_str()) != Some(new_id.as_str()) {
            for s in ["log", "upd", "lease"] {
                let _ = self.http.delete(&format!("{base}/{s}")).await;
            }
            return Err(StoreError::Conflict(
                "another owner changed the roster first; read the document again and retry".into(),
            ));
        }

        // 4. seal the old epoch: nothing appended after this counts, and an
        //    honest server refuses it.
        if self.cfg.crash_before_sealing {
            return Ok(());
        }
        self.complete_seals(c).await;
        Ok(())
    }

    /// Seal every stream of every finished epoch that is not sealed yet, as an
    /// owner of that epoch. Idempotent; errors are not fatal (a stream that
    /// is already closed answers 409).
    async fn complete_seals(&self, c: &mut DocCache) {
        let me = self.me.principal();
        let last = c.epochs.len().saturating_sub(1);
        for i in 0..last {
            if c.epochs[i].role_of(&me) != Some(Role::Owner) {
                continue;
            }
            for s in ["log", "upd", "lease"] {
                let pos = match s {
                    "log" => &c.epochs[i].log,
                    "upd" => &c.epochs[i].upd,
                    _ => &c.epochs[i].lease,
                };
                if pos.sealed {
                    continue;
                }
                let path = c.epochs[i].path(&c.doc, s);
                let seal = self.make_seal(&c.doc, c.epochs[i].n());
                let _ = self
                    .http
                    .append(&path, JSON, Self::json(&seal), None, true)
                    .await;
            }
        }
    }

    /// Finish a rotation that was interrupted before every old stream was sealed.
    pub async fn heal(&self, doc: &DocId) -> Result<()> {
        let mut docs = self.docs.lock().await;
        let c = self.cache_of(&mut docs, doc);
        self.refresh_ctl(c).await?;
        self.complete_seals(c).await;
        Ok(())
    }

    // ------------------------------------------------------------ test and measurement hooks

    /// The records of this document that this client dropped, and why.
    pub async fn dropped(&self, doc: &DocId) -> Vec<Dropped> {
        self.docs
            .lock()
            .await
            .get(doc)
            .map(|c| c.dropped.clone())
            .unwrap_or_default()
    }

    /// How many epochs this client has seen, and which it holds a key for.
    pub async fn epochs(&self, doc: &DocId) -> Vec<(u64, bool, String)> {
        self.docs
            .lock()
            .await
            .get(doc)
            .map(|c| {
                c.epochs
                    .iter()
                    .map(|e| (e.n(), e.key.is_some(), e.rec.prefix.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The raw epoch keys this client holds (a hook for the revocation test:
    /// "a removed member who kept every key it ever had").
    pub async fn held_keys(&self, doc: &DocId) -> Vec<(u64, [u8; 32])> {
        self.docs
            .lock()
            .await
            .get(doc)
            .map(|c| {
                c.epochs
                    .iter()
                    .filter_map(|e| e.key.as_ref().map(|k| (e.n(), **k)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The stream path of `which` (`log`, `upd`, `lease`) in epoch `n`.
    pub async fn stream_path(&self, doc: &DocId, n: u64, which: &str) -> Option<String> {
        self.docs
            .lock()
            .await
            .get(doc)
            .and_then(|c| c.epochs.get(n as usize).map(|e| e.path(&c.doc, which)))
    }

    pub async fn sync(&self, doc: &DocId) -> Result<()> {
        let mut docs = self.docs.lock().await;
        let c = self.cache_of(&mut docs, doc);
        self.refresh_ctl(c).await?;
        self.refresh_all_logs(c).await
    }

    /// A hostile member's client: encrypt `inner` (the plaintext JSON object
    /// of a [`Inner`]) under the epoch key this principal holds, sign it as
    /// this principal, and send it to the server with no role check. `Ok(None)`
    /// when the server's own access control refused it.
    pub async fn send_unchecked(
        &self,
        doc: &DocId,
        kind: Kind,
        inner: &Value,
    ) -> Result<Option<String>> {
        let mut docs = self.docs.lock().await;
        let c = self.cache_of(&mut docs, doc);
        self.refresh_ctl(c).await?;
        let idx = c
            .epochs
            .iter()
            .rposition(|e| e.key.is_some())
            .ok_or_else(|| StoreError::Forbidden("no epoch key".into()))?;
        let rec = self.make_record(c, idx, kind, inner);
        let stream = match kind {
            Kind::Meta | Kind::Ver => "log",
            Kind::Upd => "upd",
            Kind::Lease => "lease",
            Kind::Seal => return Err(StoreError::Invalid("not a sealed kind".into())),
        };
        match self.post(c, idx, stream, &rec, None).await {
            Ok(()) => Ok(Some(rec.id())),
            Err(HttpError::Auth(_)) => Ok(None),
            Err(e) => Err(conv(e)),
        }
    }

    /// Publish a version that `covers` every update currently in the latest
    /// epoch's `upd` stream (spec 4.4 / 7.4), and record where in that
    /// stream a reader that starts later may begin. This is the client
    /// snapshot: Durable Streams has no server-side one.
    pub async fn publish_covering_version(
        &self,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
        algorithm: &str,
    ) -> Result<Stamped<Version>> {
        self.publish_inner(doc, parents, file, message, Covering::WholeLog(algorithm))
            .await
    }

    async fn publish_inner(
        &self,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
        covering: Covering<'_>,
    ) -> Result<Stamped<Version>> {
        let mut docs = self.docs.lock().await;
        let c = self.cache_of(&mut docs, doc);
        let idx = self.current(c, Role::Editor, "publish a version").await?;
        self.refresh_all_logs(c).await?;
        let existing: Vec<Stamped<Version>> = c
            .epochs
            .iter()
            .flat_map(|e| e.versions.iter().cloned())
            .collect();
        graph::check_new_parents(&existing, &parents)
            .map_err(|e| StoreError::Invalid(format!("parents: {e}")))?;

        let file_id = self.upload_file(c, idx, &file.bytes).await?;
        let file_ref = integrity::file_ref(
            &file.mime,
            &file.bytes,
            Locator::DurableStream {
                href: self.href(doc, &file_id),
            },
        );
        let mut version = Version {
            dcp: 1,
            parents,
            file: file_ref,
            created_by: dcp_core::Author {
                principal: self.me.principal(),
                device: self.cfg.device.clone(),
            },
            message,
            covers: None,
        };

        let mut last_err = StoreError::Conflict("the epoch kept changing".into());
        for _ in 0..4 {
            let idx = self.current(c, Role::Editor, "publish a version").await?;
            let mut hint = None;
            match &covering {
                Covering::No => {}
                Covering::WholeLog(alg) => {
                    self.refresh_upd(c, idx).await?;
                    let es = &c.epochs[idx];
                    let through =
                        es.updates.last().map(|u| u.id.clone()).ok_or_else(|| {
                            StoreError::Invalid("there are no updates to cover".into())
                        })?;
                    version.covers = Some(Covers {
                        algorithm: (*alg).to_owned(),
                        through,
                    });
                    hint = Some(UpdHint {
                        epoch: es.n(),
                        offset: es.upd.offset.clone(),
                    });
                }
                // The caller's claim, about a prefix of the log and not the whole
                // of it: no resume hint, since a reader that skipped to the end of
                // the stream would miss the updates after the prefix.
                Covering::Through(covers) => version.covers = Some(covers.clone()),
            }
            schema::validate_typed(SchemaKind::Version, &version)
                .map_err(|e| StoreError::Invalid(e.to_string()))?;
            let st = self.now().await?;
            let rec = self.make_record(
                c,
                idx,
                Kind::Ver,
                &Inner {
                    st,
                    dev: None,
                    body: &version,
                    hint,
                },
            );
            match self.post(c, idx, "log", &rec, None).await {
                Ok(()) => {
                    let mut s = Stamped::new(rec.id(), st, self.me.principal(), version);
                    s.device = Some(self.cfg.device.clone());
                    return Ok(s);
                }
                Err(e) if is_sealed(&e) => {
                    last_err = conv(e);
                    continue; // a rotation sealed the epoch: re-read and write into the next one
                }
                Err(e) => return Err(conv(e)),
            }
        }
        Err(last_err)
    }
}

#[async_trait]
impl DocumentStore for StreamsStore {
    fn principal(&self) -> String {
        self.me.principal()
    }

    fn device(&self) -> String {
        self.cfg.device.clone()
    }

    async fn create_document(&self, meta: &Meta, first: Option<NewFile>) -> Result<DocId> {
        schema::validate_typed(SchemaKind::Meta, meta)
            .map_err(|e| StoreError::Invalid(e.to_string()))?;
        let (doc, rec, _key) = crate::doc::genesis(&self.me);
        let prefix = rec.prefix.clone();
        let mut cache = DocCache::new(&doc);
        cache.lenient_roles = self.cfg.lenient_roles;
        cache.ingest_epoch(
            &self.me,
            serde_json::to_value(&rec).expect("epoch serialises"),
        );
        let st = self.now().await?;
        let meta_rec = self.make_record(
            &cache,
            0,
            Kind::Meta,
            &Inner {
                st,
                dev: None,
                body: meta,
                hint: None,
            },
        );
        let base = format!("dcp/{doc}/{prefix}");
        self.http
            .create(
                &format!("{base}/log"),
                JSON,
                Some(Self::json(&vec![meta_rec])),
                false,
            )
            .await
            .map_err(conv)?;
        for s in ["upd", "lease"] {
            self.http
                .create(&format!("{base}/{s}"), JSON, None, false)
                .await
                .map_err(conv)?;
        }
        // Last: the document exists for everyone else once its control stream does.
        self.http
            .create(
                &DocCache::ctl_path(&doc),
                JSON,
                Some(Self::json(&vec![rec])),
                false,
            )
            .await
            .map_err(conv)?;
        self.docs.lock().await.insert(doc.clone(), cache);
        if let Some(f) = first {
            self.publish_inner(&doc, Vec::new(), f, None, Covering::No)
                .await?;
        }
        Ok(doc)
    }

    async fn meta(&self, doc: &DocId) -> Result<Meta> {
        let mut docs = self.docs.lock().await;
        let c = self.cache_of(&mut docs, doc);
        self.refresh_ctl(c).await?;
        self.refresh_all_logs(c).await?;
        c.epochs
            .iter()
            .rev()
            .find_map(|e| e.meta.clone())
            .ok_or_else(|| {
                StoreError::Forbidden(format!("{} cannot read {doc}", self.me.principal()))
            })
    }

    async fn role(&self, doc: &DocId) -> Result<Role> {
        let mut docs = self.docs.lock().await;
        let c = self.cache_of(&mut docs, doc);
        self.refresh_ctl(c).await?;
        let me = self.me.principal();
        c.epochs
            .last()
            .and_then(|e| e.role_of(&me))
            .ok_or_else(|| StoreError::Forbidden(format!("{me} is not a member of {doc}")))
    }

    async fn grant(&self, doc: &DocId, principal: &str, role: Role) -> Result<()> {
        crypto::parse_principal(principal)
            .map_err(|e| StoreError::Invalid(format!("cannot grant to {principal}: {e}")))?;
        let mut docs = self.docs.lock().await;
        let c = self.cache_of(&mut docs, doc);
        let latest = self.current(c, Role::Owner, "change roles").await?;
        let mut roster = c.epochs[latest].roster.clone();
        if roster.get(principal) == Some(&role) {
            return Ok(());
        }
        roster.insert(principal.to_owned(), role);
        self.rotate(c, roster).await
    }

    async fn revoke(&self, doc: &DocId, principal: &str) -> Result<()> {
        let mut docs = self.docs.lock().await;
        let c = self.cache_of(&mut docs, doc);
        let latest = self.current(c, Role::Owner, "remove a member").await?;
        if principal == self.me.principal() {
            return Err(StoreError::Invalid(
                "an owner cannot remove themselves".into(),
            ));
        }
        let mut roster = c.epochs[latest].roster.clone();
        if roster.remove(principal).is_none() {
            return Ok(());
        }
        self.rotate(c, roster).await
    }

    async fn versions(&self, doc: &DocId) -> Result<Vec<Stamped<Version>>> {
        let mut docs = self.docs.lock().await;
        let c = self.cache_of(&mut docs, doc);
        self.refresh_ctl(c).await?;
        if !c.epochs.iter().any(|e| e.key.is_some()) {
            return Err(StoreError::Forbidden(format!(
                "{} holds no key for {doc}",
                self.me.principal()
            )));
        }
        self.refresh_all_logs(c).await?;
        Ok(c.epochs
            .iter()
            .flat_map(|e| e.versions.iter().cloned())
            .collect())
    }

    async fn fetch_file(&self, doc: &DocId, version: &Stamped<Version>) -> Result<Vec<u8>> {
        self.fetch_locator(doc, &version.body.file.locator).await
    }

    async fn publish_version(
        &self,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
    ) -> Result<Stamped<Version>> {
        self.publish_inner(doc, parents, file, message, Covering::No)
            .await
    }

    async fn publish_version_through(
        &self,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
        covers: Option<Covers>,
    ) -> Result<Stamped<Version>> {
        let covering = match covers {
            Some(c) => Covering::Through(c),
            None => Covering::No,
        };
        self.publish_inner(doc, parents, file, message, covering)
            .await
    }

    async fn lease(&self, doc: &DocId) -> Result<Option<Stamped<Lease>>> {
        let mut docs = self.docs.lock().await;
        let c = self.cache_of(&mut docs, doc);
        let idx = self.current(c, Role::Viewer, "read the lease").await?;
        self.refresh_lease(c, idx).await?;
        Ok(c.epochs[idx].lease_last.as_ref().map(|l| l.stamped.clone()))
    }

    async fn write_lease(&self, doc: &DocId, record: Lease) -> Result<Stamped<Lease>> {
        schema::validate_typed(SchemaKind::Lease, &record)
            .map_err(|e| StoreError::Invalid(e.to_string()))?;
        let mut docs = self.docs.lock().await;
        let c = self.cache_of(&mut docs, doc);
        let idx = self.current(c, Role::Editor, "write a lease").await?;
        self.refresh_lease(c, idx).await?;
        let role = c.epochs[idx]
            .role_of(&self.me.principal())
            .unwrap_or(Role::Viewer);
        let current: Option<LeaseRec> = c.epochs[idx].lease_last.clone();
        let me = self.me.principal();
        if record.state == LeaseState::Held {
            let holder = record
                .holder
                .as_ref()
                .expect("schema: a held lease has a holder");
            if holder.principal != me {
                return Err(StoreError::Forbidden(format!(
                    "{me} cannot write a lease held by {}",
                    holder.principal
                )));
            }
            // Rule 8, against the record just read: a writer that built its
            // record over an older one is refused without touching the server.
            let expected = leasemod::next_renewal(current.as_ref().map(|l| &l.stamped.body));
            if record.renewal != Some(expected) {
                return Err(StoreError::Conflict(format!(
                    "lease renewal must be {expected} over the current record (rule 8), got {:?}: the record changed since it was read",
                    record.renewal
                )));
            }
        }
        if role != Role::Owner {
            if let Some(cur) = &current {
                let now = self.now().await?;
                if leasemod::is_active(&cur.stamped, now) {
                    let ch = cur
                        .stamped
                        .body
                        .holder
                        .as_ref()
                        .expect("held lease has a holder");
                    let same = match &record.holder {
                        Some(h) => h == ch,
                        None => ch.principal == me,
                    };
                    if !same {
                        return Err(StoreError::Conflict(format!(
                            "lease is held by {}/{}",
                            ch.principal, ch.session
                        )));
                    }
                }
            }
        }
        let seq = current.as_ref().map_or(0, |l| l.seq) + 1;
        let st = self.now().await?;
        let rec = self.make_record(
            c,
            idx,
            Kind::Lease,
            &LeaseInner {
                st,
                seq: Some(seq),
                body: record.clone(),
            },
        );
        let id = rec.id();
        match self.post(c, idx, "lease", &rec, Some(seq)).await {
            Ok(()) => {}
            Err(HttpError::Conflict { closed }) => {
                // Either another session appended this sequence number first
                // (the compare-and-set did its job), or our own earlier
                // attempt reached the server and its reply was lost.
                self.refresh_lease(c, idx).await?;
                let ours = c.epochs[idx]
                    .lease_last
                    .as_ref()
                    .is_some_and(|l| l.stamped.id == id);
                if !ours {
                    return Err(StoreError::Conflict(if closed {
                        "the lease stream was sealed by a rotation".into()
                    } else {
                        format!("another session wrote lease sequence {seq:016x} first")
                    }));
                }
            }
            Err(e) => return Err(conv(e)),
        }
        Ok(Stamped::new(id, st, me, record))
    }

    async fn now_ms(&self, _doc: &DocId) -> Result<u64> {
        self.now().await
    }

    async fn publish_update(&self, doc: &DocId, update: Update) -> Result<Stamped<Update>> {
        schema::validate_typed(SchemaKind::Update, &update)
            .map_err(|e| StoreError::Invalid(e.to_string()))?;
        let mut docs = self.docs.lock().await;
        let c = self.cache_of(&mut docs, doc);
        let mut last_err = StoreError::Conflict("the epoch kept changing".into());
        for _ in 0..4 {
            let idx = self.current(c, Role::Editor, "publish an update").await?;
            let st = self.now().await?;
            let rec = self.make_record(
                c,
                idx,
                Kind::Upd,
                &Inner {
                    st,
                    dev: Some(self.cfg.device.clone()),
                    body: &update,
                    hint: None,
                },
            );
            match self.post(c, idx, "upd", &rec, None).await {
                Ok(()) => {
                    let mut s = Stamped::new(rec.id(), st, self.me.principal(), update);
                    s.device = Some(self.cfg.device.clone());
                    return Ok(s);
                }
                Err(e) if is_sealed(&e) => {
                    last_err = conv(e);
                    continue;
                }
                Err(e) => return Err(conv(e)),
            }
        }
        Err(last_err)
    }

    /// Epoch by epoch (oldest first), and within an epoch in the order the server
    /// appended the records to that epoch's `upd` stream (`refresh_upd` reads from
    /// the stored offset and appends; nothing is reordered or removed). A later
    /// call therefore returns this call's list with newer updates added at the end,
    /// which is what lets `collabd` name a prefix of it in a version's `covers`.
    /// One exception: a reader that starts from a covering version's resume hint
    /// (`use_snapshots`) has no updates before the hint at all, in any call.
    async fn updates(&self, doc: &DocId) -> Result<Vec<Stamped<Update>>> {
        let mut docs = self.docs.lock().await;
        let c = self.cache_of(&mut docs, doc);
        self.refresh_ctl(c).await?;
        if !c.epochs.iter().any(|e| e.key.is_some()) {
            return Err(StoreError::Forbidden(format!(
                "{} holds no key for {doc}",
                self.me.principal()
            )));
        }
        self.refresh_all_logs(c).await?;
        for i in 0..c.epochs.len() {
            self.refresh_upd(c, i).await?;
        }
        Ok(c.epochs
            .iter()
            .flat_map(|e| e.updates.iter().cloned())
            .collect())
    }

    /// Long-poll the control stream and the current epoch's `log` and `upd`
    /// streams from the offsets this client has already read, and return when any
    /// of them has something new (or is closed), or after `max`. The cache lock is
    /// not held while waiting, so a publish is never blocked behind it.
    async fn wait_for_change(&self, doc: &DocId, max: Duration) -> Result<()> {
        let watch: Vec<(String, String)> = {
            let mut docs = self.docs.lock().await;
            let c = self.cache_of(&mut docs, doc);
            let mut w = vec![(DocCache::ctl_path(&c.doc), c.ctl_offset.clone())];
            if let Some(e) = c.epochs.last().filter(|e| e.key.is_some()) {
                if !e.log.closed {
                    w.push((e.path(&c.doc, "log"), e.log.offset.clone()));
                }
                if !e.upd.closed {
                    w.push((e.path(&c.doc, "upd"), e.upd.offset.clone()));
                }
            }
            w
        };
        let http = &self.http;
        let news = |i: usize| {
            let at = watch.get(i).cloned();
            async move {
                let Some((path, offset)) = at else {
                    return std::future::pending::<Result<()>>().await;
                };
                loop {
                    match http.long_poll(&path, &offset, true).await {
                        Ok(r) if !r.items.is_empty() || r.closed => return Ok(()),
                        // Nothing yet and the server's own long-poll timeout: ask again.
                        Ok(_) => continue,
                        // A stream the server does not (yet) have: do not spin on it.
                        Err(HttpError::NotFound) => {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            return Ok(());
                        }
                        Err(e) => return Err(conv(e)),
                    }
                }
            }
        };
        tokio::select! {
            r = news(0) => r,
            r = news(1) => r,
            r = news(2) => r,
            _ = tokio::time::sleep(max) => Ok(()),
        }
    }

    async fn put_update_file(&self, doc: &DocId, file: NewFile) -> Result<FileRef> {
        let mut docs = self.docs.lock().await;
        let c = self.cache_of(&mut docs, doc);
        let idx = self
            .current(c, Role::Editor, "publish an update file")
            .await?;
        let file_id = self.upload_file(c, idx, &file.bytes).await?;
        Ok(integrity::file_ref(
            &file.mime,
            &file.bytes,
            Locator::DurableStream {
                href: self.href(doc, &file_id),
            },
        ))
    }

    async fn fetch_update_file(&self, doc: &DocId, file: &FileRef) -> Result<Vec<u8>> {
        self.fetch_locator(doc, &file.locator).await
    }

    async fn dropped_records(&self, doc: &DocId) -> Vec<DroppedRecord> {
        self.docs
            .lock()
            .await
            .get(doc)
            .map(|c| {
                c.dropped
                    .iter()
                    .map(|d| DroppedRecord {
                        stream: d.stream.to_owned(),
                        id: d.id.clone(),
                        reason: format!("{:?}", d.reason),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl StreamsStore {
    /// The plaintext of the file stream `locator` names, unverified.
    async fn fetch_locator(&self, doc: &DocId, locator: &Locator) -> Result<Vec<u8>> {
        let file_id = Self::file_id_of(doc, locator)?;
        let mut docs = self.docs.lock().await;
        let c = self.cache_of(&mut docs, doc);
        self.refresh_ctl(c).await?;
        let blob = self
            .http
            .read_bytes(&format!("dcp/{doc}/f/{file_id}"))
            .await
            .map_err(|e| match e {
                HttpError::NotFound => StoreError::NotFound(format!("file {file_id}")),
                e => conv(e),
            })?;
        Self::open_blob(c, &file_id, &blob)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_stream_is_named_by_a_durable_stream_locator() {
        let l = Locator::DurableStream {
            href: "https://anywhere.invalid/base/dcp/doc1/f/q83JmVnH2xZk_0Tb".into(),
        };
        // Scheme, host and base path are the reader's own, never the locator's.
        assert_eq!(
            StreamsStore::file_id_of("doc1", &l).unwrap(),
            "q83JmVnH2xZk_0Tb"
        );
        let other_doc = Locator::DurableStream {
            href: "https://anywhere.invalid/dcp/doc2/f/x".into(),
        };
        assert!(StreamsStore::file_id_of("doc1", &other_doc).is_err());
    }

    /// The bytes behind this locator are ciphertext: a `url` reader would fetch them and fail the
    /// `sha256`, so the binding refuses every other locator type (spec 4.2.1).
    #[test]
    fn a_url_or_blake3_locator_is_not_a_file_stream() {
        for l in [
            Locator::Url {
                href: "https://h.invalid/dcp/doc1/f/x".into(),
            },
            Locator::Blake3 {
                hash: "0".repeat(64),
            },
        ] {
            assert!(StreamsStore::file_id_of("doc1", &l).is_err(), "{l:?}");
        }
    }
}
