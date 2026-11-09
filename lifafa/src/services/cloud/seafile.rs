//! Seafile: the web API, signed in with the account password (turned into
//! an API token) or a pasted token; uploads go into a library, made when
//! missing.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use super::http::{
    chunk_ranges, cloudflare_error, file_slice, short_error, status_error, Wording,
    CLOUDFLARE_CHUNK,
};
use super::{expiry, local_file, share_password, CloudAccount, ShareResult};

fn seafile_auth(token: &str) -> String {
    format!("Token {token}")
}

const WORDING: Wording = Wording {
    refused: &[401, 403],
    refused_text: "the server refused the sign-in. Check the e-mail and the password or API token.",
    not_found: true,
    message: short_error,
    code_first: false,
};

pub(super) fn seafile_err(what: &str, e: ureq::Error) -> String {
    status_error(what, e, &WORDING)
}

/// Sign in with the password and a two-step verification code, once:
/// the token that comes back is what the keyring keeps from then on, as
/// the password alone would not get past the code at upload time.
/// Answers with the token and the account's display name.
pub(crate) fn seafile_login_with_code(
    account: &CloudAccount,
    password: &str,
    code: &str,
) -> Result<(String, String), String> {
    let url = format!("{}/api2/auth-token/", account.base());
    let v: serde_json::Value = ureq::post(&url)
        .set("X-SEAFILE-OTP", code.trim())
        .timeout(Duration::from_secs(30))
        .send_form(&[("username", account.user.trim()), ("password", password)])
        .map_err(|e| match e {
            ureq::Error::Status(400, resp) => {
                format!(
                    "Could not sign in: {}",
                    short_error(&resp.into_string().unwrap_or_default())
                )
            }
            e => seafile_err("Could not sign in", e),
        })?
        .into_json()
        .map_err(|e| format!("Could not read the server's answer: {e}"))?;
    let token = v["token"].as_str().unwrap_or("").to_string();
    if token.is_empty() {
        return Err("Could not sign in: the server gave no token.".to_string());
    }
    let who = seafile_whoami(account, &token)?;
    Ok((token, who))
}

/// The API token for the account: the secret is the password, turned
/// into a token by the auth-token endpoint, or already a token (from a
/// two-step sign-in, or obtained another way and pasted).
pub(super) fn seafile_token(account: &CloudAccount, secret: &str) -> Result<String, String> {
    let url = format!("{}/api2/auth-token/", account.base());
    let login = ureq::post(&url)
        .timeout(Duration::from_secs(30))
        .send_form(&[("username", account.user.trim()), ("password", secret)]);
    let login_err = match login {
        Ok(resp) => {
            let v: serde_json::Value = resp
                .into_json()
                .map_err(|e| format!("Could not read the server's answer: {e}"))?;
            match v["token"].as_str() {
                Some(t) if !t.is_empty() => return Ok(t.to_string()),
                _ => "Could not sign in: the server gave no token.".to_string(),
            }
        }
        Err(ureq::Error::Status(400, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            if text.to_ascii_lowercase().contains("otp") || text.contains("two") {
                "Could not sign in: the account uses two-step verification. Enter the current code from your authenticator app in the account's settings and press Check Connection.".to_string()
            } else {
                format!("Could not sign in: {}", short_error(&text))
            }
        }
        Err(e) => seafile_err("Could not sign in", e),
    };
    // Not a password, then: perhaps a token.
    if seafile_whoami(account, secret).is_ok() {
        return Ok(secret.to_string());
    }
    Err(login_err)
}

/// The signed-in account's name (or e-mail).
pub(super) fn seafile_whoami(account: &CloudAccount, token: &str) -> Result<String, String> {
    let v: serde_json::Value = ureq::get(&format!("{}/api2/account/info/", account.base()))
        .set("Authorization", &seafile_auth(token))
        .timeout(Duration::from_secs(30))
        .call()
        .map_err(|e| seafile_err("Could not sign in", e))?
        .into_json()
        .map_err(|e| format!("Could not read the server's answer: {e}"))?;
    let email = v["email"].as_str().unwrap_or("");
    let name = v["name"].as_str().unwrap_or("");
    if email.is_empty() && name.is_empty() {
        return Err(
            "The server answered, but not like a Seafile: is the URL its root?".to_string(),
        );
    }
    Ok(if name.is_empty() {
        email.to_string()
    } else {
        name.to_string()
    })
}

/// The id of the account's library, made when there is none by that name.
fn seafile_library(account: &CloudAccount, token: &str) -> Result<String, String> {
    let base = account.base();
    let want = account.library_clean();
    let repos: serde_json::Value = ureq::get(&format!("{base}/api2/repos/?type=mine"))
        .set("Authorization", &seafile_auth(token))
        .timeout(Duration::from_secs(60))
        .call()
        .map_err(|e| seafile_err("Could not list the libraries", e))?
        .into_json()
        .map_err(|e| format!("Could not read the library list: {e}"))?;
    if let Some(list) = repos.as_array() {
        // An exact name first, then any case.
        for exact in [true, false] {
            for r in list {
                let name = r["name"].as_str().unwrap_or("");
                let hit = if exact {
                    name == want
                } else {
                    name.eq_ignore_ascii_case(&want)
                };
                if hit && !r["encrypted"].as_bool().unwrap_or(false) {
                    if let Some(id) = r["id"].as_str() {
                        return Ok(id.to_string());
                    }
                }
            }
        }
    }
    let v: serde_json::Value = ureq::post(&format!("{base}/api2/repos/"))
        .set("Authorization", &seafile_auth(token))
        .timeout(Duration::from_secs(60))
        .send_form(&[("name", want.as_str()), ("desc", "Files shared from Hylki")])
        .map_err(|e| seafile_err("Could not create the library", e))?
        .into_json()
        .map_err(|e| format!("Could not read the new library's answer: {e}"))?;
    v["repo_id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "Could not create the library: the server gave no id.".to_string())
}

/// The multipart body of a Seafile upload, streamed: the fields and the
/// file's head, the file itself, the closing boundary.
fn multipart_upload<R: Read>(
    fields: &[(&str, &str)],
    name: &str,
    file: R,
    size: u64,
) -> Result<(String, u64, impl Read), String> {
    let boundary = format!(
        "----HylkiUpload{}",
        crate::rng::token(24).map_err(|e| e.to_string())?
    );
    let mut head = String::new();
    for (k, v) in fields {
        head.push_str(&format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n"
        ));
    }
    let safe = name.replace(['\\', '"'], "_").replace(['\r', '\n'], " ");
    head.push_str(&format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{safe}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    ));
    let tail = format!("\r\n--{boundary}--\r\n");
    let len = head.len() as u64 + size + tail.len() as u64;
    let body = std::io::Cursor::new(head.into_bytes())
        .chain(file)
        .chain(std::io::Cursor::new(tail.into_bytes()));
    Ok((
        format!("multipart/form-data; boundary={boundary}"),
        len,
        body,
    ))
}

pub(super) fn seafile_upload_and_share(
    account: &CloudAccount,
    secret: &str,
    path: &Path,
    fixed: Option<&str>,
) -> Result<ShareResult, String> {
    let (name, size) = local_file(path)?;
    let token = seafile_token(account, secret)?;
    let authz = seafile_auth(&token);
    let base = account.base();
    let repo = seafile_library(account, &token)?;

    let folder = account.folder.trim().trim_matches('/').to_string();
    let dir = seafile_make_folders(&base, &repo, &authz, &folder)?;
    let parent = if dir.is_empty() {
        "/".to_string()
    } else {
        dir.clone()
    };

    let link = seafile_upload_link(&base, &repo, &authz, &parent)?;
    // replace=0: a taken name gets a numbered one, reported back.
    let fields = [("parent_dir", parent.as_str()), ("replace", "0")];
    let uploaded: serde_json::Value = if account.cloudflare && size > CLOUDFLARE_CHUNK {
        seafile_resumable_upload(&link, &authz, &fields, path, &name, size)?
    } else {
        let file = std::fs::File::open(path).map_err(|e| format!("could not read {name}: {e}"))?;
        let (ctype, len, body) = multipart_upload(&fields, &name, file, size)?;
        ureq::post(&link)
            .set("Authorization", &authz)
            .set("Content-Type", &ctype)
            .set("Content-Length", &len.to_string())
            .timeout(Duration::from_secs(60 * 60))
            .send(body)
            .map_err(|e| {
                cloudflare_error(account, "Upload failed", &name, size, &e)
                    .unwrap_or_else(|| seafile_err("Upload failed", e))
            })?
            .into_json()
            .map_err(|e| format!("Upload failed: could not read the answer: {e}"))?
    };
    let remote = uploaded[0]["name"]
        .as_str()
        .or_else(|| uploaded["name"].as_str())
        .unwrap_or(&name)
        .to_string();

    let file_path = if dir.is_empty() {
        format!("/{remote}")
    } else {
        format!("{dir}/{remote}")
    };
    let expires = expiry(account);
    let pw = share_password(account, fixed)?;
    let url = seafile_share_link(
        account,
        &authz,
        &repo,
        &file_path,
        pw.as_deref(),
        expires.is_some(),
    )?;
    Ok(ShareResult {
        name: remote,
        size,
        url,
        password: pw,
        expires,
    })
}

/// The folder inside the library, one segment at a time: the mkdir wants
/// its parent to exist. Answers with its path in the library (empty for
/// the library's root).
fn seafile_make_folders(
    base: &str,
    repo: &str,
    authz: &str,
    folder: &str,
) -> Result<String, String> {
    let mut dir = String::new();
    for part in folder.split('/').filter(|p| !p.is_empty()) {
        dir.push('/');
        dir.push_str(part);
        let url = format!(
            "{base}/api2/repos/{repo}/dir/?p={}",
            crate::encoding::percent::encode_path(&dir)
        );
        let there = ureq::get(&url)
            .set("Authorization", authz)
            .timeout(Duration::from_secs(60))
            .call();
        match there {
            Ok(_) => {}
            Err(ureq::Error::Status(404, _)) => {
                ureq::post(&url)
                    .set("Authorization", authz)
                    .timeout(Duration::from_secs(60))
                    .send_form(&[("operation", "mkdir")])
                    .map_err(|e| seafile_err("Could not create the folder", e))?;
            }
            Err(e) => return Err(seafile_err("Could not look at the folder", e)),
        }
    }
    Ok(dir)
}

/// The one-off link from the file server the upload goes to, asking for a
/// JSON answer.
fn seafile_upload_link(
    base: &str,
    repo: &str,
    authz: &str,
    parent: &str,
) -> Result<String, String> {
    let link: serde_json::Value = ureq::get(&format!(
        "{base}/api2/repos/{repo}/upload-link/?p={}",
        crate::encoding::percent::encode_path(parent)
    ))
    .set("Authorization", authz)
    .timeout(Duration::from_secs(60))
    .call()
    .map_err(|e| seafile_err("Could not get an upload link", e))?
    .into_json()
    .map_err(|e| format!("Could not read the upload link: {e}"))?;
    let link = link.as_str().unwrap_or("").to_string();
    if link.is_empty() {
        return Err("Could not get an upload link: the server gave none.".to_string());
    }
    let sep = if link.contains('?') { '&' } else { '?' };
    Ok(format!("{link}{sep}ret-json=1"))
}

/// Resumable upload: the same link takes the file in pieces, each a
/// multipart post with Content-Range saying where it goes and
/// Content-Disposition naming the file the pieces belong to. The file
/// server keeps them until the last one lands, and only that answer names
/// the file.
fn seafile_resumable_upload(
    link: &str,
    authz: &str,
    fields: &[(&str, &str)],
    path: &Path,
    name: &str,
    size: u64,
) -> Result<serde_json::Value, String> {
    let safe = name.replace(['\\', '"'], "_").replace(['\r', '\n'], " ");
    let mut last = serde_json::Value::Null;
    for (i, (offset, len)) in chunk_ranges(size, CLOUDFLARE_CHUNK).into_iter().enumerate() {
        let piece = file_slice(path, name, offset, len)?;
        let (ctype, body_len, body) = multipart_upload(fields, name, piece, len)?;
        let end = (offset + len).saturating_sub(1);
        last = ureq::post(link)
            .set("Authorization", authz)
            .set("Content-Type", &ctype)
            .set("Content-Length", &body_len.to_string())
            .set("Content-Range", &format!("bytes {offset}-{end}/{size}"))
            .set(
                "Content-Disposition",
                &format!("attachment; filename=\"{safe}\""),
            )
            .timeout(Duration::from_secs(60 * 60))
            .send(body)
            .map_err(|e| seafile_err(&format!("Upload failed at piece {}", i + 1), e))?
            .into_json()
            .map_err(|e| format!("Upload failed: could not read the answer: {e}"))?;
    }
    Ok(last)
}

/// The share link to `file_path` in the library, with the password and
/// the account's expiry when wanted.
fn seafile_share_link(
    account: &CloudAccount,
    authz: &str,
    repo: &str,
    file_path: &str,
    password: Option<&str>,
    expires: bool,
) -> Result<String, String> {
    let days = account.expire_days.to_string();
    let mut form: Vec<(&str, &str)> = vec![("repo_id", repo), ("path", file_path)];
    if let Some(p) = password {
        form.push(("password", p));
    }
    if expires {
        form.push(("expire_days", days.as_str()));
    }
    let v: serde_json::Value = ureq::post(&format!("{}/api/v2.1/share-links/", account.base()))
        .set("Authorization", authz)
        .timeout(Duration::from_secs(60))
        .send_form(&form)
        .map_err(|e| seafile_err("Uploaded, but could not create the share link", e))?
        .into_json()
        .map_err(|e| format!("Uploaded, but could not read the share answer: {e}"))?;
    let url = v["link"].as_str().unwrap_or("").to_string();
    if url.is_empty() {
        return Err(format!(
            "Uploaded, but the server made no share link: {}",
            v["error_msg"].as_str().unwrap_or("no link in the answer")
        ));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_body_has_the_right_length() {
        let dir = std::env::temp_dir().join(format!("hylki-mp-{}", std::process::id()));
        std::fs::write(&dir, b"hello").unwrap();
        let f = std::fs::File::open(&dir).unwrap();
        let (ctype, len, mut body) = multipart_upload(
            &[("parent_dir", "/"), ("replace", "0")],
            "a \"b\".txt",
            f,
            5,
        )
        .unwrap();
        let mut bytes = Vec::new();
        body.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.len() as u64, len);
        assert!(ctype.starts_with("multipart/form-data; boundary="));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("filename=\"a _b_.txt\""));
        assert!(text.contains("\r\n\r\nhello\r\n--"));
        let _ = std::fs::remove_file(&dir);
    }
}
