//! Nextcloud, ownCloud and OpenCloud: WebDAV under
//! `remote.php/dav/files/<user>/` for the upload, the OCS files-sharing
//! endpoint for the public link.

use std::path::Path;
use std::time::Duration;

use super::http::{chunk_ranges, cloudflare_error, file_slice, http_err, CLOUDFLARE_CHUNK};
use super::{expiry, local_file, share_password, CloudAccount, ShareResult};

/// Which product a WebDAV server is, from its `status.php` (Nextcloud and
/// ownCloud 10 answer `productname`; Infinite Scale and OpenCloud answer
/// `product`). Unauthenticated and quick; none when the server does not
/// say or cannot be reached.
pub(crate) fn detect_product(base: &str) -> Option<String> {
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() {
        return None;
    }
    let v: serde_json::Value = ureq::get(&format!("{base}/status.php"))
        .timeout(Duration::from_secs(15))
        .call()
        .ok()?
        .into_json()
        .ok()?;
    let name = v["productname"]
        .as_str()
        .or_else(|| v["product"].as_str())
        .unwrap_or("")
        .to_lowercase();
    let product = if name.contains("nextcloud") {
        "nextcloud"
    } else if name.contains("opencloud") {
        "opencloud"
    } else if name.contains("owncloud") || name.contains("infinite scale") {
        "owncloud"
    } else {
        return None;
    };
    Some(product.to_string())
}

fn auth(account: &CloudAccount, password: &str) -> String {
    let raw = format!("{}:{}", account.user.trim(), password);
    format!("Basic {}", crate::encoding::base64_encode(raw.as_bytes()))
}

fn dav_root(account: &CloudAccount) -> String {
    format!(
        "{}/remote.php/dav/files/{}",
        account.base(),
        crate::encoding::percent::encode(account.user.trim())
    )
}

/// The OCS user endpoint answers with the display name.
pub(super) fn nextcloud_verify(account: &CloudAccount, password: &str) -> Result<String, String> {
    let url = format!("{}/ocs/v2.php/cloud/user?format=json", account.base());
    let v: serde_json::Value = ureq::get(&url)
        .set("Authorization", &auth(account, password))
        .set("OCS-APIRequest", "true")
        .timeout(Duration::from_secs(30))
        .call()
        .map_err(|e| http_err("Could not sign in", e))?
        .into_json()
        .map_err(|e| format!("Could not read the server's answer: {e}"))?;
    let data = &v["ocs"]["data"];
    let name = data["displayname"]
        .as_str()
        .or_else(|| data["display-name"].as_str())
        .or_else(|| data["id"].as_str())
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        return Err(
            "The server answered, but not like a Nextcloud: is the URL its root?".to_string(),
        );
    }
    Ok(name)
}

pub(super) fn nextcloud_upload_and_share(
    account: &CloudAccount,
    password: &str,
    path: &Path,
    fixed: Option<&str>,
) -> Result<ShareResult, String> {
    let (name, size) = local_file(path)?;
    let folder = account.folder_clean();
    let root = dav_root(account);
    let authz = auth(account, password);
    let timeout = Duration::from_secs(60 * 60);

    let dir = make_folders(&root, &folder, &authz)?;

    // A name that is free.
    let mut remote = name.clone();
    let exists = |n: &str| {
        ureq::head(&format!(
            "{root}{dir}/{}",
            crate::encoding::percent::encode(n)
        ))
        .set("Authorization", &authz)
        .call()
        .is_ok()
    };
    if exists(&remote) {
        remote = stamped_name(&name);
    }

    let target = format!("{root}{dir}/{}", crate::encoding::percent::encode(&remote));
    if account.cloudflare && size > CLOUDFLARE_CHUNK {
        chunked_upload(account, &authz, path, &name, size, &target, timeout)?;
    } else {
        let file = std::fs::File::open(path).map_err(|e| format!("could not read {name}: {e}"))?;
        ureq::put(&target)
            .set("Authorization", &authz)
            .set("Content-Length", &size.to_string())
            .set("Content-Type", "application/octet-stream")
            .timeout(timeout)
            .send(file)
            .map_err(|e| {
                cloudflare_error(account, "Upload failed", &name, size, &e)
                    .unwrap_or_else(|| http_err("Upload failed", e))
            })?;
    }

    let expires = expiry(account);
    let pw = share_password(account, fixed)?;
    let url = share_link(
        account,
        &authz,
        format!("/{folder}/{remote}"),
        expires.as_deref(),
        pw.as_deref(),
    )?;
    Ok(ShareResult {
        name: remote,
        size,
        url,
        password: pw,
        expires,
    })
}

/// Make each folder on the way to `folder` under the WebDAV `root`, and
/// answer with its encoded path. MKCOL says 405 when one already exists,
/// which is fine.
fn make_folders(root: &str, folder: &str, authz: &str) -> Result<String, String> {
    let mut dir = String::new();
    for part in folder.split('/').filter(|p| !p.is_empty()) {
        dir.push('/');
        dir.push_str(&crate::encoding::percent::encode(part));
        let r = ureq::request("MKCOL", &format!("{root}{dir}"))
            .set("Authorization", authz)
            .call();
        match r {
            Ok(_) | Err(ureq::Error::Status(405, _)) => {}
            Err(e) => return Err(http_err("Could not create the folder", e)),
        }
    }
    Ok(dir)
}

/// Chunked upload (the "uploads" DAV endpoint): the pieces go into a
/// one-off folder, numbered so they sort in order, and a MOVE of its
/// `.file` has the server stitch them into the target. Nextcloud's
/// v2 wants the target named on every request (Destination);
/// ownCloud and OpenCloud ignore that header and work the same way.
fn chunked_upload(
    account: &CloudAccount,
    authz: &str,
    path: &Path,
    name: &str,
    size: u64,
    target: &str,
    timeout: Duration,
) -> Result<(), String> {
    let upload = format!(
        "{}/remote.php/dav/uploads/{}/hylki-{}",
        account.base(),
        crate::encoding::percent::encode(account.user.trim()),
        crate::rng::token(12).map_err(|e| e.to_string())?
    );
    ureq::request("MKCOL", &upload)
        .set("Authorization", authz)
        .set("Destination", target)
        .timeout(Duration::from_secs(60))
        .call()
        .map_err(|e| http_err("Could not start the chunked upload", e))?;
    for (i, (offset, len)) in chunk_ranges(size, CLOUDFLARE_CHUNK).into_iter().enumerate() {
        let piece = file_slice(path, name, offset, len)?;
        ureq::put(&format!("{upload}/{:05}", i + 1))
            .set("Authorization", authz)
            .set("Destination", target)
            .set("Content-Length", &len.to_string())
            .set("Content-Type", "application/octet-stream")
            .timeout(timeout)
            .send(piece)
            .map_err(|e| http_err(&format!("Upload failed at piece {}", i + 1), e))?;
    }
    ureq::request("MOVE", &format!("{upload}/.file"))
        .set("Authorization", authz)
        .set("Destination", target)
        .set("OC-Total-Length", &size.to_string())
        .timeout(timeout)
        .call()
        .map_err(|e| {
            http_err(
                "Uploaded the pieces, but the server could not put them together",
                e,
            )
        })?;
    Ok(())
}

/// The public link to the file at `share_path`, through the OCS
/// files-sharing endpoint.
fn share_link(
    account: &CloudAccount,
    authz: &str,
    share_path: String,
    expires: Option<&str>,
    password: Option<&str>,
) -> Result<String, String> {
    let mut form: Vec<(&str, String)> = vec![
        ("path", share_path),
        ("shareType", "3".to_string()),
        ("permissions", "1".to_string()),
    ];
    if let Some(d) = expires {
        form.push(("expireDate", d.to_string()));
    }
    if let Some(p) = password {
        form.push(("password", p.to_string()));
    }
    let form_ref: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let v: serde_json::Value = ureq::post(&format!(
        "{}/ocs/v2.php/apps/files_sharing/api/v1/shares?format=json",
        account.base()
    ))
    .set("Authorization", authz)
    .set("OCS-APIRequest", "true")
    .timeout(Duration::from_secs(60))
    .send_form(&form_ref)
    .map_err(|e| http_err("Uploaded, but could not create the share link", e))?
    .into_json()
    .map_err(|e| format!("Uploaded, but could not read the share answer: {e}"))?;
    let url = v["ocs"]["data"]["url"].as_str().unwrap_or("").to_string();
    if url.is_empty() {
        let msg = v["ocs"]["meta"]["message"]
            .as_str()
            .unwrap_or("no link in the answer");
        return Err(format!(
            "Uploaded, but the server made no share link: {msg}"
        ));
    }
    Ok(url)
}

/// `report-20260909-141500.pdf`: the name with the time in it, for when
/// the plain one is taken.
fn stamped_name(name: &str) -> String {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}-{stamp}.{ext}"),
        _ => format!("{name}-{stamp}"),
    }
}
