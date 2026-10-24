//! File integrity, spec 4.2 version rule 2: a writer records `size` and
//! `sha256` of the plaintext file; a reader fetches the file, recomputes both
//! and discards the version if either differs.

use sha2::{Digest, Sha256};

use crate::{FileRef, Locator, Stamped, Version};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IntegrityError {
    #[error("size mismatch: the reference says {expected} bytes, got {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("sha256 mismatch: the reference says {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
}

/// Lower-case hex sha256, the form the schema's `sha256` pattern requires.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Build the reference a writer records for `bytes` it is about to publish.
pub fn file_ref(mime: &str, bytes: &[u8], locator: Locator) -> FileRef {
    FileRef { mime: mime.to_owned(), size: bytes.len() as u64, sha256: sha256_hex(bytes), locator }
}

/// Check fetched plaintext against its reference. The size is compared first
/// (it is free, and it is what a truncated download changes), then the hash.
pub fn verify(file_ref: &FileRef, bytes: &[u8]) -> Result<(), IntegrityError> {
    let actual_size = bytes.len() as u64;
    if actual_size != file_ref.size {
        return Err(IntegrityError::SizeMismatch { expected: file_ref.size, actual: actual_size });
    }
    let actual = sha256_hex(bytes);
    if actual != file_ref.sha256 {
        return Err(IntegrityError::HashMismatch { expected: file_ref.sha256.clone(), actual });
    }
    Ok(())
}

/// A version the reader discarded, and why. "Discard the version and report
/// which version" (spec 10).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("version {id} discarded: {cause}")]
pub struct Discarded {
    pub id: String,
    pub cause: IntegrityError,
}

pub fn verify_version(version: &Stamped<Version>, bytes: &[u8]) -> Result<(), Discarded> {
    verify(&version.body.file, bytes).map_err(|cause| Discarded { id: version.id.clone(), cause })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url() -> Locator {
        Locator::Url { href: "https://example.org/f".into() }
    }

    #[test]
    fn sha256_of_known_input() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn built_reference_has_size_and_hash_and_verifies() {
        let f = file_ref("application/vnd.oasis.opendocument.text", b"hello", url());
        assert_eq!(f.size, 5);
        assert_eq!(f.sha256.len(), 64);
        verify(&f, b"hello").unwrap();
    }

    #[test]
    fn truncation_is_a_size_mismatch() {
        let f = file_ref("text/plain", b"hello world", url());
        assert_eq!(verify(&f, b"hello"), Err(IntegrityError::SizeMismatch { expected: 11, actual: 5 }));
    }

    #[test]
    fn same_size_corruption_is_a_hash_mismatch() {
        let f = file_ref("text/plain", b"hello", url());
        assert!(matches!(verify(&f, b"hellp"), Err(IntegrityError::HashMismatch { .. })));
    }

    #[test]
    fn appended_bytes_and_empty_files_are_rejected() {
        let f = file_ref("text/plain", b"hello", url());
        assert!(matches!(verify(&f, b"hello!"), Err(IntegrityError::SizeMismatch { .. })));
        assert!(matches!(verify(&f, b""), Err(IntegrityError::SizeMismatch { .. })));
        // An empty file is a legitimate file with its own hash.
        let e = file_ref("text/plain", b"", url());
        verify(&e, b"").unwrap();
        assert!(verify(&e, b"x").is_err());
    }

    #[test]
    fn right_size_and_wrong_recorded_hash_is_rejected() {
        let mut f = file_ref("text/plain", b"hello", url());
        f.sha256 = sha256_hex(b"other");
        assert!(matches!(verify(&f, b"hello"), Err(IntegrityError::HashMismatch { .. })));
        // A reference with an upper-case hash is not the schema's form either.
        let mut f = file_ref("text/plain", b"hello", url());
        f.sha256 = f.sha256.to_uppercase();
        assert!(verify(&f, b"hello").is_err());
    }

    #[test]
    fn discard_names_the_version() {
        use crate::{Author, Version};
        let v = Stamped::new(
            "$v2",
            5,
            "@a:x",
            Version {
                dcp: 0,
                parents: vec![],
                file: file_ref("text/plain", b"abc", url()),
                created_by: Author { principal: "@a:x".into(), device: "D".into() },
                message: None,
                covers: None,
            },
        );
        verify_version(&v, b"abc").unwrap();
        let d = verify_version(&v, b"abd").unwrap_err();
        assert_eq!(d.id, "$v2");
        assert!(d.to_string().contains("$v2"));
    }
}
