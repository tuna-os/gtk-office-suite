//! Every application plaintext is signed by its author, inside the space encryption.
//!
//! Why: `p2panda_spaces::Event::Application` carries only `{ space_id, data }`. It names neither
//! the operation nor the author, and a plaintext that was queued before the receiver was welcomed
//! comes out later, as a by-product of processing a different message. So the author of a decrypted
//! entry cannot be taken from the operation that carried it. The author signs the entry body (with
//! the document id, the auth operations the author had seen, a clock reading and a nonce); a reader
//! verifies the signature and uses the signer as the author. The entry id is the BLAKE3 hash of the
//! body, so every replica names the same entry the same way.

use dcp_core::StoreError;
use p2panda_core::{Hash, Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::envelope::{parse_principal, principal_of, Envelope};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Body {
    /// The signer, 64 hex characters. A reader checks the signature against exactly this key.
    pub author: String,
    /// The document id: an entry cannot be replayed into another document.
    pub doc: String,
    /// The auth operations (hex of their hash) the author had processed: the causal point at which
    /// the author claims to have had the right to write. See `auth_check.rs`.
    pub heads: Vec<String>,
    pub ts_us: u64,
    pub nonce: String,
    pub env: Envelope,
}

#[derive(Serialize, Deserialize)]
struct Plain {
    /// The JSON text of a [`Body`], kept as a string so the signed bytes are exactly the bytes sent.
    body: String,
    sig: String,
}

pub struct Verified {
    pub id: String,
    pub body: Body,
    pub author: VerifyingKey,
}

/// Returns the plaintext to encrypt and the entry id.
pub fn sign(key: &SigningKey, body: &Body) -> Result<(Vec<u8>, String), StoreError> {
    let text = serde_json::to_string(body).map_err(|e| StoreError::Invalid(e.to_string()))?;
    let sig = key.sign(text.as_bytes());
    let id = Hash::digest(text.as_bytes()).to_hex();
    let plain = Plain {
        body: text,
        sig: sig.to_hex(),
    };
    let bytes = serde_json::to_vec(&plain).map_err(|e| StoreError::Invalid(e.to_string()))?;
    Ok((bytes, id))
}

pub fn verify(plaintext: &[u8], doc: &str) -> Result<Verified, String> {
    let plain: Plain = serde_json::from_slice(plaintext).map_err(|e| format!("not an entry: {e}"))?;
    let body: Body = serde_json::from_str(&plain.body).map_err(|e| format!("not an entry body: {e}"))?;
    if body.doc != doc {
        return Err(format!("entry of document {} found in {}", body.doc, doc));
    }
    let author = parse_principal(&body.author).ok_or("the author is not a public key")?;
    let sig: Signature = plain.sig.parse().map_err(|_| "the signature is not hex")?;
    if !author.verify(plain.body.as_bytes(), &sig) {
        return Err(format!(
            "the signature does not match the claimed author {}",
            principal_of(&author)
        ));
    }
    Ok(Verified {
        id: Hash::digest(plain.body.as_bytes()).to_hex(),
        body,
        author,
    })
}
