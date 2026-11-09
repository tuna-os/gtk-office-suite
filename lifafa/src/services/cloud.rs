//! Cloud attachments (#144): upload a file to cloud storage and share it
//! by public link, so a large file travels as a link in the message
//! instead of an attachment. Three kinds of account:
//!
//! * **Nextcloud, ownCloud, OpenCloud**: WebDAV under
//!   `remote.php/dav/files/<user>/` for the upload and the OCS
//!   files-sharing endpoint for the public link, signed in with an app
//!   password.
//! * **Dropbox**: the HTTP API v2, signed in with OAuth (PKCE, so only an
//!   app key is needed; the refresh token is what the keyring holds).
//!   Files over the single-request limit go up in an upload session.
//! * **Seafile**: the web API, signed in with the account password (turned
//!   into an API token on the spot) or a pasted API token. Uploads go
//!   into a library, made when missing.
//! * **OneDrive**: through a GNOME Online Accounts Microsoft 365 account,
//!   whose token GOA hands out and refreshes; nothing of ours in the
//!   keyring. Microsoft Graph: simple or session upload, `createLink`.
//!
//! Accounts live in `cloud.toml` beside the other settings; each secret is
//! in the system keyring under the account's [`CloudAccount::key`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

mod account;
mod dropbox;
mod http;
mod nextcloud;
mod onedrive;
mod seafile;

pub(crate) use account::{CloudAccount, CloudKind, LinkTerms, Service, SERVICES};
pub(crate) use dropbox::{dropbox_client_id, dropbox_connect};
pub(crate) use nextcloud::detect_product;
pub(crate) use seafile::seafile_login_with_code;

use dropbox::{dropbox_access, dropbox_upload_and_share, dropbox_whoami};
use nextcloud::{nextcloud_upload_and_share, nextcloud_verify};
use onedrive::{goa_token, onedrive_upload_and_share, onedrive_whoami};
use seafile::{seafile_token, seafile_upload_and_share, seafile_whoami};

#[derive(Default, Serialize, Deserialize)]
struct CloudFile {
    #[serde(default)]
    accounts: Vec<CloudAccount>,
}

fn path() -> Option<PathBuf> {
    crate::config::config_file("cloud.toml")
}

pub(crate) fn load_accounts() -> Vec<CloudAccount> {
    crate::config::read_toml::<CloudFile>(path())
        .map(|f| f.accounts)
        .unwrap_or_default()
}

/// The accounts the composer offers: those switched on.
pub(crate) fn load_enabled_accounts() -> Vec<CloudAccount> {
    load_accounts().into_iter().filter(|a| a.enabled).collect()
}

pub(crate) fn save_accounts(accounts: &[CloudAccount]) {
    let file = CloudFile {
        accounts: accounts.to_vec(),
    };
    crate::config::save_private_toml(path(), &file, "cloud accounts", false);
}

/// What an upload produced: the public link and what it was made with.
#[derive(Clone, Debug)]
pub(crate) struct ShareResult {
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) url: String,
    /// The download password, when the account protects links.
    pub(crate) password: Option<String>,
    /// The expiry date (YYYY-MM-DD), when the account sets one.
    pub(crate) expires: Option<String>,
}

/// The upload's name and size, checked before anything goes over the wire.
fn local_file(path: &Path) -> Result<(String, u64), String> {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "the file has no name".to_string())?;
    let meta = std::fs::metadata(path).map_err(|e| format!("could not read {name}: {e}"))?;
    Ok((name, meta.len()))
}

/// The expiry date the account asks for, YYYY-MM-DD.
fn expiry(account: &CloudAccount) -> Option<String> {
    (account.expire_days > 0).then(|| {
        (chrono::Local::now() + chrono::Duration::days(account.expire_days as i64))
            .format("%Y-%m-%d")
            .to_string()
    })
}

/// Check the account signs in; answers with the display name.
pub(crate) fn verify(account: &CloudAccount, secret: &str) -> Result<String, String> {
    match account.kind {
        CloudKind::Nextcloud => nextcloud_verify(account, secret),
        CloudKind::Dropbox => {
            let access = dropbox_access(account, secret)?;
            dropbox_whoami(&access).map(|(_, name)| name)
        }
        CloudKind::Seafile => {
            let token = seafile_token(account, secret)?;
            seafile_whoami(account, &token)
        }
        CloudKind::OneDrive => {
            let token = goa_token(account)?;
            onedrive_whoami(&token)
        }
    }
}

/// Find out what the service lets a link carry for this account: only
/// OneDrive differs by plan, so the others answer `None` (everything).
/// A free personal OneDrive takes neither an expiry nor a password, a
/// Microsoft 365 personal one takes both, and OneDrive for Business takes
/// an expiry but no link password. Microsoft does not say which plan a
/// drive is on; a personal drive's quota tells a free one (5 GB) from a
/// subscription (100 GB and up).
pub(crate) fn probe_link_terms(
    account: &CloudAccount,
    _secret: &str,
) -> Result<Option<LinkTerms>, String> {
    if account.kind != CloudKind::OneDrive {
        return Ok(None);
    }
    onedrive::link_terms(account).map(Some)
}

/// Upload `path` into the account's folder and share it by public link.
/// A file already there by that name is left alone: the upload takes
/// another name instead. The account's `expire_days` and `password` say
/// how the link is made (the composer hands in a copy with the user's
/// choices for this upload); `fixed_password` is the password to use
/// when one is wanted, else one is generated per file.
pub(crate) fn upload_and_share(
    account: &CloudAccount,
    secret: &str,
    path: &Path,
    fixed_password: Option<&str>,
) -> Result<ShareResult, String> {
    match account.kind {
        CloudKind::Nextcloud => nextcloud_upload_and_share(account, secret, path, fixed_password),
        CloudKind::Dropbox => dropbox_upload_and_share(account, secret, path, fixed_password),
        CloudKind::Seafile => seafile_upload_and_share(account, secret, path, fixed_password),
        CloudKind::OneDrive => onedrive_upload_and_share(account, path, fixed_password),
    }
}

/// The link's download password: none unless the account (as handed in)
/// wants one; then the fixed one when given, else a fresh one.
fn share_password(account: &CloudAccount, fixed: Option<&str>) -> Result<Option<String>, String> {
    if !account.password {
        return Ok(None);
    }
    match fixed.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => Ok(Some(p.to_string())),
        None => generate_password().map(Some),
    }
}

/// A download password people can read out: letters and digits, no
/// look-alikes, twelve long.
fn generate_password() -> Result<String, String> {
    crate::rng::from_alphabet(
        12,
        b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789",
    )
    .map_err(|e| format!("Could not make a link password: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cloud::onedrive::onedrive_item;

    #[test]
    fn sizes_read_well() {
        // GLib's, as Files writes sizes: decimal units, a no-break space.
        assert_eq!(crate::models::human_size(2_300_000), "2.3\u{a0}MB");
    }

    #[test]
    fn passwords_are_readable_and_long_enough() {
        let p = generate_password().unwrap();
        assert_eq!(p.len(), 12);
        assert!(p.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn old_cloud_toml_still_reads_as_nextcloud() {
        let text =
            "[[accounts]]\nname = \"Work\"\nurl = \"https://cloud.example.com\"\nuser = \"me\"\n";
        let f: CloudFile = toml::from_str(text).unwrap();
        assert_eq!(f.accounts[0].kind, CloudKind::Nextcloud);
        assert_eq!(f.accounts[0].key(), "cloud:https://cloud.example.com|me");
        let mut d = CloudAccount::empty();
        d.kind = CloudKind::Dropbox;
        d.user = "me@example.com".into();
        assert_eq!(d.key(), "cloud:dropbox|me@example.com");
        let back = toml::to_string(&CloudFile { accounts: vec![d] }).unwrap();
        assert!(back.contains("kind = \"dropbox\""));
    }

    #[test]
    fn goa_kinds_have_no_secret_and_their_own_key() {
        let mut a = CloudAccount::empty();
        a.kind = CloudKind::OneDrive;
        a.goa_id = "account_123".into();
        assert!(!a.has_secret());
        assert_eq!(a.key(), "cloud:goa|account_123");
        let back = toml::to_string(&CloudFile { accounts: vec![a] }).unwrap();
        assert!(back.contains("kind = \"onedrive\""));
        assert_eq!(
            onedrive_item("Hylki/Q3 report"),
            "https://graph.microsoft.com/v1.0/me/drive/root:/Hylki/Q3%20report"
        );
        assert_eq!(
            onedrive_item(""),
            "https://graph.microsoft.com/v1.0/me/drive/root"
        );
    }

    #[test]
    fn the_link_password_follows_the_choice() {
        let mut a = CloudAccount::empty();
        assert_eq!(share_password(&a, Some("typed")), Ok(None));
        a.password = true;
        assert_eq!(
            share_password(&a, Some(" typed ")).unwrap().as_deref(),
            Some("typed")
        );
        assert_eq!(
            share_password(&a, Some("  ")).unwrap().map(|p| p.len()),
            Some(12)
        );
        assert_eq!(share_password(&a, None).unwrap().map(|p| p.len()), Some(12));
    }
}
