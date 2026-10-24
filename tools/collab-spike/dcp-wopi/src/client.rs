//! A WOPI **client simulator** that is itself a [`DocumentStore`].
//!
//! It talks to a [`crate::WopiHost`] only through WOPI HTTP calls, in the order
//! Collabora Online uses them (the order is recorded in the Spike D results):
//! `CheckFileInfo`, `Lock`, `GetFile`, `PutFile` (with `X-WOPI-Lock` and
//! `X-COOL-WOPI-Timestamp`), `Unlock`.
//!
//! What WOPI cannot express is refused with [`StoreError::Invalid`] and
//! described by [`WopiClientStore::capabilities`], so a conformance journey
//! reports "unsupported" instead of passing vacuously.

use std::collections::HashMap;

use async_trait::async_trait;
use dcp_core::{
    DocId, DocumentStore, Lease, LeaseState, Meta, NewFile, Result, Role, Stamped, StoreError,
    Update, Version,
};
use parking_lot::Mutex;
use reqwest::{Method, StatusCode};
use serde_json::Value;

use crate::host::{pct_encode, COOL_STATUS_DOC_CHANGED};

/// What this store can honestly do through WOPI. Every `false` is a reason a
/// journey must be reported as unsupported, not passed.
///
/// `*_via_extension` means: WOPI itself has no such call, and the simulator
/// gets it from a vendor extension of this host (`DcpVersions` in
/// CheckFileInfo, `?dcp_version=` on GetFile). A stock WOPI client such as
/// Collabora never uses these and cannot do the thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WopiCapabilities {
    /// `PutRelativeFile` is the only WOPI creation call and the host forbids it
    /// (`UserCanNotWriteRelative`).
    pub create_document: bool,
    /// WOPI has no permission-management call.
    pub grant_revoke: bool,
    /// `versions` lists the whole graph, and `fetch_file` reads any retained
    /// version. Plain WOPI shows the current file only.
    pub version_history_via_extension: bool,
    /// A save can be a child of one chosen retained version: the session opens
    /// it (`?dcp_version=`) and then saves, which is how Collabora's forced
    /// save forks.
    pub single_chosen_parent_via_extension: bool,
    /// A version can list more than one parent. A WOPI save has exactly one.
    pub merge_parents: bool,
    /// `message` survives. `PutFile` has no field for it.
    pub version_messages: bool,
    /// The lease record's `holder`, `base_version`, `renewal` and
    /// `duration_ms` are WRITABLE: `write_lease` carries only the lock string
    /// (through `LOCK`/`UNLOCK`; the host chooses holder, base_version,
    /// renewal and the fixed 1 800 000 duration), so the conformance lease
    /// journeys cannot run. READING is richer: `lease()` returns the real
    /// record through the host's `DcpLease` extension in CheckFileInfo.
    pub lease_fields: bool,
    /// L2 updates.
    pub updates: bool,
}

pub const CAPABILITIES: WopiCapabilities = WopiCapabilities {
    create_document: false,
    grant_revoke: false,
    version_history_via_extension: true,
    single_chosen_parent_via_extension: true,
    merge_parents: false,
    version_messages: false,
    lease_fields: false,
    updates: false,
};

pub struct WopiClientStore {
    base_url: String,
    principal: String,
    device: String,
    http: reqwest::Client,
    tokens: Mutex<HashMap<DocId, String>>,
}

struct FileInfo {
    raw: Value,
}

impl FileInfo {
    fn str(&self, k: &str) -> &str {
        self.raw.get(k).and_then(Value::as_str).unwrap_or("")
    }

    fn head_id(&self) -> Option<&str> {
        Some(self.str("Version")).filter(|v| !v.is_empty())
    }

}

fn stamped_version(raw: &Value, id: Option<&str>) -> Result<Option<Stamped<Version>>> {
    let Some(v) = raw.get("DcpVersion").filter(|v| !v.is_null()) else {
        return match id {
            None => Ok(None),
            Some(_) => Err(StoreError::Invalid(
                "host did not send DcpVersion: plain WOPI cannot supply parents, hash or author"
                    .into(),
            )),
        };
    };
    let body: Version = serde_json::from_value(v.clone())
        .map_err(|e| StoreError::Invalid(format!("DcpVersion does not parse: {e}")))?;
    let ts = raw.get("DcpVersionTimestampMs").and_then(Value::as_u64).unwrap_or(0);
    let sender = raw.get("DcpVersionSender").and_then(Value::as_str).unwrap_or("");
    let id = id.ok_or_else(|| StoreError::Invalid("DcpVersion without a Version id".into()))?;
    Ok(Some(Stamped::new(id, ts, sender, body)))
}

fn new_lock_string() -> String {
    let mut raw = [0u8; 8];
    getrandom::fill(&mut raw).expect("OS random source");
    format!("sim-lock-{}", hex::encode(raw))
}

impl WopiClientStore {
    pub fn new(
        base_url: impl Into<String>,
        principal: impl Into<String>,
        device: impl Into<String>,
    ) -> Self {
        WopiClientStore {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            principal: principal.into(),
            device: device.into(),
            http: reqwest::Client::builder().no_proxy().build().expect("reqwest client"),
            tokens: Mutex::new(HashMap::new()),
        }
    }

    /// Tell the simulator which access token opens `doc` (the token a real
    /// WOPI client receives in the `WOPISrc` launch).
    pub fn attach(&self, doc: impl Into<DocId>, token: impl Into<String>) {
        self.tokens.lock().insert(doc.into(), token.into());
    }

    pub const fn capabilities(&self) -> WopiCapabilities {
        CAPABILITIES
    }

    fn file_url(&self, doc: &str, contents: bool) -> String {
        format!(
            "{}/wopi/files/{}{}",
            self.base_url,
            pct_encode(doc),
            if contents { "/contents" } else { "" }
        )
    }

    fn token(&self, doc: &str) -> Result<String> {
        self.tokens
            .lock()
            .get(doc)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("no WOPI access token attached for {doc}")))
    }

    async fn send(
        &self,
        method: Method,
        doc: &str,
        contents: bool,
        headers: &[(&str, String)],
        body: Option<Vec<u8>>,
    ) -> Result<reqwest::Response> {
        self.send_q(method, doc, contents, &[], headers, body).await
    }

    async fn send_q(
        &self,
        method: Method,
        doc: &str,
        contents: bool,
        extra_query: &[(&str, &str)],
        headers: &[(&str, String)],
        body: Option<Vec<u8>>,
    ) -> Result<reqwest::Response> {
        let token = self.token(doc)?;
        let mut req = self
            .http
            .request(method, self.file_url(doc, contents))
            .query(&[("access_token", token.as_str())])
            .query(extra_query);
        for (k, v) in headers {
            req = req.header(*k, v);
        }
        if let Some(b) = body {
            req = req.header("content-type", "application/octet-stream").body(b);
        } else if !headers.is_empty() {
            // IIS-style hosts want a content-length on POST (see
            // WopiStorage::createLockRequest in Collabora).
            req = req.header("content-length", "0");
        }
        req.send().await.map_err(|e| StoreError::Transport(e.to_string()))
    }

    async fn file_info(&self, doc: &str) -> Result<FileInfo> {
        self.file_info_q(doc, false).await
    }

    async fn file_info_q(&self, doc: &str, history: bool) -> Result<FileInfo> {
        let q: &[(&str, &str)] = if history { &[("dcp_history", "1")] } else { &[] };
        let resp = self.send_q(Method::GET, doc, false, q, &[], None).await?;
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(|e| StoreError::Transport(e.to_string()))?;
        if status != StatusCode::OK {
            return Err(map_status(status, None, &bytes, "CheckFileInfo"));
        }
        let raw: Value = serde_json::from_slice(&bytes)
            .map_err(|e| StoreError::Transport(format!("CheckFileInfo is not JSON: {e}")))?;
        Ok(FileInfo { raw })
    }

    /// `X-WOPI-Override: <op>` POST. Returns the response on 200 and the
    /// mismatching lock string on 409.
    async fn lock_call(
        &self,
        doc: &str,
        op: &str,
        lock: Option<&str>,
        old_lock: Option<&str>,
    ) -> Result<reqwest::Response> {
        let mut h = vec![("X-WOPI-Override", op.to_string())];
        if let Some(l) = lock {
            h.push(("X-WOPI-Lock", l.to_string()));
        }
        if let Some(l) = old_lock {
            h.push(("X-WOPI-OldLock", l.to_string()));
        }
        let resp = self.send(Method::POST, doc, false, &h, None).await?;
        if resp.status() == StatusCode::OK {
            return Ok(resp);
        }
        let status = resp.status();
        let held = resp.headers().get("x-wopi-lock").and_then(|v| v.to_str().ok()).map(str::to_string);
        let bytes = resp.bytes().await.unwrap_or_default();
        Err(map_status(status, held.as_deref(), &bytes, op))
    }

    async fn get_lock(&self, doc: &str) -> Result<String> {
        let resp = self.lock_call(doc, "GET_LOCK", None, None).await?;
        Ok(resp
            .headers()
            .get("x-wopi-lock")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string())
    }
}

/// Status codes to errors, following what Collabora does with them
/// (`handleUploadToStorageResponse`, `classifyLockResponse`).
fn map_status(status: StatusCode, held_lock: Option<&str>, body: &[u8], what: &str) -> StoreError {
    let text = String::from_utf8_lossy(&body[..body.len().min(300)]).into_owned();
    match status.as_u16() {
        401 | 403 => StoreError::Forbidden(format!("{what}: {status} {text}")),
        404 => StoreError::NotFound(format!("{what}: {status} {text}")),
        409 => {
            let changed = serde_json::from_slice::<Value>(body)
                .ok()
                .and_then(|v| v.get("COOLStatusCode").and_then(Value::as_u64))
                == Some(u64::from(COOL_STATUS_DOC_CHANGED));
            if changed {
                StoreError::Conflict(format!("{what}: document changed in storage (COOLStatusCode 1010)"))
            } else {
                StoreError::Conflict(format!(
                    "{what}: lock mismatch, current lock {:?}",
                    held_lock.unwrap_or("")
                ))
            }
        }
        400 | 413 | 501 => StoreError::Invalid(format!("{what}: {status} {text}")),
        429 | 503 => StoreError::RateLimited { retry_after_ms: None },
        _ => StoreError::Transport(format!("{what}: {status} {text}")),
    }
}

fn unsupported(what: &str, why: &str) -> StoreError {
    StoreError::Invalid(format!("unsupported over WOPI: {what}: {why}"))
}

#[async_trait]
impl DocumentStore for WopiClientStore {
    fn principal(&self) -> String {
        self.principal.clone()
    }

    fn device(&self) -> String {
        self.device.clone()
    }

    async fn create_document(&self, _meta: &Meta, _first: Option<NewFile>) -> Result<DocId> {
        Err(unsupported(
            "create_document",
            "the only WOPI creation call is PutRelativeFile, and CheckFileInfo says UserCanNotWriteRelative",
        ))
    }

    async fn meta(&self, doc: &DocId) -> Result<Meta> {
        let info = self.file_info(doc).await?;
        let m = info
            .raw
            .get("DcpMeta")
            .filter(|m| !m.is_null())
            .ok_or_else(|| StoreError::Invalid("host did not send DcpMeta".into()))?;
        serde_json::from_value(m.clone())
            .map_err(|e| StoreError::Invalid(format!("DcpMeta does not parse: {e}")))
    }

    async fn role(&self, doc: &DocId) -> Result<Role> {
        let info = self.file_info(doc).await?;
        if let Some(r) = info.raw.get("DcpRole").filter(|r| !r.is_null()) {
            return serde_json::from_value(r.clone())
                .map_err(|e| StoreError::Invalid(format!("DcpRole does not parse: {e}")));
        }
        // Plain WOPI: owner and editor are indistinguishable.
        let can_write = info.raw.get("UserCanWrite").and_then(Value::as_bool).unwrap_or(false);
        let comment = info.raw.get("UserCanOnlyComment").and_then(Value::as_bool).unwrap_or(false);
        Ok(match (can_write, comment) {
            (true, _) => Role::Editor,
            (false, true) => Role::Commenter,
            (false, false) => Role::Viewer,
        })
    }

    async fn grant(&self, _doc: &DocId, _principal: &str, _role: Role) -> Result<()> {
        Err(unsupported("grant", "WOPI has no permission-management operation"))
    }

    async fn revoke(&self, _doc: &DocId, _principal: &str) -> Result<()> {
        Err(unsupported("revoke", "WOPI has no permission-management operation"))
    }

    async fn versions(&self, doc: &DocId) -> Result<Vec<Stamped<Version>>> {
        // Plain WOPI: CheckFileInfo describes the current file only (its
        // optional `Version` is an opaque string of the head). This host also
        // lists every version when asked (`DcpVersions`).
        let info = self.file_info_q(doc, true).await?;
        let list = info
            .raw
            .get("DcpVersions")
            .and_then(Value::as_array)
            .ok_or_else(|| StoreError::Invalid("host did not send DcpVersions".into()))?;
        list.iter()
            .map(|e| {
                let id = e.get("id").and_then(Value::as_str).unwrap_or_default();
                let ts = e.get("ts_ms").and_then(Value::as_u64).unwrap_or(0);
                let sender = e.get("sender").and_then(Value::as_str).unwrap_or_default();
                let body: Version = serde_json::from_value(e.get("version").cloned().unwrap_or(Value::Null))
                    .map_err(|err| StoreError::Invalid(format!("DcpVersions entry does not parse: {err}")))?;
                Ok(Stamped::new(id, ts, sender, body))
            })
            .collect()
    }

    async fn fetch_file(&self, doc: &DocId, version: &Stamped<Version>) -> Result<Vec<u8>> {
        // The standard GetFile cannot name a version; `?dcp_version=` is this
        // host's extension.
        let resp = self
            .send_q(Method::GET, doc, true, &[("dcp_version", version.id.as_str())], &[], None)
            .await?;
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(|e| StoreError::Transport(e.to_string()))?;
        if status != StatusCode::OK {
            return Err(map_status(status, None, &bytes, "GetFile"));
        }
        Ok(bytes.to_vec())
    }

    async fn publish_version(
        &self,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        _message: Option<String>,
    ) -> Result<Stamped<Version>> {
        if parents.len() > 1 {
            return Err(unsupported("publish_version", "a WOPI save has exactly one parent; merges are inexpressible"));
        }
        let info = self.file_info(doc).await?;
        let head = info.head_id().map(str::to_string);

        // `base` is the version the session opens and the host will take as
        // the parent. `forced` mirrors Collabora's "savetostorage force=1": no
        // timestamp precondition, so the host may fork instead of refusing.
        let (base, forced) = match (&head, parents.first()) {
            (None, None) => (None, false),
            (Some(h), Some(p)) if h == p => (Some(p.clone()), false),
            (Some(_), Some(p)) => (Some(p.clone()), true),
            (Some(_), None) => {
                return Err(unsupported("publish_version", "parents [] on a document that already has versions"));
            }
            (None, Some(p)) => {
                return Err(StoreError::Invalid(format!("parent {p} does not exist: the document has no version")));
            }
        };

        // Collabora's measured order (Spike D, run on collabora/code 26.04.5.2):
        // CheckFileInfo, GetFile, LOCK, PutFile ... UNLOCK on close.
        if let Some(base) = &base {
            self.read_base(doc, base, forced).await?;
        }
        let lock = new_lock_string();
        self.lock_call(doc, "LOCK", Some(&lock), None).await?;
        let result = self.save_locked(doc, &info, &lock, forced, file).await;
        // Collabora unlocks when its last editing session goes; a failed unlock
        // after a good save does not undo the version.
        let _ = self.lock_call(doc, "UNLOCK", Some(&lock), None).await;
        result
    }

    /// With the host's `DcpLease` member the real active record comes back,
    /// stamped with its binding timestamp (`DcpLeaseTimestampMs`), so holder,
    /// base_version, renewal and duration_ms are all readable. `DcpLease`
    /// null means no lease is active: a released record stamped with the
    /// host's clock. Only when the member is absent (a host that is not this
    /// one) does it fall back to `GetLock`, which yields the lock string and
    /// nothing else (so that held record has no `renewal`).
    async fn lease(&self, doc: &DocId) -> Result<Option<Stamped<Lease>>> {
        let info = self.file_info(doc).await?;
        if let Some(member) = info.raw.get("DcpLease") {
            if member.is_null() {
                let now = info
                    .raw
                    .get("DcpNowMs")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| StoreError::Invalid("host did not send DcpNowMs; WOPI has no clock".into()))?;
                return Ok(Some(Stamped::new("wopi-lock", now, "", dcp_core::lease::released())));
            }
            let body: Lease = serde_json::from_value(member.clone())
                .map_err(|e| StoreError::Invalid(format!("DcpLease does not parse: {e}")))?;
            let ts = info
                .raw
                .get("DcpLeaseTimestampMs")
                .and_then(Value::as_u64)
                .ok_or_else(|| StoreError::Invalid("DcpLease without DcpLeaseTimestampMs".into()))?;
            return Ok(Some(Stamped::new("wopi-lock", ts, "", body)));
        }
        let token = self.get_lock(doc).await?;
        let now = self.now_ms(doc).await?;
        let body = if token.is_empty() {
            dcp_core::lease::released()
        } else {
            // WOPI GetLock returns the lock string and nothing else.
            Lease {
                dcp: 1,
                state: LeaseState::Held,
                holder: None,
                token: Some(token),
                base_version: None,
                renewal: None,
                duration_ms: None,
            }
        };
        Ok(Some(Stamped::new("wopi-lock", now, "", body)))
    }

    async fn write_lease(&self, doc: &DocId, lease: Lease) -> Result<Stamped<Lease>> {
        match lease.state {
            LeaseState::Held => {
                let token = lease
                    .token
                    .clone()
                    .ok_or_else(|| StoreError::Invalid("a held lease needs a token".into()))?;
                self.lock_call(doc, "LOCK", Some(&token), None).await?;
            }
            LeaseState::Released => {
                let current = self.get_lock(doc).await?;
                if !current.is_empty() {
                    self.lock_call(doc, "UNLOCK", Some(&current), None).await?;
                }
            }
        }
        let now = self.now_ms(doc).await?;
        Ok(Stamped::new("wopi-lock", now, self.principal.clone(), lease))
    }

    async fn now_ms(&self, doc: &DocId) -> Result<u64> {
        self.file_info(doc)
            .await?
            .raw
            .get("DcpNowMs")
            .and_then(Value::as_u64)
            .ok_or_else(|| StoreError::Invalid("host did not send DcpNowMs; WOPI has no clock".into()))
    }

    async fn publish_update(&self, _doc: &DocId, _update: Update) -> Result<Stamped<Update>> {
        Err(unsupported("publish_update", "WOPI has no update stream (L2)"))
    }

    async fn updates(&self, _doc: &DocId) -> Result<Vec<Stamped<Update>>> {
        Err(unsupported("updates", "WOPI has no update stream (L2)"))
    }
}

impl WopiClientStore {
    /// Collabora downloads the base before it edits; the host takes that read
    /// as the version a later PutFile is a child of. A base that is not the
    /// head is opened by version (host extension).
    async fn read_base(&self, doc: &str, base: &str, forced: bool) -> Result<()> {
        let q: Vec<(&str, &str)> = if forced { vec![("dcp_version", base)] } else { vec![] };
        let resp = self.send_q(Method::GET, doc, true, &q, &[], None).await?;
        let status = resp.status();
        let served = resp
            .headers()
            .get("x-wopi-itemversion")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let bytes = resp.bytes().await.map_err(|e| StoreError::Transport(e.to_string()))?;
        if status != StatusCode::OK {
            return Err(map_status(status, None, &bytes, "GetFile"));
        }
        if served.as_deref() != Some(base) {
            return Err(StoreError::Conflict(format!(
                "head moved from {base} to {served:?} between CheckFileInfo and GetFile"
            )));
        }
        Ok(())
    }

    async fn save_locked(
        &self,
        doc: &str,
        info: &FileInfo,
        lock: &str,
        forced: bool,
        file: NewFile,
    ) -> Result<Stamped<Version>> {
        let mut headers = vec![
            ("X-WOPI-Override", "PUT".to_string()),
            ("X-WOPI-Lock", lock.to_string()),
            ("X-COOL-WOPI-IsModifiedByUser", "true".to_string()),
            ("X-COOL-WOPI-IsAutosave", "false".to_string()),
            ("X-COOL-WOPI-IsExitSave", "false".to_string()),
        ];
        if !forced && info.head_id().is_some() {
            headers.push(("X-COOL-WOPI-Timestamp", info.str("LastModifiedTime").to_string()));
        }
        let resp = self.send(Method::POST, doc, true, &headers, Some(file.bytes)).await?;
        let status = resp.status();
        let held = resp.headers().get("x-wopi-lock").and_then(|v| v.to_str().ok()).map(str::to_string);
        let item = resp.headers().get("x-wopi-itemversion").and_then(|v| v.to_str().ok()).map(str::to_string);
        let bytes = resp.bytes().await.map_err(|e| StoreError::Transport(e.to_string()))?;
        if status != StatusCode::OK {
            return Err(map_status(status, held.as_deref(), &bytes, "PutFile"));
        }
        let raw: Value = serde_json::from_slice(&bytes)
            .map_err(|e| StoreError::Transport(format!("PutFile response is not JSON: {e}")))?;
        let id = item.ok_or_else(|| StoreError::Transport("PutFile response has no X-WOPI-ItemVersion".into()))?;
        stamped_version(&raw, Some(&id))?
            .ok_or_else(|| StoreError::Invalid("PutFile response has no DcpVersion".into()))
    }
}
