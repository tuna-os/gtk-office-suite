//! What a client believes about one document, and the rules that decide it.
//!
//! Nothing here does I/O. The server hands over messages; this module decides
//! which of them count. The server is not trusted for any of it (see the
//! results document, "what the server must be trusted for"): authorship comes
//! from Ed25519 signatures, confidentiality and integrity from the epoch key,
//! and authorisation from the roster of the epoch the message belongs to.

use std::collections::{BTreeMap, HashMap, HashSet};

use dcp_core::schema::{self, Kind as SchemaKind};
use dcp_core::{Lease, Meta, Role, Stamped, Update, Version};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::crypto::{self, EpochKey, Identity};
use crate::wire::{self, EpochRecord, Inner, Kind, Record, UpdHint, WIRE};

/// A `st` further ahead of this reader's own estimate of server time than the
/// skew spec 4.3 allows (30 s) is clamped to that estimate.
pub const SKEW_MS: u64 = 30_000;

pub const START: &str = "-1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropReason {
    /// Not JSON of the right shape (including a message that is not an object).
    Malformed,
    /// A message of a kind that does not belong in this stream.
    WrongStream,
    /// `e` is not this epoch.
    WrongEpoch,
    /// The signature does not verify under the claimed author.
    Signature,
    /// The author is not in this epoch's roster.
    NotAMember,
    /// The author is a member but the role is too low for this kind.
    Role(Role),
    /// A message after the stream's seal.
    AfterSeal,
    /// Authenticated decryption failed: wrong key, or modified bytes.
    Decrypt,
    /// The decrypted object does not match its schema.
    Schema(String),
    /// `created_by.principal` or `holder.principal` is not the signer.
    AuthorMismatch,
    /// A seal that is not from an owner of the epoch.
    BadSeal,
    /// An epoch record that breaks the chain rules.
    BadEpoch(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dropped {
    pub stream: &'static str,
    pub id: String,
    pub reason: DropReason,
}

#[derive(Clone, Debug)]
pub struct StreamPos {
    pub offset: String,
    /// `Stream-Closed` was seen by a read.
    pub closed: bool,
    /// A valid owner-signed seal was read.
    pub sealed: bool,
}

impl Default for StreamPos {
    fn default() -> Self {
        StreamPos {
            offset: START.to_owned(),
            closed: false,
            sealed: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LeaseRec {
    pub stamped: Stamped<Lease>,
    pub seq: u64,
}

pub struct EpochState {
    pub rec: EpochRecord,
    pub id: String,
    pub roster: BTreeMap<String, Role>,
    pub key: Option<EpochKey>,
    pub log: StreamPos,
    pub upd: StreamPos,
    pub lease: StreamPos,
    seen: HashSet<String>,
    pub meta: Option<Meta>,
    pub versions: Vec<Stamped<Version>>,
    /// version id -> where in an `upd` stream to resume reading.
    pub hints: HashMap<String, UpdHint>,
    pub updates: Vec<Stamped<Update>>,
    pub lease_last: Option<LeaseRec>,
}

impl EpochState {
    pub fn path(&self, doc: &str, stream: &str) -> String {
        format!("dcp/{doc}/{}/{stream}", self.rec.prefix)
    }

    pub fn n(&self) -> u64 {
        self.rec.n
    }

    pub fn role_of(&self, principal: &str) -> Option<Role> {
        self.roster.get(principal).copied()
    }
}

pub struct DocCache {
    pub doc: String,
    pub ctl_offset: String,
    ctl_seen: HashSet<String>,
    pub epochs: Vec<EpochState>,
    pub dropped: Vec<Dropped>,
    /// Counts, for measurements: valid records accepted per kind.
    pub accepted: HashMap<&'static str, u64>,
    /// Negative control only: ignore the role check.
    pub lenient_roles: bool,
}

fn decode<T: DeserializeOwned>(v: Value) -> Option<T> {
    serde_json::from_value(v).ok()
}

const MAX_DROPPED: usize = 10_000;

impl DocCache {
    pub fn new(doc: &str) -> Self {
        DocCache {
            doc: doc.to_owned(),
            ctl_offset: START.to_owned(),
            ctl_seen: HashSet::new(),
            epochs: Vec::new(),
            dropped: Vec::new(),
            accepted: HashMap::new(),
            lenient_roles: false,
        }
    }

    pub fn ctl_path(doc: &str) -> String {
        format!("dcp/{doc}/ctl")
    }

    pub fn latest(&self) -> Option<&EpochState> {
        self.epochs.last()
    }

    fn note(&mut self, stream: &'static str, id: &str, reason: DropReason) {
        if self.dropped.len() < MAX_DROPPED {
            self.dropped.push(Dropped {
                stream,
                id: id.to_owned(),
                reason,
            });
        }
    }

    fn bump(&mut self, what: &'static str) {
        *self.accepted.entry(what).or_default() += 1;
    }

    // ------------------------------------------------------------ control

    /// Offer one message of the control stream. Returns true when it
    /// extended the chain.
    pub fn ingest_epoch(&mut self, me: &Identity, v: Value) -> bool {
        let Some(rec) = decode::<EpochRecord>(v) else {
            self.note("ctl", "", DropReason::Malformed);
            return false;
        };
        let id = rec.id();
        if !self.ctl_seen.insert(id.clone()) {
            return false;
        }
        match self.check_epoch(&rec) {
            Ok(roster) => {
                let key = rec
                    .wraps
                    .iter()
                    .find(|w| w.p == me.principal())
                    .and_then(|w| crypto::unwrap(me, &self.doc, rec.n, w).ok());
                self.epochs.push(EpochState {
                    rec,
                    id,
                    roster,
                    key,
                    log: StreamPos::default(),
                    upd: StreamPos::default(),
                    lease: StreamPos::default(),
                    seen: HashSet::new(),
                    meta: None,
                    versions: Vec::new(),
                    hints: HashMap::new(),
                    updates: Vec::new(),
                    lease_last: None,
                });
                self.bump("epoch");
                self.propagate_keys();
                true
            }
            Err(why) => {
                self.note("ctl", &id, DropReason::BadEpoch(why));
                false
            }
        }
    }

    fn check_epoch(&self, rec: &EpochRecord) -> Result<BTreeMap<String, Role>, String> {
        let s = |m: &str| m.to_owned();
        if rec.v != WIRE {
            return Err(s("wire version"));
        }
        if rec.doc != self.doc {
            return Err(s("another document"));
        }
        if rec.n != self.epochs.len() as u64 {
            return Err(format!(
                "expected epoch {}, got {}",
                self.epochs.len(),
                rec.n
            ));
        }
        if rec.prefix.is_empty()
            || rec.prefix.len() > 64
            || !rec
                .prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(s("bad stream prefix"));
        }
        if !rec.verify() {
            return Err(s("signature"));
        }
        let mut roster = BTreeMap::new();
        for m in &rec.roster {
            if crypto::parse_principal(&m.p).is_err() {
                return Err(s("a roster entry is not a principal"));
            }
            if roster.insert(m.p.clone(), m.r).is_some() {
                return Err(s("duplicate roster entry"));
            }
        }
        if !roster.values().any(|r| *r == Role::Owner) {
            return Err(s("no owner"));
        }
        let mut wrapped = HashSet::new();
        for w in &rec.wraps {
            if !roster.contains_key(&w.p) || !wrapped.insert(&w.p) {
                return Err(s("a wrap for a non-member or twice"));
            }
        }
        match self.epochs.last() {
            None => {
                if rec.prev.is_some() || rec.prev_key.is_some() {
                    return Err(s("epoch 0 has a predecessor"));
                }
                let Some(root) = &rec.root else {
                    return Err(s("epoch 0 without a root nonce"));
                };
                if wire::doc_id(&rec.a, root) != self.doc {
                    return Err(s("the document id does not commit to this creator"));
                }
                if roster.get(&rec.a) != Some(&Role::Owner) {
                    return Err(s("the creator is not an owner"));
                }
            }
            Some(last) => {
                if rec.root.is_some() {
                    return Err(s("a root nonce after epoch 0"));
                }
                if rec.prev.as_deref() != Some(last.id.as_str()) {
                    return Err(s("does not follow the previous epoch"));
                }
                if last.roster.get(&rec.a) != Some(&Role::Owner) {
                    return Err(s("not signed by an owner of the previous epoch"));
                }
                if rec.prev_key.is_none() {
                    return Err(s("no sealed previous key"));
                }
            }
        }
        Ok(roster)
    }

    /// Walk back from every epoch whose key is known and open the key of the
    /// epoch before it from the sealed `prev_key`.
    fn propagate_keys(&mut self) {
        let mut i = self.epochs.len();
        while i > 1 {
            i -= 1;
            let Some(key) = self.epochs[i].key.as_ref() else {
                continue;
            };
            if self.epochs[i - 1].key.is_some() {
                continue;
            }
            let Some(sealed) = self.epochs[i].rec.prev_key.clone() else {
                continue;
            };
            let aad = prev_key_aad(&self.doc, self.epochs[i].rec.n);
            let opened = crypto::unb64(&sealed.n)
                .ok()
                .zip(crypto::unb64(&sealed.c).ok())
                .and_then(|(n, c)| crypto::open(key, &n, &aad, &c).ok())
                .and_then(|p| <[u8; 32]>::try_from(p).ok());
            if let Some(k) = opened {
                self.epochs[i - 1].key = Some(zeroize::Zeroizing::new(k));
            }
        }
    }

    // ------------------------------------------------------------ streams

    /// The checks every record of an epoch stream goes through, in this order:
    /// shape, seal, stream, epoch, signature, roster, role, authenticated
    /// decryption. `Err(None)` is a duplicate (not worth a note).
    fn judge(
        &self,
        idx: usize,
        stream: &'static str,
        allowed: &[Kind],
        min_role: Role,
        rec: &Record,
        id: &str,
    ) -> Result<Vec<u8>, Option<DropReason>> {
        let es = &self.epochs[idx];
        if rec.v != WIRE {
            return Err(Some(DropReason::Malformed));
        }
        if es.seen.contains(id) {
            return Err(None);
        }
        let sealed = match stream {
            "log" => es.log.sealed,
            "upd" => es.upd.sealed,
            _ => es.lease.sealed,
        };
        if sealed {
            return Err(Some(DropReason::AfterSeal));
        }
        if !allowed.contains(&rec.k) {
            return Err(Some(DropReason::WrongStream));
        }
        if rec.e != es.rec.n {
            return Err(Some(DropReason::WrongEpoch));
        }
        if !rec.verify(&self.doc) {
            return Err(Some(DropReason::Signature));
        }
        let Some(role) = es.role_of(&rec.a) else {
            return Err(Some(DropReason::NotAMember));
        };
        if role < min_role && !self.lenient_roles {
            return Err(Some(DropReason::Role(role)));
        }
        let Some(key) = es.key.as_ref() else {
            return Err(Some(DropReason::Decrypt));
        };
        crypto::unb64(&rec.n)
            .ok()
            .zip(crypto::unb64(&rec.c).ok())
            .and_then(|(n, c)| {
                crypto::open(key, &n, &Record::aad(&self.doc, rec.k, rec.e, &rec.a), &c).ok()
            })
            .ok_or(Some(DropReason::Decrypt))
    }

    fn open_record(
        &mut self,
        idx: usize,
        stream: &'static str,
        allowed: &[Kind],
        min_role: Role,
        v: Value,
    ) -> Option<(Record, String, Vec<u8>)> {
        let Some(rec) = decode::<Record>(v) else {
            self.note(stream, "", DropReason::Malformed);
            return None;
        };
        let id = rec.id();
        match self.judge(idx, stream, allowed, min_role, &rec, &id) {
            Ok(plain) => {
                self.epochs[idx].seen.insert(id.clone());
                Some((rec, id, plain))
            }
            Err(Some(why)) => {
                self.note(stream, &id, why);
                None
            }
            Err(None) => None,
        }
    }

    fn ingest_seal(&mut self, idx: usize, stream: &'static str, rec: Record, id: &str) {
        let doc = self.doc.clone();
        let es = &self.epochs[idx];
        let ok = rec.verify(&doc)
            && rec.e == es.rec.n
            && rec.x == Some(es.rec.n + 1)
            && es.role_of(&rec.a) == Some(Role::Owner);
        if !ok {
            self.note(stream, id, DropReason::BadSeal);
            return;
        }
        let es = &mut self.epochs[idx];
        match stream {
            "log" => es.log.sealed = true,
            "upd" => es.upd.sealed = true,
            _ => es.lease.sealed = true,
        }
    }

    fn clamp(st: u64, now: u64) -> u64 {
        st.min(now.saturating_add(SKEW_MS))
    }

    /// One message of an epoch's `log` stream: meta, versions, the seal.
    pub fn ingest_log(&mut self, idx: usize, v: Value, now: u64) {
        if v.get("k").and_then(Value::as_str) == Some("seal") {
            if let Some(rec) = decode::<Record>(v) {
                let id = rec.id();
                self.ingest_seal(idx, "log", rec, &id);
            } else {
                self.note("log", "", DropReason::Malformed);
            }
            return;
        }
        let Some((rec, id, plain)) =
            self.open_record(idx, "log", &[Kind::Meta, Kind::Ver], Role::Editor, v)
        else {
            return;
        };
        match rec.k {
            Kind::Meta => {
                if let Some(r) = self.epochs[idx]
                    .role_of(&rec.a)
                    .filter(|r| *r != Role::Owner)
                {
                    self.note("log", &id, DropReason::Role(r));
                    return;
                }
                let Ok(inner) = serde_json::from_slice::<Inner<Meta>>(&plain) else {
                    self.note("log", &id, DropReason::Malformed);
                    return;
                };
                if let Err(e) = schema::validate_typed(SchemaKind::Meta, &inner.body) {
                    self.note("log", &id, DropReason::Schema(e.to_string()));
                    return;
                }
                self.epochs[idx].meta = Some(inner.body);
                self.bump("meta");
            }
            Kind::Ver => {
                let Ok(inner) = serde_json::from_slice::<Inner<Version>>(&plain) else {
                    self.note("log", &id, DropReason::Malformed);
                    return;
                };
                if let Err(e) = schema::validate_typed(SchemaKind::Version, &inner.body) {
                    self.note("log", &id, DropReason::Schema(e.to_string()));
                    return;
                }
                if inner.body.created_by.principal != rec.a {
                    self.note("log", &id, DropReason::AuthorMismatch);
                    return;
                }
                let mut stamped = Stamped::new(
                    id.clone(),
                    Self::clamp(inner.st, now),
                    rec.a.clone(),
                    inner.body,
                );
                stamped.device = Some(stamped.body.created_by.device.clone());
                if let Some(h) = inner.hint {
                    self.epochs[idx].hints.insert(id, h);
                }
                self.epochs[idx].versions.push(stamped);
                self.bump("ver");
            }
            _ => unreachable!("open_record filtered the kinds"),
        }
    }

    pub fn ingest_upd(&mut self, idx: usize, v: Value, now: u64) {
        if v.get("k").and_then(Value::as_str) == Some("seal") {
            if let Some(rec) = decode::<Record>(v) {
                let id = rec.id();
                self.ingest_seal(idx, "upd", rec, &id);
            } else {
                self.note("upd", "", DropReason::Malformed);
            }
            return;
        }
        let Some((rec, id, plain)) = self.open_record(idx, "upd", &[Kind::Upd], Role::Editor, v)
        else {
            return;
        };
        let Ok(inner) = serde_json::from_slice::<Inner<Update>>(&plain) else {
            self.note("upd", &id, DropReason::Malformed);
            return;
        };
        if let Err(e) = schema::validate_typed(SchemaKind::Update, &inner.body) {
            self.note("upd", &id, DropReason::Schema(e.to_string()));
            return;
        }
        let mut stamped = Stamped::new(id, Self::clamp(inner.st, now), rec.a, inner.body);
        stamped.device = inner.dev;
        self.epochs[idx].updates.push(stamped);
        self.bump("upd");
    }

    pub fn ingest_lease(&mut self, idx: usize, v: Value, now: u64) {
        if v.get("k").and_then(Value::as_str) == Some("seal") {
            if let Some(rec) = decode::<Record>(v) {
                let id = rec.id();
                self.ingest_seal(idx, "lease", rec, &id);
            } else {
                self.note("lease", "", DropReason::Malformed);
            }
            return;
        }
        let Some((rec, id, plain)) =
            self.open_record(idx, "lease", &[Kind::Lease], Role::Editor, v)
        else {
            return;
        };
        let Ok(inner) = serde_json::from_slice::<LeaseInner>(&plain) else {
            self.note("lease", &id, DropReason::Malformed);
            return;
        };
        if let Err(e) = schema::validate_typed(SchemaKind::Lease, &inner.body) {
            self.note("lease", &id, DropReason::Schema(e.to_string()));
            return;
        }
        if inner
            .body
            .holder
            .as_ref()
            .is_some_and(|h| h.principal != rec.a)
        {
            self.note("lease", &id, DropReason::AuthorMismatch);
            return;
        }
        let Some(seq) = inner.seq else {
            self.note("lease", &id, DropReason::Malformed);
            return;
        };
        let stamped = Stamped::new(id, Self::clamp(inner.st, now), rec.a, inner.body);
        let newer = self.epochs[idx]
            .lease_last
            .as_ref()
            .is_none_or(|cur| seq > cur.seq);
        if newer {
            self.epochs[idx].lease_last = Some(LeaseRec { stamped, seq });
        }
        self.bump("lease");
    }
}

/// The lease plaintext: [`Inner`] plus the writer's sequence number, the
/// value it sent as `Stream-Seq`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseInner {
    pub st: u64,
    pub seq: Option<u64>,
    pub body: Lease,
}

pub fn prev_key_aad(doc: &str, epoch: u64) -> Vec<u8> {
    let mut v = b"dcp-streams/1/prevkey".to_vec();
    v.extend_from_slice(doc.as_bytes());
    v.extend_from_slice(&epoch.to_be_bytes());
    v
}

/// A new document: its id, its epoch 0 record (signed, the creator the only
/// owner and member) and the epoch key.
pub fn genesis(me: &Identity) -> (String, EpochRecord, EpochKey) {
    let principal = me.principal();
    let root = crypto::b64(&crypto::random::<16>());
    let doc = wire::doc_id(&principal, &root);
    let key = crypto::new_epoch_key();
    let mut rec = EpochRecord {
        v: WIRE,
        doc: doc.clone(),
        n: 0,
        prefix: format!("e0-{}", hex::encode(crypto::random::<6>())),
        prev: None,
        root: Some(root),
        roster: vec![wire::Member {
            p: principal.clone(),
            r: Role::Owner,
        }],
        wraps: vec![crypto::wrap(&principal, &doc, 0, &key).expect("wrapping to oneself")],
        prev_key: None,
        a: principal,
        s: String::new(),
    };
    rec.s = crypto::b64(&me.sign(&rec.signed_bytes()));
    (doc, rec, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::b64;
    use dcp_core::{Author, Level};
    use serde_json::json;

    fn meta() -> Meta {
        Meta {
            dcp: 1,
            title: "t".into(),
            mime: "application/vnd.oasis.opendocument.text".into(),
            levels: vec![Level::L1],
            algorithms: None,
            editor_services: None,
        }
    }

    fn signed(
        doc: &str,
        who: &Identity,
        key: &[u8; 32],
        kind: Kind,
        epoch: u64,
        inner: &impl serde::Serialize,
    ) -> Value {
        let me = who.principal();
        let (n, c) = crypto::seal(
            key,
            &Record::aad(doc, kind, epoch, &me),
            &serde_json::to_vec(inner).unwrap(),
        );
        let mut r = Record {
            v: WIRE,
            k: kind,
            e: epoch,
            a: me,
            n: b64(&n),
            c: b64(&c),
            s: String::new(),
            x: None,
        };
        r.s = b64(&who.sign(&r.signed_bytes(doc)));
        serde_json::to_value(r).unwrap()
    }

    fn seal_of(doc: &str, who: &Identity, epoch: u64) -> Value {
        let mut r = Record {
            v: WIRE,
            k: Kind::Seal,
            e: epoch,
            a: who.principal(),
            n: String::new(),
            c: String::new(),
            s: String::new(),
            x: Some(epoch + 1),
        };
        r.s = b64(&who.sign(&r.signed_bytes(doc)));
        serde_json::to_value(r).unwrap()
    }

    fn version(by: &Identity, parents: &[&str]) -> Inner<Version> {
        Inner {
            st: 1_000,
            dev: None,
            body: Version {
                dcp: 1,
                parents: parents.iter().map(|p| p.to_string()).collect(),
                file: dcp_core::FileRef {
                    mime: "application/x".into(),
                    size: 1,
                    sha256: "0".repeat(64),
                    locator: dcp_core::Locator::DurableStream {
                        href: "https://h.invalid/dcp/d/f/x".into(),
                    },
                },
                created_by: Author {
                    principal: by.principal(),
                    device: "d".into(),
                },
                message: None,
                covers: None,
            },
            hint: None,
        }
    }

    fn rig() -> (Identity, DocCache, EpochKey) {
        let owner = Identity::generate();
        let (doc, rec, key) = genesis(&owner);
        let mut c = DocCache::new(&doc);
        assert!(c.ingest_epoch(&owner, serde_json::to_value(rec).unwrap()));
        (owner, c, key)
    }

    #[test]
    fn nothing_after_a_seal_counts_and_a_non_owner_cannot_seal() {
        let (owner, mut c, key) = rig();
        let doc = c.doc.clone();
        c.ingest_log(
            0,
            signed(
                &doc,
                &owner,
                &key,
                Kind::Meta,
                0,
                &Inner {
                    st: 1,
                    dev: None,
                    body: meta(),
                    hint: None,
                },
            ),
            10,
        );
        c.ingest_log(
            0,
            signed(&doc, &owner, &key, Kind::Ver, 0, &version(&owner, &[])),
            10,
        );
        assert_eq!(c.epochs[0].versions.len(), 1);
        // A record by someone who is not in the roster does not seal.
        let stranger = Identity::generate();
        c.ingest_log(0, seal_of(&doc, &stranger, 0), 10);
        assert!(!c.epochs[0].log.sealed);
        assert!(c.dropped.iter().any(|d| d.reason == DropReason::BadSeal));
        // The owner's does.
        c.ingest_log(0, seal_of(&doc, &owner, 0), 10);
        assert!(c.epochs[0].log.sealed);
        // After it, even a perfectly valid record from a member is dropped.
        let id = c.epochs[0].versions[0].id.clone();
        c.ingest_log(
            0,
            signed(&doc, &owner, &key, Kind::Ver, 0, &version(&owner, &[&id])),
            10,
        );
        assert_eq!(c.epochs[0].versions.len(), 1);
        assert!(c.dropped.iter().any(|d| d.reason == DropReason::AfterSeal));
    }

    #[test]
    fn a_far_future_timestamp_is_clamped_and_a_replayed_record_is_ignored() {
        let (owner, mut c, key) = rig();
        let doc = c.doc.clone();
        let mut inner = version(&owner, &[]);
        inner.st = 10_000_000_000;
        let rec = signed(&doc, &owner, &key, Kind::Ver, 0, &inner);
        c.ingest_log(0, rec.clone(), 5_000);
        c.ingest_log(0, rec, 5_000);
        assert_eq!(c.epochs[0].versions.len(), 1, "a duplicate is ignored");
        assert_eq!(
            c.epochs[0].versions[0].ts_ms,
            5_000 + SKEW_MS,
            "clamped to the reader's own server time plus the allowed skew"
        );
    }

    #[test]
    fn an_author_who_is_not_the_signer_is_dropped() {
        let (owner, mut c, key) = rig();
        let doc = c.doc.clone();
        let someone_else = Identity::generate();
        let inner = version(&someone_else, &[]); // created_by names another principal
        c.ingest_log(0, signed(&doc, &owner, &key, Kind::Ver, 0, &inner), 10);
        assert!(c.epochs[0].versions.is_empty());
        assert!(c
            .dropped
            .iter()
            .any(|d| d.reason == DropReason::AuthorMismatch));
        let _ = json!(null);
    }
}
