//! Dropbox: the HTTP API v2, signed in with OAuth (PKCE); files over the
//! single-request limit go up in an upload session.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::http::{read_chunk, short_error, status_error, Wording};
use super::{expiry, local_file, share_password, CloudAccount, ShareResult};

const DROPBOX_API: &str = "https://api.dropboxapi.com/2";
const DROPBOX_CONTENT: &str = "https://content.dropboxapi.com/2";
/// Above this a file goes up in an upload session (the single-request
/// limit is 150 MB).
const DROPBOX_SINGLE_LIMIT: u64 = 150 * 1024 * 1024;
/// Session chunks: a multiple of 4 MiB, well under the 150 MB cap.
const DROPBOX_CHUNK: usize = 32 * 1024 * 1024;

/// The app key the sign-in runs as: the account's own, else the build's.
pub(crate) fn dropbox_client_id(account: &CloudAccount) -> String {
    let own = account.client_id.trim();
    if !own.is_empty() {
        return own.to_string();
    }
    crate::services::oauth::provider_credentials("dropbox").0
}

fn dropbox_settings(account: &CloudAccount) -> crate::config::OAuthSettings {
    crate::config::OAuthSettings {
        auth_url: "https://www.dropbox.com/oauth2/authorize".to_string(),
        token_url: "https://api.dropboxapi.com/oauth2/token".to_string(),
        client_id: dropbox_client_id(account),
        client_secret: String::new(),
        scopes: "account_info.read files.content.write sharing.write".to_string(),
        redirect_uri: String::new(),
    }
}

/// Sign in to Dropbox in the browser (blocking; call off the UI thread).
/// Answers with the refresh token to keep, the account's e-mail (its
/// login, for the keyring key) and its display name.
pub(crate) fn dropbox_connect(account: &CloudAccount) -> Result<(String, String, String), String> {
    let settings = dropbox_settings(account);
    if settings.client_id.is_empty() {
        return Err("Enter the app key of a Dropbox app first.".to_string());
    }
    let flow = crate::services::oauth::run_flow(&settings)
        .map_err(|e| format!("Dropbox sign-in failed: {e}"))?;
    let access = crate::services::oauth::refresh_access_token(&settings, &flow.refresh_token)
        .map_err(|e| format!("Dropbox sign-in failed: {e}"))?
        .access_token;
    let (email, name) = dropbox_whoami(&access)?;
    Ok((flow.refresh_token, email, name))
}

/// Access tokens last four hours; keep the ones minted, by refresh token.
static DROPBOX_ACCESS: Mutex<Option<HashMap<String, (String, Instant)>>> = Mutex::new(None);

pub(super) fn dropbox_access(account: &CloudAccount, refresh: &str) -> Result<String, String> {
    if let Ok(mut g) = DROPBOX_ACCESS.lock() {
        if let Some((tok, made)) = g.get_or_insert_with(HashMap::new).get(refresh) {
            if made.elapsed() < Duration::from_secs(3 * 3600) {
                return Ok(tok.clone());
            }
        }
    }
    let settings = dropbox_settings(account);
    if settings.client_id.is_empty() {
        return Err(
            "The Dropbox app key is missing: open the account's settings and enter it.".to_string(),
        );
    }
    let tok = crate::services::oauth::refresh_access_token(&settings, refresh)
        .map_err(|e| format!("Dropbox refused the sign-in ({e}). Open Settings, Cloud Storage, and connect the account again."))?
        .access_token;
    if let Ok(mut g) = DROPBOX_ACCESS.lock() {
        g.get_or_insert_with(HashMap::new)
            .insert(refresh.to_string(), (tok.clone(), Instant::now()));
    }
    Ok(tok)
}

/// JSON with every non-ASCII character escaped, as the `Dropbox-API-Arg`
/// header wants it.
fn dropbox_arg(v: &serde_json::Value) -> String {
    let mut out = String::new();
    for c in v.to_string().chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for u in c.encode_utf16(&mut buf) {
                out.push_str(&format!("\\u{:04x}", u));
            }
        }
    }
    out
}

const WORDING: Wording = Wording {
    refused: &[401],
    refused_text:
        "Dropbox refused the sign-in. Open Settings, Cloud Storage, and connect the account again.",
    not_found: false,
    message: short_error,
    code_first: false,
};

pub(super) fn dropbox_err(what: &str, e: ureq::Error) -> String {
    status_error(what, e, &WORDING)
}

/// One RPC-style call: JSON in, JSON out.
// ureq 2's Error is what callers match on (`status_error`); one per
// request, so its size costs nothing worth boxing it for.
#[allow(clippy::result_large_err)]
fn dropbox_rpc(
    access: &str,
    endpoint: &str,
    arg: &serde_json::Value,
) -> Result<serde_json::Value, ureq::Error> {
    ureq::post(&format!("{DROPBOX_API}/{endpoint}"))
        .set("Authorization", &format!("Bearer {access}"))
        .timeout(Duration::from_secs(60))
        .send_json(arg)?
        .into_json()
        .map_err(ureq::Error::from)
}

/// The signed-in account: (e-mail, display name).
pub(super) fn dropbox_whoami(access: &str) -> Result<(String, String), String> {
    let v = dropbox_rpc(
        access,
        "users/get_current_account",
        &serde_json::Value::Null,
    )
    .map_err(|e| dropbox_err("Could not sign in", e))?;
    let email = v["email"].as_str().unwrap_or("").to_string();
    let name = v["name"]["display_name"].as_str().unwrap_or("").to_string();
    if email.is_empty() {
        return Err("Dropbox answered, but without an account.".to_string());
    }
    let name = if name.is_empty() { email.clone() } else { name };
    Ok((email, name))
}

/// One content call: the argument in the header, bytes in the body.
// ureq 2's Error is what callers match on (`status_error`); one per
// request, so its size costs nothing worth boxing it for.
#[allow(clippy::result_large_err)]
fn dropbox_content(
    access: &str,
    endpoint: &str,
    arg: &serde_json::Value,
    body: impl Read,
    len: u64,
) -> Result<serde_json::Value, ureq::Error> {
    ureq::post(&format!("{DROPBOX_CONTENT}/{endpoint}"))
        .set("Authorization", &format!("Bearer {access}"))
        .set("Dropbox-API-Arg", &dropbox_arg(arg))
        .set("Content-Type", "application/octet-stream")
        .set("Content-Length", &len.to_string())
        .timeout(Duration::from_secs(60 * 60))
        .send(body)?
        .into_json()
        .map_err(ureq::Error::from)
}

pub(super) fn dropbox_upload_and_share(
    account: &CloudAccount,
    refresh: &str,
    path: &Path,
    fixed: Option<&str>,
) -> Result<ShareResult, String> {
    let (name, size) = local_file(path)?;
    let access = dropbox_access(account, refresh)?;
    let remote_path = format!("/{}/{}", account.folder_clean(), name);
    // Missing folders are made on the way; a taken name is renamed by
    // Dropbox ("report (1).pdf") and the answer says what it became.
    let commit = serde_json::json!({
        "path": remote_path,
        "mode": "add",
        "autorename": true,
        "mute": true,
    });
    let file = std::fs::File::open(path).map_err(|e| format!("could not read {name}: {e}"))?;

    let meta = if size <= DROPBOX_SINGLE_LIMIT {
        dropbox_content(&access, "files/upload", &commit, file, size)
            .map_err(|e| dropbox_err("Upload failed", e))?
    } else {
        dropbox_upload_session(&access, file, &name, size, &commit)?
    };
    let remote = meta["name"].as_str().unwrap_or(&name).to_string();
    let lower = meta["path_lower"]
        .as_str()
        .unwrap_or(&remote_path)
        .to_string();

    let expires = expiry(account);
    let pw = share_password(account, fixed)?;
    let url = dropbox_share_link(&access, &lower, pw.as_deref(), expires.as_deref())?;
    Ok(ShareResult {
        name: remote,
        size,
        url,
        password: pw,
        expires,
    })
}

/// An upload session: start with the first chunk, append the middle ones,
/// finish with the last and the commit. The file is over the
/// single-request limit, so the first chunk is never the last. Answers
/// with the committed file's metadata.
fn dropbox_upload_session(
    access: &str,
    mut file: std::fs::File,
    name: &str,
    size: u64,
    commit: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut buf = vec![0u8; DROPBOX_CHUNK];
    let mut offset: u64 = 0;
    let mut session = String::new();
    loop {
        let n = read_chunk(&mut file, &mut buf, name)?;
        if n == 0 {
            return Err("Upload failed: the file ended early.".to_string());
        }
        let last = offset + n as u64 >= size;
        let chunk = &buf[..n];
        if session.is_empty() {
            let v = dropbox_content(
                access,
                "files/upload_session/start",
                &serde_json::json!({"close": false}),
                chunk,
                n as u64,
            )
            .map_err(|e| dropbox_err("Upload failed", e))?;
            session = v["session_id"].as_str().unwrap_or("").to_string();
            if session.is_empty() {
                return Err("Upload failed: Dropbox opened no upload session.".to_string());
            }
        } else if last {
            let arg = serde_json::json!({
                "cursor": {"session_id": session, "offset": offset},
                "commit": commit,
            });
            return dropbox_content(access, "files/upload_session/finish", &arg, chunk, n as u64)
                .map_err(|e| dropbox_err("Upload failed", e));
        } else {
            let arg = serde_json::json!({
                "cursor": {"session_id": session, "offset": offset},
                "close": false,
            });
            dropbox_content(
                access,
                "files/upload_session/append_v2",
                &arg,
                chunk,
                n as u64,
            )
            .map_err(|e| dropbox_err("Upload failed", e))?;
        }
        offset += n as u64;
    }
}

/// The public link to the uploaded file at `lower` (its lower-cased path).
/// Passwords and expiry are settings only paid plans may set; Dropbox says
/// so with a settings_error.
fn dropbox_share_link(
    access: &str,
    lower: &str,
    password: Option<&str>,
    expires: Option<&str>,
) -> Result<String, String> {
    let mut settings = serde_json::json!({"audience": "public", "access": "viewer"});
    if let Some(p) = password {
        settings["requested_visibility"] = "password".into();
        settings["link_password"] = p.into();
    }
    if let Some(d) = expires {
        settings["expires"] = format!("{d}T00:00:00Z").into();
    }
    let arg = serde_json::json!({"path": lower, "settings": settings});
    let url = match dropbox_rpc(access, "sharing/create_shared_link_with_settings", &arg) {
        Ok(v) => v["url"].as_str().unwrap_or("").to_string(),
        Err(ureq::Error::Status(409, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            if text.contains("shared_link_already_exists") {
                let v = dropbox_rpc(
                    access,
                    "sharing/list_shared_links",
                    &serde_json::json!({"path": lower, "direct_only": true}),
                )
                .map_err(|e| dropbox_err("Uploaded, but could not find the share link", e))?;
                v["links"][0]["url"].as_str().unwrap_or("").to_string()
            } else if text.contains("settings_error") {
                return Err("Uploaded, but Dropbox would not make the link: link passwords and expiry dates need a paid Dropbox plan. Turn them off in the account's settings.".to_string());
            } else {
                return Err(format!(
                    "Uploaded, but could not create the share link: {}",
                    short_error(&text)
                ));
            }
        }
        Err(e) => {
            return Err(dropbox_err(
                "Uploaded, but could not create the share link",
                e,
            ))
        }
    };
    if url.is_empty() {
        return Err("Uploaded, but Dropbox made no share link.".to_string());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropbox_header_arg_is_ascii() {
        let arg = dropbox_arg(&serde_json::json!({"path": "/Hylki/caf\u{e9} \u{1F4CE}.pdf"}));
        assert!(arg.is_ascii());
        assert!(arg.contains("caf\\u00e9"));
        assert!(arg.contains("\\ud83d\\udcce"));
    }
}
