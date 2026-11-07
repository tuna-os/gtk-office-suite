//! What a DCP entry carries. Every entry of a document is a p2panda operation whose payload is a
//! space-encrypted JSON `Envelope`; the DCP objects inside are exactly the draft-1 schemas.

use dcp_core::{Lease, Meta, Update, Version};
use p2panda_core::VerifyingKey;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Envelope {
    Meta {
        meta: Meta,
    },
    /// `blob` is the BLAKE3 hash (hex) of the sealed file in iroh-blobs and `key` the 32-byte
    /// file key (hex): the binding's own fields, outside the DCP `Version` object.
    Version {
        version: Version,
        blob: String,
        key: String,
    },
    /// `lamport` orders concurrent lease writes without a clock (see the results document).
    Lease {
        lease: Lease,
        lamport: u64,
    },
    Update {
        update: Update,
        device: String,
    },
}

/// A principal of this binding is the Ed25519 public key as 64 lower-case hex characters. It is
/// the iroh endpoint id, the p2panda author and the p2panda-auth actor at once.
pub fn principal_of(key: &VerifyingKey) -> String {
    key.to_hex()
}

pub fn parse_principal(s: &str) -> Option<VerifyingKey> {
    s.parse().ok()
}

/// The locator a version's file gets: `blake3`, the hash of the bytes the provider serves (spec
/// 4.2.1, added in draft 1 update 2). Those bytes are the sealed file, so the hash is not the
/// version's `sha256`. `blake3_hex` is iroh's lower-case 64-digit hex form.
pub fn blob_locator(blake3_hex: &str) -> dcp_core::Locator {
    dcp_core::Locator::Blake3 {
        hash: blake3_hex.to_owned(),
    }
}

pub fn blob_of_locator(loc: &dcp_core::Locator) -> Option<String> {
    match loc {
        dcp_core::Locator::Blake3 { hash } => Some(hash.clone()),
        _ => None,
    }
}
