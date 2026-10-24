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
    FileRef {
        mime: mime.to_owned(),
        size: bytes.len() as u64,
        sha256: sha256_hex(bytes),
        locator,
    }
}

/// Check fetched plaintext against its reference. The size is compared first
/// (it is free, and it is what a truncated download changes), then the hash.
pub fn verify(file_ref: &FileRef, bytes: &[u8]) -> Result<(), IntegrityError> {
    let actual_size = bytes.len() as u64;
    if actual_size != file_ref.size {
        return Err(IntegrityError::SizeMismatch {
            expected: file_ref.size,
            actual: actual_size,
        });
    }
    let actual = sha256_hex(bytes);
    if actual != file_ref.sha256 {
        return Err(IntegrityError::HashMismatch {
            expected: file_ref.sha256.clone(),
            actual,
        });
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
    verify(&version.body.file, bytes).map_err(|cause| Discarded {
        id: version.id.clone(),
        cause,
    })
}

/// Why a file is not a well-formed package (version rule 3).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackageError {
    #[error("not a ZIP archive: no end-of-central-directory record (truncated?)")]
    NoEndRecord,
    #[error("ZIP central directory is damaged: {0}")]
    BadDirectory(&'static str),
    #[error("ZIP64 archives are not checked")]
    Zip64,
    #[error("the package has no `{0}` entry")]
    MissingEntry(&'static str),
}

const ODF_PREFIX: &str = "application/vnd.oasis.opendocument.";
const OOXML_PREFIX: &str = "application/vnd.openxmlformats-officedocument.";

fn le16(b: &[u8], at: usize) -> usize {
    u16::from_le_bytes([b[at], b[at + 1]]) as usize
}

fn le32(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
}

/// Version rule 3: a writer MUST NOT publish a file it has not opened with the
/// format's own reader; for ODF and OOXML that means a ZIP archive whose
/// package structure parses. This walks the central directory (every entry
/// header, every local-header offset) and requires the entry that names the
/// format (`mimetype` for ODF, `[Content_Types].xml` for OOXML). Other MIME
/// types are not checked: DCP has no reader for them.
pub fn check_package(mime: &str, bytes: &[u8]) -> Result<(), PackageError> {
    let required = if mime.starts_with(ODF_PREFIX) {
        "mimetype"
    } else if mime.starts_with(OOXML_PREFIX) {
        "[Content_Types].xml"
    } else {
        return Ok(());
    };

    const EOCD: usize = 22;
    if bytes.len() < EOCD {
        return Err(PackageError::NoEndRecord);
    }
    let lowest = bytes.len().saturating_sub(EOCD + 0xFFFF);
    let eocd = (lowest..=bytes.len() - EOCD)
        .rev()
        .find(|&p| {
            bytes[p..p + 4] == *b"PK\x05\x06" && p + EOCD + le16(bytes, p + 20) == bytes.len()
        })
        .ok_or(PackageError::NoEndRecord)?;
    let entries = le16(bytes, eocd + 10);
    let (dir_size, dir_at) = (le32(bytes, eocd + 12), le32(bytes, eocd + 16));
    if entries == 0xFFFF || dir_size == 0xFFFF_FFFF || dir_at == 0xFFFF_FFFF {
        return Err(PackageError::Zip64);
    }
    if dir_at.checked_add(dir_size).is_none_or(|end| end > eocd) {
        return Err(PackageError::BadDirectory(
            "directory lies outside the file",
        ));
    }
    let mut at = dir_at;
    let mut found = false;
    for _ in 0..entries {
        if at + 46 > dir_at + dir_size || bytes[at..at + 4] != *b"PK\x01\x02" {
            return Err(PackageError::BadDirectory("entry header missing"));
        }
        let (name_len, extra_len, comment_len) = (
            le16(bytes, at + 28),
            le16(bytes, at + 30),
            le16(bytes, at + 32),
        );
        let local = le32(bytes, at + 42);
        let next = at + 46 + name_len + extra_len + comment_len;
        if next > dir_at + dir_size {
            return Err(PackageError::BadDirectory("entry runs past the directory"));
        }
        if local + 4 > dir_at || bytes[local..local + 4] != *b"PK\x03\x04" {
            return Err(PackageError::BadDirectory("entry data missing"));
        }
        found |= &bytes[at + 46..at + 46 + name_len] == required.as_bytes();
        at = next;
    }
    if at != dir_at + dir_size {
        return Err(PackageError::BadDirectory(
            "directory size does not match its entries",
        ));
    }
    if !found {
        return Err(PackageError::MissingEntry(required));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn url() -> Locator {
        Locator::Url {
            href: "https://example.org/f".into(),
        }
    }

    #[test]
    fn sha256_of_known_input() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
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
        assert_eq!(
            verify(&f, b"hello"),
            Err(IntegrityError::SizeMismatch {
                expected: 11,
                actual: 5
            })
        );
    }

    #[test]
    fn same_size_corruption_is_a_hash_mismatch() {
        let f = file_ref("text/plain", b"hello", url());
        assert!(matches!(
            verify(&f, b"hellp"),
            Err(IntegrityError::HashMismatch { .. })
        ));
    }

    #[test]
    fn appended_bytes_and_empty_files_are_rejected() {
        let f = file_ref("text/plain", b"hello", url());
        assert!(matches!(
            verify(&f, b"hello!"),
            Err(IntegrityError::SizeMismatch { .. })
        ));
        assert!(matches!(
            verify(&f, b""),
            Err(IntegrityError::SizeMismatch { .. })
        ));
        // An empty file is a legitimate file with its own hash.
        let e = file_ref("text/plain", b"", url());
        verify(&e, b"").unwrap();
        assert!(verify(&e, b"x").is_err());
    }

    #[test]
    fn right_size_and_wrong_recorded_hash_is_rejected() {
        let mut f = file_ref("text/plain", b"hello", url());
        f.sha256 = sha256_hex(b"other");
        assert!(matches!(
            verify(&f, b"hello"),
            Err(IntegrityError::HashMismatch { .. })
        ));
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
                dcp: 1,
                parents: vec![],
                file: file_ref("text/plain", b"abc", url()),
                created_by: Author {
                    principal: "@a:x".into(),
                    device: "D".into(),
                },
                message: None,
                covers: None,
            },
        );
        verify_version(&v, b"abc").unwrap();
        let d = verify_version(&v, b"abd").unwrap_err();
        assert_eq!(d.id, "$v2");
        assert!(d.to_string().contains("$v2"));
    }

    /// A ZIP with stored (uncompressed) entries; the CRC is not checked by
    /// `check_package`, so it is left zero.
    pub(crate) fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut dir = Vec::new();
        for (name, data) in entries {
            let offset = out.len() as u32;
            out.extend(b"PK\x03\x04");
            out.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // version..crc
            out.extend((data.len() as u32).to_le_bytes()); // compressed size
            out.extend((data.len() as u32).to_le_bytes()); // size
            out.extend((name.len() as u16).to_le_bytes());
            out.extend([0, 0]);
            out.extend(name.as_bytes());
            out.extend(*data);
            dir.extend(b"PK\x01\x02");
            dir.extend([20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // versions..crc
            dir.extend((data.len() as u32).to_le_bytes());
            dir.extend((data.len() as u32).to_le_bytes());
            dir.extend((name.len() as u16).to_le_bytes());
            dir.extend([0u8; 12]); // extra, comment, disk, attrs(2+4)
            dir.extend(offset.to_le_bytes());
            dir.extend(name.as_bytes());
        }
        let dir_at = out.len() as u32;
        let dir_len = dir.len() as u32;
        out.extend(dir);
        out.extend(b"PK\x05\x06");
        out.extend([0, 0, 0, 0]);
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend(dir_len.to_le_bytes());
        out.extend(dir_at.to_le_bytes());
        out.extend([0, 0]);
        out
    }

    const ODT: &str = "application/vnd.oasis.opendocument.text";
    const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

    #[test]
    fn a_well_formed_odf_or_ooxml_package_passes_version_rule_3() {
        let odt = zip(&[
            ("mimetype", b"application/vnd.oasis.opendocument.text"),
            ("content.xml", b"<x/>"),
        ]);
        check_package(ODT, &odt).unwrap();
        let docx = zip(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("word/document.xml", b"<w/>"),
        ]);
        check_package(DOCX, &docx).unwrap();
    }

    #[test]
    fn a_truncated_package_is_refused() {
        let odt = zip(&[
            ("mimetype", b"application/vnd.oasis.opendocument.text"),
            ("content.xml", b"<x/>"),
        ]);
        for cut in [0, 10, odt.len() / 2, odt.len() - 1] {
            assert!(
                check_package(ODT, &odt[..cut]).is_err(),
                "cut at {cut} was accepted"
            );
        }
    }

    #[test]
    fn a_damaged_directory_or_wrong_package_is_refused() {
        let odt = zip(&[("mimetype", b"x"), ("content.xml", b"<x/>")]);
        assert_eq!(
            check_package(DOCX, &odt),
            Err(PackageError::MissingEntry("[Content_Types].xml"))
        );
        assert!(matches!(
            check_package(ODT, b"this is plain text, not a zip at all....."),
            Err(PackageError::NoEndRecord)
        ));
        let mut bad = odt.clone();
        let dir_at = bad.len() - 22 - 2 * 46 - "mimetype".len() - "content.xml".len();
        bad[dir_at] = b'X'; // first directory entry's signature
        assert!(matches!(
            check_package(ODT, &bad),
            Err(PackageError::BadDirectory(_))
        ));
        let no_mimetype = zip(&[("content.xml", b"<x/>")]);
        assert_eq!(
            check_package(ODT, &no_mimetype),
            Err(PackageError::MissingEntry("mimetype"))
        );
    }

    #[test]
    fn other_mime_types_are_not_checked() {
        check_package("text/plain", b"anything").unwrap();
        check_package("application/octet-stream", b"").unwrap();
    }
}
