//! OneDrive through a GNOME Online Accounts Microsoft 365 account: GOA
//! hands out the token, Microsoft Graph takes the upload and makes the
//! link.

use std::path::Path;
use std::time::Duration;

use crate::services::graph_api::{bearer, GRAPH_BASE};

use super::http::{read_chunk, short_error, status_error, Wording};
use super::{expiry, local_file, share_password, CloudAccount, LinkTerms, ShareResult};

/// What the account's OneDrive plan lets a link carry (see
/// [`super::probe_link_terms`]).
pub(super) fn link_terms(account: &CloudAccount) -> Result<LinkTerms, String> {
    let token = goa_token(account)?;
    let v = graph_json(
        ureq::get(&format!("{GRAPH_BASE}/me/drive?$select=driveType,quota"))
            .set("Authorization", &bearer(&token))
            .timeout(Duration::from_secs(60)),
        None,
    )
    .map_err(|e| api_err("Could not look at the OneDrive plan", e))?;
    let drive_type = v["driveType"].as_str().unwrap_or("");
    let total = v["quota"]["total"].as_u64().unwrap_or(0);
    Ok(onedrive_terms(drive_type, total))
}

fn onedrive_terms(drive_type: &str, quota_total: u64) -> LinkTerms {
    const FREE_CEILING: u64 = 16 * 1024 * 1024 * 1024;
    match drive_type {
        "personal" if quota_total > 0 && quota_total <= FREE_CEILING => LinkTerms {
            expiry: false,
            password: false,
            note: "Not available on a free personal OneDrive; needs a Microsoft 365 subscription"
                .to_string(),
        },
        "personal" => LinkTerms {
            expiry: true,
            password: true,
            note: String::new(),
        },
        _ => LinkTerms {
            expiry: true,
            password: false,
            note: "OneDrive for Business links take no password".to_string(),
        },
    }
}

/// A fresh access token for the account's GOA account.
pub(super) fn goa_token(account: &CloudAccount) -> Result<String, String> {
    let id = account.goa_id.trim();
    if id.is_empty() {
        return Err("The account is not linked to a GNOME Online Accounts account: open its settings and choose one.".to_string());
    }
    crate::services::goa::oauth_token(id).ok_or_else(|| {
        "GNOME Online Accounts gave no token for the account. Check it under Settings, Online Accounts; it may need signing in again.".to_string()
    })
}

/// Microsoft Graph: the message inside its JSON error, with the status.
const WORDING: Wording = Wording {
    refused: &[401],
    refused_text:
        "the service refused the sign-in. Check the account under Settings, Online Accounts.",
    not_found: false,
    message: api_message,
    code_first: false,
};

/// An error from a JSON API: the message inside, with the status.
pub(super) fn api_err(what: &str, e: ureq::Error) -> String {
    status_error(what, e, &WORDING)
}

/// The message of a Graph error body (`error.message`), else the plain
/// short form.
pub(super) fn api_message(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(m) = v["error"]["message"].as_str() {
            return m.chars().take(200).collect();
        }
    }
    short_error(body)
}

/// Above this the file goes up in an upload session (the simple upload
/// takes up to 250 MB; stay well under).
const ONEDRIVE_SINGLE_LIMIT: u64 = 60 * 1024 * 1024;
/// Session chunks must be multiples of 320 KiB.
const ONEDRIVE_CHUNK: usize = 32 * 320 * 1024;

// ureq 2's Error is what callers match on (`status_error`); one per
// request, so its size costs nothing worth boxing it for.
#[allow(clippy::result_large_err)]
fn graph_json(
    req: ureq::Request,
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value, ureq::Error> {
    let resp = match body {
        Some(b) => req.send_json(b)?,
        None => req.call()?,
    };
    resp.into_json().map_err(ureq::Error::from)
}

/// `/me/drive/root:/a/b` for a folder path, `/me/drive/root` for none.
pub(super) fn onedrive_item(path: &str) -> String {
    if path.is_empty() {
        format!("{GRAPH_BASE}/me/drive/root")
    } else {
        format!(
            "{GRAPH_BASE}/me/drive/root:/{}",
            crate::encoding::percent::encode_path(path)
        )
    }
}

/// The signed-in Microsoft account's display name.
pub(super) fn onedrive_whoami(token: &str) -> Result<String, String> {
    let v = graph_json(
        ureq::get(&format!("{GRAPH_BASE}/me/drive?$select=id,owner"))
            .set("Authorization", &bearer(token))
            .timeout(Duration::from_secs(60)),
        None,
    )
    .map_err(|e| api_err("Could not reach OneDrive", e))?;
    let name = v["owner"]["user"]["displayName"].as_str().unwrap_or("");
    if v["id"].as_str().unwrap_or("").is_empty() {
        return Err("OneDrive answered, but without a drive.".to_string());
    }
    Ok(if name.is_empty() {
        "OneDrive".to_string()
    } else {
        name.to_string()
    })
}

/// Make sure the upload folder exists, one segment at a time.
fn onedrive_folder(token: &str, folder: &str) -> Result<String, String> {
    let mut dir = String::new();
    for part in folder.split('/').filter(|p| !p.is_empty()) {
        let next = if dir.is_empty() {
            part.to_string()
        } else {
            format!("{dir}/{part}")
        };
        let there = ureq::get(&format!("{}?$select=id", onedrive_item(&next)))
            .set("Authorization", &bearer(token))
            .timeout(Duration::from_secs(60))
            .call();
        match there {
            Ok(_) => {}
            Err(ureq::Error::Status(404, _)) => {
                let url = if dir.is_empty() {
                    format!("{GRAPH_BASE}/me/drive/root/children")
                } else {
                    format!("{}:/children", onedrive_item(&dir))
                };
                graph_json(
                    ureq::post(&url).set("Authorization", &bearer(token)).timeout(Duration::from_secs(60)),
                    Some(&serde_json::json!({"name": part, "folder": {}, "@microsoft.graph.conflictBehavior": "fail"})),
                )
                .map_err(|e| api_err("Could not create the folder", e))?;
            }
            Err(e) => return Err(api_err("Could not look at the folder", e)),
        }
        dir = next;
    }
    Ok(dir)
}

pub(super) fn onedrive_upload_and_share(
    account: &CloudAccount,
    path: &Path,
    fixed: Option<&str>,
) -> Result<ShareResult, String> {
    let (name, size) = local_file(path)?;
    let token = goa_token(account)?;
    let dir = onedrive_folder(&token, &account.folder_clean())?;
    let remote_path = if dir.is_empty() {
        name.clone()
    } else {
        format!("{dir}/{name}")
    };
    let mut file = std::fs::File::open(path).map_err(|e| format!("could not read {name}: {e}"))?;

    // A taken name is renamed by OneDrive ("report 1.pdf"); the answer
    // says what it became.
    let meta = if size <= ONEDRIVE_SINGLE_LIMIT {
        let url = format!(
            "{}:/content?@microsoft.graph.conflictBehavior=rename",
            onedrive_item(&remote_path)
        );
        ureq::put(&url)
            .set("Authorization", &bearer(&token))
            .set("Content-Length", &size.to_string())
            .set("Content-Type", "application/octet-stream")
            .timeout(Duration::from_secs(60 * 60))
            .send(file)
            .map_err(|e| api_err("Upload failed", e))?
            .into_json::<serde_json::Value>()
            .map_err(|e| format!("Upload failed: could not read the answer: {e}"))?
    } else {
        let open = graph_json(
            ureq::post(&format!("{}:/createUploadSession", onedrive_item(&remote_path)))
                .set("Authorization", &bearer(&token))
                .timeout(Duration::from_secs(60)),
            Some(&serde_json::json!({"item": {"@microsoft.graph.conflictBehavior": "rename", "name": name}})),
        )
        .map_err(|e| api_err("Upload failed", e))?;
        let session = open["uploadUrl"].as_str().unwrap_or("").to_string();
        if session.is_empty() {
            return Err("Upload failed: OneDrive opened no upload session.".to_string());
        }
        let mut buf = vec![0u8; ONEDRIVE_CHUNK];
        let mut offset: u64 = 0;
        loop {
            let n = read_chunk(&mut file, &mut buf, &name)?;
            if n == 0 {
                return Err("Upload failed: the file ended early.".to_string());
            }
            let end = offset + n as u64 - 1;
            let resp = ureq::put(&session)
                .set("Content-Length", &n.to_string())
                .set("Content-Range", &format!("bytes {offset}-{end}/{size}"))
                .timeout(Duration::from_secs(60 * 60))
                .send_bytes(&buf[..n])
                .map_err(|e| api_err("Upload failed", e))?;
            offset += n as u64;
            if offset >= size {
                break resp
                    .into_json::<serde_json::Value>()
                    .map_err(|e| format!("Upload failed: could not read the answer: {e}"))?;
            }
        }
    };
    let id = meta["id"].as_str().unwrap_or("").to_string();
    if id.is_empty() {
        return Err("Upload failed: OneDrive gave the file no id.".to_string());
    }
    let remote = meta["name"].as_str().unwrap_or(&name).to_string();

    // The public link. Expiry and passwords take a OneDrive for Business
    // or Microsoft 365 subscription; a personal account says no.
    let expires = expiry(account);
    let pw = share_password(account, fixed)?;
    let mut body = serde_json::json!({"type": "view", "scope": "anonymous"});
    if let Some(d) = &expires {
        body["expirationDateTime"] = format!("{d}T00:00:00Z").into();
    }
    if let Some(p) = &pw {
        body["password"] = p.as_str().into();
    }
    let v = graph_json(
        ureq::post(&format!("{GRAPH_BASE}/me/drive/items/{id}/createLink"))
            .set("Authorization", &bearer(&token))
            .timeout(Duration::from_secs(60)),
        Some(&body),
    )
    .map_err(|e| match e {
        ureq::Error::Status(code, resp) if (expires.is_some() || pw.is_some()) && (code == 400 || code == 403) => {
            let text = resp.into_string().unwrap_or_default();
            format!(
                "Uploaded, but OneDrive would not make the link: {}. Link expiry and download passwords need a Microsoft 365 subscription or OneDrive for Business; on a free personal OneDrive set Links expire after to 0 and turn the password off, under Settings, Cloud Storage.",
                api_message(&text).trim()
            )
        }
        e => api_err("Uploaded, but could not create the share link", e),
    })?;
    let url = v["link"]["webUrl"].as_str().unwrap_or("").to_string();
    if url.is_empty() {
        return Err("Uploaded, but OneDrive made no share link.".to_string());
    }
    Ok(ShareResult {
        name: remote,
        size,
        url,
        password: pw,
        expires,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onedrive_plans_map_to_link_terms() {
        let free = onedrive_terms("personal", 5 * 1024 * 1024 * 1024);
        assert!(!free.expiry && !free.password && !free.note.is_empty());
        let m365 = onedrive_terms("personal", 1024 * 1024 * 1024 * 1024);
        assert!(m365.expiry && m365.password);
        let biz = onedrive_terms("business", 1024 * 1024 * 1024 * 1024);
        assert!(biz.expiry && !biz.password);
        let mut a = CloudAccount::empty();
        a.password = true;
        a.set_link_terms(Some(&free));
        assert_eq!((a.expire_days, a.password), (0, false));
        assert!(!a.expiry_allowed() && !a.password_allowed());
        a.set_link_terms(None);
        assert!(a.expiry_allowed() && a.link_note.is_empty());
    }
}
