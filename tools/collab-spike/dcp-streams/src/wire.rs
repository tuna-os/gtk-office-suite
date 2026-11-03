//! The bytes on the server. Everything a client stores in a stream is one JSON
//! message of one of two shapes:
//!
//! - an [`EpochRecord`] in the document's control stream (`ctl`): cleartext,
//!   signed by an owner. It fixes the roster and the key of one epoch, and
//!   wraps the epoch key for every member;
//! - a [`Record`] in an epoch's `log`, `upd` or `lease` stream: an
//!   XChaCha20-Poly1305 ciphertext under the epoch key, signed by its author
//!   over the ciphertext and the header (encrypt-then-sign), or a cleartext
//!   `seal` that ends the stream.
//!
//! The signed byte strings are built from length-prefixed fields, not from
//! JSON text, so that no JSON canonicalisation is needed to verify them.

use dcp_core::Role;
use serde::{Deserialize, Serialize};

use crate::crypto::{self, sha256_hex};

/// The wire version, carried in every message. Bump on any change of bytes.
pub const WIRE: u8 = 1;
const DOMAIN: &[u8] = b"dcp-streams/1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    #[serde(rename = "meta")]
    Meta,
    #[serde(rename = "ver")]
    Ver,
    #[serde(rename = "upd")]
    Upd,
    #[serde(rename = "lease")]
    Lease,
    /// The last message of a stream of a finished epoch. Cleartext.
    #[serde(rename = "seal")]
    Seal,
}

impl Kind {
    pub fn tag(self) -> &'static str {
        match self {
            Kind::Meta => "meta",
            Kind::Ver => "ver",
            Kind::Upd => "upd",
            Kind::Lease => "lease",
            Kind::Seal => "seal",
        }
    }
}

/// One message of an epoch stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub v: u8,
    pub k: Kind,
    /// The epoch whose key encrypts `c`. For a seal, the epoch it ends.
    pub e: u64,
    /// Author: a principal string.
    pub a: String,
    /// base64url nonce (empty for a seal).
    pub n: String,
    /// base64url ciphertext (empty for a seal).
    pub c: String,
    /// base64url Ed25519 signature over [`Record::signed_bytes`].
    pub s: String,
    /// A seal only: the epoch that follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<u64>,
}

fn put(out: &mut Vec<u8>, part: &[u8]) {
    out.extend_from_slice(&(part.len() as u32).to_be_bytes());
    out.extend_from_slice(part);
}

impl Record {
    /// What the author signs: the domain, the document, the header and the
    /// ciphertext. Binding the document and the kind stops a record being
    /// replayed into another document or another stream.
    pub fn signed_bytes(&self, doc: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(160 + self.c.len());
        put(&mut out, DOMAIN);
        put(&mut out, b"record");
        put(&mut out, doc.as_bytes());
        put(&mut out, self.k.tag().as_bytes());
        put(&mut out, &self.e.to_be_bytes());
        put(&mut out, self.a.as_bytes());
        put(&mut out, &self.x.unwrap_or(u64::MAX).to_be_bytes());
        put(&mut out, self.n.as_bytes());
        put(&mut out, self.c.as_bytes());
        out
    }

    /// The AEAD associated data.
    pub fn aad(doc: &str, k: Kind, e: u64, a: &str) -> Vec<u8> {
        let mut out = Vec::new();
        put(&mut out, DOMAIN);
        put(&mut out, b"aead");
        put(&mut out, doc.as_bytes());
        put(&mut out, k.tag().as_bytes());
        put(&mut out, &e.to_be_bytes());
        put(&mut out, a.as_bytes());
        out
    }

    /// The identifier: SHA-256 of the record as normalised by this
    /// serialisation. Content-addressed, so a duplicate append has the same id.
    pub fn id(&self) -> String {
        sha256_hex(&[&serde_json::to_vec(self).expect("a Record always serialises")])
    }

    pub fn verify(&self, doc: &str) -> bool {
        match crypto::unb64(&self.s) {
            Ok(sig) => crypto::verify(&self.a, &self.signed_bytes(doc), &sig),
            Err(_) => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub p: String,
    pub r: Role,
}

/// An epoch key wrapped for one member.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wrap {
    pub p: String,
    pub eph: String,
    pub n: String,
    pub c: String,
}

/// Bytes sealed under a key, with their nonce.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sealed {
    pub n: String,
    pub c: String,
}

/// The control stream's only message type: the roster and key of one epoch.
///
/// Rules a reader applies (see `doc.rs`): epochs are numbered 0, 1, 2, ... in
/// stream order; epoch 0 is signed by the creator named in the document id;
/// epoch `n + 1` is signed by an owner of epoch `n` and names the hash of
/// epoch `n`; the first valid record for each number wins.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpochRecord {
    pub v: u8,
    pub doc: String,
    pub n: u64,
    /// The random path component of this epoch's streams.
    pub prefix: String,
    /// Hash of the previous epoch record (none for epoch 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
    /// Epoch 0 only: the random nonce the document id commits to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    pub roster: Vec<Member>,
    pub wraps: Vec<Wrap>,
    /// The previous epoch's key, sealed under this epoch's key, so that a
    /// member of this epoch can read what came before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_key: Option<Sealed>,
    pub a: String,
    pub s: String,
}

pub fn role_tag(r: Role) -> &'static str {
    match r {
        Role::Viewer => "viewer",
        Role::Commenter => "commenter",
        Role::Editor => "editor",
        Role::Owner => "owner",
    }
}

impl EpochRecord {
    pub fn signed_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put(&mut out, DOMAIN);
        put(&mut out, b"epoch");
        put(&mut out, self.doc.as_bytes());
        put(&mut out, &self.n.to_be_bytes());
        put(&mut out, self.prefix.as_bytes());
        put(&mut out, self.prev.as_deref().unwrap_or("").as_bytes());
        put(&mut out, self.root.as_deref().unwrap_or("").as_bytes());
        put(&mut out, &(self.roster.len() as u32).to_be_bytes());
        for m in &self.roster {
            put(&mut out, m.p.as_bytes());
            put(&mut out, role_tag(m.r).as_bytes());
        }
        put(&mut out, &(self.wraps.len() as u32).to_be_bytes());
        for w in &self.wraps {
            put(&mut out, w.p.as_bytes());
            put(&mut out, w.eph.as_bytes());
            put(&mut out, w.n.as_bytes());
            put(&mut out, w.c.as_bytes());
        }
        match &self.prev_key {
            Some(s) => {
                put(&mut out, s.n.as_bytes());
                put(&mut out, s.c.as_bytes());
            }
            None => {
                put(&mut out, b"");
                put(&mut out, b"");
            }
        }
        put(&mut out, self.a.as_bytes());
        out
    }

    pub fn verify(&self) -> bool {
        match crypto::unb64(&self.s) {
            Ok(sig) => crypto::verify(&self.a, &self.signed_bytes(), &sig),
            Err(_) => false,
        }
    }

    /// The hash the next epoch names in `prev`.
    pub fn id(&self) -> String {
        sha256_hex(&[&serde_json::to_vec(self).expect("an EpochRecord always serialises")])
    }
}

/// The document id commits to its creator: anyone holding the id can tell a
/// genuine epoch 0 from one the server made up.
pub fn doc_id(creator: &str, root_nonce: &str) -> String {
    let h = sha256_hex(&[DOMAIN, b"doc", creator.as_bytes(), root_nonce.as_bytes()]);
    crypto::b64(&hex::decode(&h).expect("hex of a digest")[..16])
}

/// The plaintext inside a [`Record`]'s ciphertext.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inner<T> {
    /// Server time in milliseconds as the author last observed it (see
    /// `clock.rs`): the only timestamp in this binding. A reader clamps it.
    pub st: u64,
    /// The author's device, for updates (their `seq` is per principal and device).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev: Option<String>,
    pub body: T,
    /// A version that `covers` updates: the offset in the epoch's `upd` stream
    /// to resume reading at, so that a late reader skips what the version covers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<UpdHint>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdHint {
    /// The epoch whose `upd` stream the offset belongs to.
    pub epoch: u64,
    pub offset: String,
}

/// A file blob: `"DCPF" 0x01 || epoch u64 BE || nonce || ciphertext || tag`.
pub const BLOB_MAGIC: &[u8; 5] = b"DCPF\x01";
pub const BLOB_HEADER: usize = 5 + 8;

pub fn blob_aad(doc: &str, file_id: &str, epoch: u64) -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, DOMAIN);
    put(&mut out, b"file");
    put(&mut out, doc.as_bytes());
    put(&mut out, file_id.as_bytes());
    put(&mut out, &epoch.to_be_bytes());
    out
}
