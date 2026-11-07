//! The primitives, all from RustCrypto / dalek, and nothing invented:
//!
//! - XChaCha20-Poly1305 for every record and file (one random 192-bit nonce
//!   per message, so no counter has to survive a crash or be shared by two
//!   writers holding the same epoch key);
//! - Ed25519 per principal for authorship (the principal string IS the public
//!   key, so nothing has to be looked up to verify it);
//! - X25519 key wrapping to a member: the member's X25519 public key is the
//!   Edwards-to-Montgomery image of its Ed25519 public key (the same
//!   conversion libsodium offers), an ephemeral key per wrap, HKDF-SHA256 over
//!   the shared secret with the epoch and recipient bound in, then the same
//!   AEAD. This is ECIES; it is not age and not HPKE, and it is not
//!   post-quantum.

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey as XPublic, StaticSecret as XSecret};
use zeroize::Zeroizing;

use crate::wire::Wrap;

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 24;
pub const TAG_LEN: usize = 16;
pub const PRINCIPAL_PREFIX: &str = "ed25519:";

/// An epoch key. Zeroised on drop.
pub type EpochKey = Zeroizing<[u8; KEY_LEN]>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CryptoError {
    #[error("decryption failed (wrong key or modified data)")]
    Decrypt,
    #[error("not a valid principal: {0}")]
    Principal(String),
    #[error("bad encoding: {0}")]
    Encoding(String),
    #[error("the shared secret is not contributory (low-order public key)")]
    LowOrder,
}

pub fn b64(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

pub fn unb64(s: &str) -> Result<Vec<u8>, CryptoError> {
    B64.decode(s)
        .map_err(|e| CryptoError::Encoding(e.to_string()))
}

pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("the operating system has no randomness");
    b
}

pub fn sha256_hex(parts: &[&[u8]]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    hex::encode(h.finalize())
}

pub fn new_epoch_key() -> EpochKey {
    Zeroizing::new(random::<KEY_LEN>())
}

/// Seal `plaintext` under `key`. Returns `(nonce, ciphertext || tag)`.
pub fn seal(key: &[u8; KEY_LEN], aad: &[u8], plaintext: &[u8]) -> ([u8; NONCE_LEN], Vec<u8>) {
    let nonce = random::<NONCE_LEN>();
    let ct = XChaCha20Poly1305::new(key.into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .expect("XChaCha20-Poly1305 encryption does not fail for in-memory input");
    (nonce, ct)
}

pub fn open(
    key: &[u8; KEY_LEN],
    nonce: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if nonce.len() != NONCE_LEN {
        return Err(CryptoError::Decrypt);
    }
    XChaCha20Poly1305::new(key.into())
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| CryptoError::Decrypt)
}

/// A principal's keys. The principal string is `ed25519:<base64url public key>`.
#[derive(Clone)]
pub struct Identity {
    sk: SigningKey,
}

impl Identity {
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Identity {
            sk: SigningKey::from_bytes(&seed),
        }
    }

    pub fn generate() -> Self {
        Self::from_seed(random::<32>())
    }

    pub fn principal(&self) -> String {
        principal_of(&self.sk.verifying_key())
    }

    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.sk.sign(msg).to_bytes()
    }

    fn x25519_secret(&self) -> XSecret {
        XSecret::from(self.sk.to_scalar_bytes())
    }
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Identity({})", self.principal())
    }
}

pub fn principal_of(pk: &VerifyingKey) -> String {
    format!("{PRINCIPAL_PREFIX}{}", b64(pk.as_bytes()))
}

pub fn parse_principal(p: &str) -> Result<VerifyingKey, CryptoError> {
    let rest = p
        .strip_prefix(PRINCIPAL_PREFIX)
        .ok_or_else(|| CryptoError::Principal(p.to_owned()))?;
    let raw = unb64(rest)?;
    let arr: [u8; 32] = raw
        .try_into()
        .map_err(|_| CryptoError::Principal(p.to_owned()))?;
    VerifyingKey::from_bytes(&arr).map_err(|_| CryptoError::Principal(p.to_owned()))
}

/// Strict Ed25519 verification (rejects non-canonical and small-order inputs).
pub fn verify(principal: &str, msg: &[u8], sig: &[u8]) -> bool {
    let Ok(pk) = parse_principal(principal) else {
        return false;
    };
    let Ok(sig) = Signature::from_slice(sig) else {
        return false;
    };
    pk.verify_strict(msg, &sig).is_ok()
}

fn wrap_info(doc: &str, epoch: u64, recipient: &str) -> Vec<u8> {
    let mut v = b"dcp-streams/1/wrap".to_vec();
    for part in [doc.as_bytes(), recipient.as_bytes()] {
        v.extend_from_slice(&(part.len() as u32).to_be_bytes());
        v.extend_from_slice(part);
    }
    v.extend_from_slice(&epoch.to_be_bytes());
    v
}

fn wrap_key_for(shared: &[u8], eph: &[u8; 32], recipient_x: &[u8; 32], info: &[u8]) -> [u8; 32] {
    let mut salt = [0u8; 64];
    salt[..32].copy_from_slice(eph);
    salt[32..].copy_from_slice(recipient_x);
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared);
    let mut okm = [0u8; 32];
    hk.expand(info, &mut okm)
        .expect("32 bytes is a valid HKDF-SHA256 length");
    okm
}

/// Wrap `key` for `recipient` (a principal string).
pub fn wrap(
    recipient: &str,
    doc: &str,
    epoch: u64,
    key: &[u8; KEY_LEN],
) -> Result<Wrap, CryptoError> {
    let pk = parse_principal(recipient)?;
    let recipient_x = pk.to_montgomery().to_bytes();
    let eph_secret = XSecret::from(random::<32>());
    let eph_public = XPublic::from(&eph_secret).to_bytes();
    let shared = eph_secret.diffie_hellman(&XPublic::from(recipient_x));
    if !shared.was_contributory() {
        return Err(CryptoError::LowOrder);
    }
    let kek = Zeroizing::new(wrap_key_for(
        shared.as_bytes(),
        &eph_public,
        &recipient_x,
        &wrap_info(doc, epoch, recipient),
    ));
    let (nonce, ct) = seal(&kek, recipient.as_bytes(), key);
    Ok(Wrap {
        p: recipient.to_owned(),
        eph: b64(&eph_public),
        n: b64(&nonce),
        c: b64(&ct),
    })
}

/// Open a wrap addressed to `me`.
pub fn unwrap(me: &Identity, doc: &str, epoch: u64, w: &Wrap) -> Result<EpochKey, CryptoError> {
    if w.p != me.principal() {
        return Err(CryptoError::Decrypt);
    }
    let eph: [u8; 32] = unb64(&w.eph)?
        .try_into()
        .map_err(|_| CryptoError::Encoding("ephemeral key length".into()))?;
    let my_x = XPublic::from(&me.x25519_secret()).to_bytes();
    let shared = me.x25519_secret().diffie_hellman(&XPublic::from(eph));
    if !shared.was_contributory() {
        return Err(CryptoError::LowOrder);
    }
    let kek = Zeroizing::new(wrap_key_for(
        shared.as_bytes(),
        &eph,
        &my_x,
        &wrap_info(doc, epoch, &w.p),
    ));
    let plain = open(&kek, &unb64(&w.n)?, w.p.as_bytes(), &unb64(&w.c)?)?;
    let key: [u8; KEY_LEN] = plain.try_into().map_err(|_| CryptoError::Decrypt)?;
    Ok(Zeroizing::new(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrip_and_tamper() {
        let k = new_epoch_key();
        let (n, ct) = seal(&k, b"aad", b"hello");
        assert_eq!(open(&k, &n, b"aad", &ct).unwrap(), b"hello");
        assert!(open(&k, &n, b"other aad", &ct).is_err());
        let mut bad = ct.clone();
        bad[0] ^= 1;
        assert!(open(&k, &n, b"aad", &bad).is_err());
        let other = new_epoch_key();
        assert!(open(&other, &n, b"aad", &ct).is_err());
    }

    #[test]
    fn sign_verify() {
        let id = Identity::generate();
        let sig = id.sign(b"msg");
        assert!(verify(&id.principal(), b"msg", &sig));
        assert!(!verify(&id.principal(), b"msh", &sig));
        assert!(!verify(&Identity::generate().principal(), b"msg", &sig));
    }

    #[test]
    fn wrap_roundtrip_only_for_the_recipient() {
        let alice = Identity::generate();
        let mallory = Identity::generate();
        let k = new_epoch_key();
        let w = wrap(&alice.principal(), "doc", 3, &k).unwrap();
        assert_eq!(*unwrap(&alice, "doc", 3, &w).unwrap(), *k);
        assert!(unwrap(&alice, "doc", 4, &w).is_err(), "epoch is bound in");
        assert!(
            unwrap(&alice, "other", 3, &w).is_err(),
            "document is bound in"
        );
        assert!(unwrap(&mallory, "doc", 3, &w).is_err());
        let mut retargeted = w.clone();
        retargeted.p = mallory.principal();
        assert!(unwrap(&mallory, "doc", 3, &retargeted).is_err());
    }
}
