//! The Streams signing key: the only thing this process stores.
//!
//! # What is stored, exactly
//!
//! One file, `<data dir>/streams-identity`: the 32-byte Ed25519 seed as 64
//! lower-case hex digits and a newline (65 bytes). The principal other people
//! put on a roster, `ed25519:<base64url public key>`, is derived from it; the
//! public key is not stored. Nothing else is written to disk: no document key
//! (epoch keys are unwrapped from the roster with this seed and held in memory),
//! no access token, no password, no server credential, no document content, no
//! device id (a new one is drawn per process, so a restarted process is a new
//! stream of the same principal and cannot repeat a `seq`).
//!
//! The data dir is created `0700` and the file `0600`. An existing file that is
//! group- or world-accessible, or that is not what is described above, is
//! **refused with an error**, never repaired and never overwritten: a key that
//! may have been read by someone else is not a key to sign with, and a file that
//! is not a key may be something the user needs.
//!
//! # Why a file, and what replaces it
//!
//! RFC-0005 section 4.2 puts secrets in the Secret portal (`oo7`), inside this
//! process, so that `zbus` and its tree stay out of the product lockfile. That
//! comes with the Matrix binding, which has an access token and a store key to
//! protect as well, and needs the same dependency; it is one backend choice in this
//! module and no other code changes. Until then the key lives in the file above:
//! the same protection as an SSH private key, no better, and it is documented as
//! that rather than as a keyring.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use dcp_streams::crypto::random;
use dcp_streams::Identity;

pub const IDENTITY_FILE: &str = "streams-identity";

#[derive(Debug)]
pub enum IdentityError {
    Io(PathBuf, io::Error),
    /// The file exists and is not a seed, or is readable by others.
    Refused(PathBuf, String),
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdentityError::Io(p, e) => write!(f, "{}: {e}", p.display()),
            IdentityError::Refused(p, why) => write!(f, "{}: {why}", p.display()),
        }
    }
}

impl std::error::Error for IdentityError {}

fn parse(path: &Path, text: &str) -> Result<[u8; 32], IdentityError> {
    let hex_digits = text.strip_suffix('\n').unwrap_or(text);
    if hex_digits.len() != 64
        || !hex_digits
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(IdentityError::Refused(
            path.to_owned(),
            "not an identity seed (expected 64 lower-case hex digits); not touching it".into(),
        ));
    }
    let mut seed = [0u8; 32];
    hex::decode_to_slice(hex_digits, &mut seed).map_err(|e| {
        IdentityError::Refused(path.to_owned(), format!("not an identity seed: {e}"))
    })?;
    Ok(seed)
}

#[cfg(unix)]
fn check_private(path: &Path) -> Result<(), IdentityError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path)
        .map_err(|e| IdentityError::Io(path.to_owned(), e))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(IdentityError::Refused(
            path.to_owned(),
            format!("the key file is accessible to others (mode {:o}); refusing to sign with it. Check who may have read it, then `chmod 600` it or delete it for a new identity", mode & 0o777),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_private(_: &Path) -> Result<(), IdentityError> {
    Ok(())
}

fn create_dir(dir: &Path) -> Result<(), IdentityError> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(dir)
        .map_err(|e| IdentityError::Io(dir.to_owned(), e))
}

fn write_new(tmp: &Path, text: &str) -> io::Result<()> {
    let mut o = fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(tmp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()
}

/// The identity stored in `dir`, made first if there is none. Two processes
/// starting at once agree on one key: the new file is written to a temporary name
/// and hard-linked into place, which fails if the name is taken.
pub fn load_or_create(dir: &Path) -> Result<Identity, IdentityError> {
    let path = dir.join(IDENTITY_FILE);
    if !path.exists() {
        create_dir(dir)?;
        let seed = random::<32>();
        let text = format!("{}\n", hex::encode(seed));
        let tmp = dir.join(format!(".{IDENTITY_FILE}.{}.tmp", hex::encode(random::<6>())));
        write_new(&tmp, &text).map_err(|e| IdentityError::Io(tmp.clone(), e))?;
        let linked = fs::hard_link(&tmp, &path);
        let _ = fs::remove_file(&tmp);
        match linked {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(IdentityError::Io(path, e)),
        }
    }
    check_private(&path)?;
    let text = fs::read_to_string(&path).map_err(|e| IdentityError::Io(path.clone(), e))?;
    Ok(Identity::from_seed(parse(&path, &text)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("collabd-id-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_first_call_makes_a_key_and_the_second_finds_it() {
        let dir = scratch("make").join("nested");
        let a = load_or_create(&dir).unwrap();
        let b = load_or_create(&dir).unwrap();
        assert_eq!(a.principal(), b.principal());
        let text = fs::read_to_string(dir.join(IDENTITY_FILE)).unwrap();
        assert_eq!(text.len(), 65, "64 hex digits and a newline, nothing else");
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [IDENTITY_FILE], "no temporary file is left behind");
        let _ = fs::remove_dir_all(dir.parent().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn the_directory_and_the_file_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("modes");
        load_or_create(&dir).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join(IDENTITY_FILE)), 0o600);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_key_file_others_can_read_is_refused_not_repaired() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("loose");
        load_or_create(&dir).unwrap();
        let file = dir.join(IDENTITY_FILE);
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        let before = fs::read(&file).unwrap();
        let err = load_or_create(&dir).unwrap_err();
        assert!(matches!(err, IdentityError::Refused(..)), "{err}");
        assert_eq!(fs::read(&file).unwrap(), before, "the file is untouched");
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o644,
            "and the mode is not changed behind the user's back"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_that_is_not_a_seed_is_refused_and_kept() {
        let dir = scratch("junk");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join(IDENTITY_FILE);
        let mut o = fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        o.open(&file).unwrap().write_all(b"my notes, not a key\n").unwrap();
        let err = load_or_create(&dir).unwrap_err();
        assert!(matches!(err, IdentityError::Refused(..)), "{err}");
        assert_eq!(fs::read(&file).unwrap(), b"my notes, not a key\n");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn two_directories_are_two_identities() {
        let (a, b) = (scratch("two-a"), scratch("two-b"));
        assert_ne!(
            load_or_create(&a).unwrap().principal(),
            load_or_create(&b).unwrap().principal()
        );
        let _ = fs::remove_dir_all(a);
        let _ = fs::remove_dir_all(b);
    }
}
