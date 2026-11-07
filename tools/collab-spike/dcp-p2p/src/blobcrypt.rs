//! File bytes are encrypted before they enter the blob store.
//!
//! iroh-blobs addresses a blob by the BLAKE3 hash of its bytes and serves it to any peer that asks
//! for that hash and is not refused by the provider's own gate. So the file is sealed with a fresh
//! XChaCha20-Poly1305 key (p2panda-encryption's own primitive) and the key travels only inside the
//! space-encrypted version entry: a removed member who learns a hash still cannot read the file.

use p2panda_encryption::crypto::xchacha20::{x_aead_decrypt, x_aead_encrypt, XAeadKey, XAeadNonce};
use p2panda_encryption::Rng;

pub const NONCE_LEN: usize = 24;

#[derive(Debug, thiserror::Error)]
pub enum BlobCryptError {
    #[error("random number generator: {0}")]
    Rng(String),
    #[error("encrypting the file failed: {0}")]
    Encrypt(String),
    #[error("the blob is shorter than a nonce")]
    TooShort,
    #[error("decrypting the blob failed (wrong key, or the bytes were altered): {0}")]
    Decrypt(String),
}

/// `nonce || ciphertext-with-tag`, and the key.
pub fn seal(rng: &Rng, plaintext: &[u8]) -> Result<(Vec<u8>, XAeadKey), BlobCryptError> {
    let key: XAeadKey = rng
        .random_array()
        .map_err(|e| BlobCryptError::Rng(e.to_string()))?;
    let nonce: XAeadNonce = rng
        .random_array()
        .map_err(|e| BlobCryptError::Rng(e.to_string()))?;
    let ct = x_aead_encrypt(&key, plaintext, nonce, None)
        .map_err(|e| BlobCryptError::Encrypt(e.to_string()))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok((out, key))
}

pub fn open(key: &XAeadKey, sealed: &[u8]) -> Result<Vec<u8>, BlobCryptError> {
    if sealed.len() < NONCE_LEN {
        return Err(BlobCryptError::TooShort);
    }
    let (nonce, ct) = sealed.split_at(NONCE_LEN);
    let nonce: XAeadNonce = nonce.try_into().map_err(|_| BlobCryptError::TooShort)?;
    x_aead_decrypt(key, ct, nonce, None).map_err(|e| BlobCryptError::Decrypt(e.to_string()))
}
