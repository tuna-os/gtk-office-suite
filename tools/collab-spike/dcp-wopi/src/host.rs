//! A WOPI host over any [`DocumentStore`] (spec 8.2).
//!
//! Mapping, in one place:
//!
//! | WOPI | DCP |
//! |---|---|
//! | file id | `DocId` |
//! | access token | registry entry: `(doc, store handle, principal)` |
//! | `CheckFileInfo` | `meta` + head of `versions` + `role` + the active lease (`DcpLease`) |
//! | `GetFile` | `fetch_file` of the head, hash and size checked here |
//! | `PutFile` | `publish_version` with the parent the session last read; refused (409, empty lock) when it has read none of an existing version |
//! | `Lock`/`RefreshLock`/`Unlock`/`UnlockAndRelock`/`GetLock` | `write_lease`/`lease` with `token` = the WOPI lock string, `renewal` per lease rule 8, `duration_ms` always 1 800 000, confirmed by read-back, settle, read-back (lease rule 3) |
//!
//! Locks are not user-owned in WOPI (Microsoft's concepts page): any valid
//! token with write permission may refresh or release a lock whose string it
//! knows. The DCP lease keeps the principal that took it as `holder`; a refresh
//! by another token keeps the original holder.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use dcp_core::lease;
use dcp_core::{
    DocId, DocumentStore, Holder, Lease, LeaseState, NewFile, Role, Stamped, StoreError, Version,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::package;
use crate::timefmt::{iso8601_ms, parse_iso8601_ms};

/// Longest lock string the host accepts. Spec 4.3 `token` allows 256;
/// WOPI allows 1024 when the host advertises `SupportsExtendedLockLength`,
/// which this host does not, so clients stay under 256.
pub const MAX_LOCK_LEN: usize = 256;

/// The `COOLStatusCode` Collabora reads from a 409 body to mean "the document
/// changed in storage" (`COOLStatusCode::DOC_CHANGED` in `common/Protocol.hpp`).
pub const COOL_STATUS_DOC_CHANGED: u32 = 1010;

#[derive(Clone, Debug)]
pub struct HostConfig {
    pub max_put_bytes: usize,
    /// Spec version rule 3: refuse a `PutFile` body that is not a readable ODF
    /// or OOXML package. On by default; the shared conformance fixtures use
    /// opaque bytes and turn it off.
    pub validate_packages: bool,
    /// Lease rule 3 settle interval: after writing a lease the host reads it
    /// back, waits this long, reads it back again, and only then answers 200.
    /// Defaults to [`lease::MATRIX_SETTLE_MS`]; tests over an in-memory store
    /// with a manual clock may set 0. The lease duration is not configurable:
    /// the host always writes [`lease::WOPI_DURATION_MS`] (spec 8.2 rule 4).
    pub lease_settle_ms: u64,
    /// Print one line per request to stderr (also enabled by `DCP_WOPI_LOG=1`).
    pub log_stderr: bool,
}

impl Default for HostConfig {
    fn default() -> Self {
        HostConfig {
            max_put_bytes: 64 * 1024 * 1024,
            validate_packages: true,
            lease_settle_ms: lease::MATRIX_SETTLE_MS,
            log_stderr: std::env::var("DCP_WOPI_LOG").is_ok_and(|v| v == "1"),
        }
    }
}

/// One request as the host saw it, for the Spike D results (what Collabora
/// actually sends). The access token is redacted.
#[derive(Clone, Debug)]
pub struct LoggedRequest {
    pub seq: u64,
    pub method: String,
    pub path: String,
    pub query: String,
    /// `X-WOPI-Override`, when present.
    pub operation: Option<String>,
    /// Request headers starting `X-WOPI-`, `X-COOL-WOPI-` or `X-LOOL-WOPI-`,
    /// plus `User-Agent` and `Content-Length`.
    pub headers: Vec<(String, String)>,
    pub status: u16,
    /// `X-WOPI-Lock` of the response, when present.
    pub response_lock: Option<String>,
    pub response_body_preview: String,
}

impl LoggedRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

struct Session {
    doc: DocId,
    store: Arc<dyn DocumentStore>,
    principal: String,
    /// Stamped on leases as `holder.session`.
    session_id: String,
    /// The version this session last read (`GetFile`) or wrote (`PutFile`).
    /// A save is a child of it (spec version rule 4).
    opened: Mutex<Option<String>>,
}

struct Inner {
    cfg: HostConfig,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    doc_locks: Mutex<HashMap<DocId, Arc<tokio::sync::Mutex<()>>>>,
    log: Mutex<Vec<LoggedRequest>>,
    seq: AtomicU64,
}

#[derive(Clone)]
pub struct WopiHost {
    inner: Arc<Inner>,
}

/// A host listening on a socket. Dropping it does not stop the server; call
/// [`RunningHost::shutdown`].
pub struct RunningHost {
    pub addr: SocketAddr,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl RunningHost {
    /// `http://<addr>` with no trailing slash.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub async fn shutdown(mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        let _ = self.task.await;
    }
}

impl Default for WopiHost {
    fn default() -> Self {
        Self::new(HostConfig::default())
    }
}

impl WopiHost {
    pub fn new(cfg: HostConfig) -> Self {
        WopiHost {
            inner: Arc::new(Inner {
                cfg,
                sessions: Mutex::new(HashMap::new()),
                doc_locks: Mutex::new(HashMap::new()),
                log: Mutex::new(Vec::new()),
                seq: AtomicU64::new(0),
            }),
        }
    }

    pub fn config(&self) -> &HostConfig {
        &self.inner.cfg
    }

    /// Register an access token. `store` is the principal's own handle on the
    /// backing binding (it enforces roles, spec conformance test 6); `principal`
    /// is what CheckFileInfo reports as `UserId`.
    pub fn register(
        &self,
        doc: impl Into<DocId>,
        store: Arc<dyn DocumentStore>,
        principal: impl Into<String>,
    ) -> String {
        let mut raw = [0u8; 18];
        getrandom::fill(&mut raw).expect("OS random source");
        let token = hex::encode(raw);
        let session_id = format!("wopi-{}", &hex::encode(Sha256::digest(token.as_bytes()))[..12]);
        let session = Session {
            doc: doc.into(),
            store,
            principal: principal.into(),
            session_id,
            opened: Mutex::new(None),
        };
        self.inner.sessions.lock().insert(token.clone(), Arc::new(session));
        token
    }

    pub fn revoke_token(&self, token: &str) {
        self.inner.sessions.lock().remove(token);
    }

    /// The WOPISrc (without the token) a client should be given for `doc`.
    pub fn wopi_src(base_url: &str, doc: &str) -> String {
        format!("{}/wopi/files/{}", base_url.trim_end_matches('/'), pct_encode(doc))
    }

    pub fn request_log(&self) -> Vec<LoggedRequest> {
        self.inner.log.lock().clone()
    }

    pub fn clear_log(&self) {
        self.inner.log.lock().clear();
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/wopi/files/{id}", get(check_file_info).post(file_operation))
            .route("/wopi/files/{id}/contents", get(get_file).post(put_file))
            .layer(DefaultBodyLimit::max(self.inner.cfg.max_put_bytes))
            .layer(middleware::from_fn_with_state(self.clone(), log_requests))
            .with_state(self.clone())
    }

    pub async fn serve(&self, addr: SocketAddr) -> std::io::Result<RunningHost> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let addr = listener.local_addr()?;
        let (stop, rx) = tokio::sync::oneshot::channel();
        let app = self.router();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    // Only an explicit `shutdown()` stops the server. A dropped
                    // handle leaves it serving until the runtime ends.
                    if rx.await.is_err() {
                        std::future::pending::<()>().await;
                    }
                })
                .await;
        });
        Ok(RunningHost { addr, stop: Some(stop), task, })
    }

    async fn doc_guard(&self, doc: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let m = {
            let mut locks = self.inner.doc_locks.lock();
            locks.entry(doc.to_string()).or_default().clone()
        };
        m.lock_owned().await
    }
}

// ---- helpers -------------------------------------------------------------

pub(crate) fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn hdr(h: &HeaderMap, name: &str) -> Option<String> {
    h.get(name).and_then(|v| v.to_str().ok()).map(str::to_string)
}

fn respond(status: StatusCode, headers: &[(&str, &str)], body: impl Into<Body>) -> Response {
    let mut r = Response::new(body.into());
    *r.status_mut() = status;
    for (k, v) in headers {
        let name = axum::http::HeaderName::from_bytes(k.as_bytes()).expect("static header name");
        // A lock string with bytes a header cannot carry is reported as
        // empty rather than corrupting the response.
        let value = HeaderValue::from_str(v).unwrap_or_else(|_| HeaderValue::from_static(""));
        r.headers_mut().insert(name, value);
    }
    r
}

fn text(status: StatusCode, msg: impl Into<String>) -> Response {
    respond(status, &[("content-type", "text/plain; charset=utf-8")], msg.into())
}

fn json_resp(status: StatusCode, v: &Value) -> Response {
    respond(status, &[("content-type", "application/json")], v.to_string())
}

/// 409 with the current lock, exactly as WOPI requires for every lock mismatch.
fn lock_mismatch(current: &str, reason: &str) -> Response {
    respond(
        StatusCode::CONFLICT,
        &[("x-wopi-lock", current), ("x-wopi-lockfailurereason", reason)],
        Body::empty(),
    )
}

/// 409 that Collabora reads as "document changed in storage": a body with
/// `COOLStatusCode` 1010 and no `X-WOPI-Lock` header.
fn doc_changed() -> Response {
    json_resp(
        StatusCode::CONFLICT,
        &json!({ "COOLStatusCode": COOL_STATUS_DOC_CHANGED, "Message": "document changed in storage" }),
    )
}

fn store_err(e: StoreError) -> Response {
    match e {
        StoreError::NotFound(m) => text(StatusCode::NOT_FOUND, m),
        StoreError::Forbidden(m) => text(StatusCode::FORBIDDEN, m),
        StoreError::Conflict(m) => respond(
            StatusCode::CONFLICT,
            &[("content-type", "text/plain; charset=utf-8"), ("x-wopi-lock", "")],
            m,
        ),
        StoreError::RateLimited { retry_after_ms } => {
            let secs = retry_after_ms.map_or(1, |ms| ms.div_ceil(1000).max(1)).to_string();
            respond(StatusCode::TOO_MANY_REQUESTS, &[("retry-after", &secs)], "rate limited")
        }
        StoreError::Invalid(m) => text(StatusCode::BAD_REQUEST, m),
        StoreError::Transport(m) => text(StatusCode::BAD_GATEWAY, m),
    }
}

fn is_lease_active(l: &Stamped<Lease>, now_ms: u64) -> bool {
    l.body.state == LeaseState::Held
        && l.body.duration_ms.is_some_and(|d| l.ts_ms.saturating_add(d) > now_ms)
}

/// Heads of the version graph, newest first by binding timestamp (spec 6.1).
fn heads_of(versions: &[Stamped<Version>]) -> Vec<Stamped<Version>> {
    let mut referenced = std::collections::HashSet::new();
    for v in versions {
        for p in &v.body.parents {
            referenced.insert(p.as_str());
        }
    }
    let mut heads: Vec<_> =
        versions.iter().filter(|v| !referenced.contains(v.id.as_str())).cloned().collect();
    heads.sort_by(|a, b| b.ts_ms.cmp(&a.ts_ms).then_with(|| b.id.cmp(&a.id)));
    heads
}

fn base_file_name(title: &str, mime: &str) -> String {
    let mut name: String = title.chars().filter(|c| !matches!(c, '/' | '\\') && !c.is_control()).collect();
    if name.trim().is_empty() {
        name = "document".to_string();
    }
    if let Some(ext) = package::extension_for_mime(mime) {
        if !name.to_ascii_lowercase().ends_with(&format!(".{ext}")) {
            name.push('.');
            name.push_str(ext);
        }
    }
    name
}

// ---- auth and shared operations --------------------------------------------

impl WopiHost {
    fn authenticate(
        &self,
        file_id: &str,
        query: &HashMap<String, String>,
        headers: &HeaderMap,
    ) -> Result<Arc<Session>, Response> {
        let token = query.get("access_token").cloned().or_else(|| {
            hdr(headers, "authorization")
                .and_then(|a| a.strip_prefix("Bearer ").map(str::to_string))
        });
        let Some(token) = token else {
            return Err(text(StatusCode::UNAUTHORIZED, "missing access token"));
        };
        let session = self.inner.sessions.lock().get(&token).cloned();
        match session {
            Some(s) if s.doc == file_id => Ok(s),
            // A token for another file is as good as an invalid one.
            _ => Err(text(StatusCode::UNAUTHORIZED, "invalid access token")),
        }
    }
}

async fn role_of(s: &Session) -> Result<Role, Response> {
    s.store.role(&s.doc).await.map_err(store_err)
}

async fn require_write(s: &Session) -> Result<Role, Response> {
    let role = role_of(s).await?;
    if role >= Role::Editor {
        Ok(role)
    } else {
        Err(text(
            StatusCode::FORBIDDEN,
            format!("role {role:?} may not write (spec 5: editor or owner)"),
        ))
    }
}

/// The active lease, if any, by the binding's own clock (spec 4.3 rules 1, 2).
async fn active_lease(s: &Session) -> Result<Option<Stamped<Lease>>, Response> {
    let now = s.store.now_ms(&s.doc).await.map_err(store_err)?;
    let lease = s.store.lease(&s.doc).await.map_err(store_err)?;
    Ok(lease.filter(|l| is_lease_active(l, now)))
}

fn lease_token(l: &Option<Stamped<Lease>>) -> String {
    l.as_ref().and_then(|l| l.body.token.clone()).unwrap_or_default()
}

// ---- CheckFileInfo -----------------------------------------------------------

async fn check_file_info(
    State(host): State<WopiHost>,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let s = match host.authenticate(&id, &q, &headers) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let with_history = q.get("dcp_history").is_some_and(|v| v == "1");
    match build_check_file_info(&s, with_history).await {
        Ok(v) => json_resp(StatusCode::OK, &v),
        Err(r) => r,
    }
}

async fn build_check_file_info(s: &Session, with_history: bool) -> Result<Value, Response> {
    let meta = s.store.meta(&s.doc).await.map_err(store_err)?;
    let role = role_of(s).await?;
    let versions = s.store.versions(&s.doc).await.map_err(store_err)?;
    let heads = heads_of(&versions);
    let head = heads.first();
    let now = s.store.now_ms(&s.doc).await.map_err(store_err)?;
    let active = active_lease(s).await?;

    let mime = head.map_or(meta.mime.as_str(), |h| h.body.file.mime.as_str());
    let owner = versions
        .iter()
        .filter(|v| v.body.parents.is_empty())
        .min_by_key(|v| v.ts_ms)
        .map_or_else(|| s.principal.clone(), |v| v.body.created_by.principal.clone());
    // More than one head is a conflict that must be shown, not hidden (spec
    // version rule 5). WOPI cannot show it, so the document opens read-only.
    let conflicted = heads.len() > 1;
    let can_write = role >= Role::Editor && !conflicted;

    Ok(json!({
        "BaseFileName": base_file_name(&meta.title, mime),
        "OwnerId": owner,
        "UserId": s.principal,
        "UserFriendlyName": s.principal,
        "Size": head.map_or(0, |h| h.body.file.size),
        "Version": head.map(|h| h.id.clone()).unwrap_or_default(),
        "LastModifiedTime": iso8601_ms(head.map_or(0, |h| h.ts_ms)),
        "UserCanWrite": can_write,
        "ReadOnly": !can_write,
        "UserCanOnlyComment": role == Role::Commenter,
        "UserCanNotWriteRelative": true,
        "SupportsLocks": true,
        "SupportsGetLock": true,
        "SupportsUpdate": true,
        "SupportsRename": false,
        "SupportsDeleteFile": false,
        "SupportsExtendedLockLength": false,
        // Vendor extensions. A plain WOPI client ignores unknown members; the
        // WopiClientStore simulator reads them to rebuild the DCP objects that
        // WOPI's own fields cannot carry (parents, hash, author, role, clock,
        // lease). `DcpLease` is the active lease record as the binding holds
        // it and `DcpLeaseTimestampMs` its binding timestamp; both null when
        // no lease is active (spec 4.3 rule 2). A WOPI client's own lock is
        // only the string; a DCP-aware client needs holder, base_version and
        // renewal too.
        "DcpLease": active.as_ref().map(|l| serde_json::to_value(&l.body).expect("Lease serialises")),
        "DcpLeaseTimestampMs": active.as_ref().map(|l| l.ts_ms),
        "DcpMeta": serde_json::to_value(&meta).expect("Meta serialises"),
        "DcpRole": serde_json::to_value(role).expect("Role serialises"),
        "DcpVersion": head.map(|h| serde_json::to_value(&h.body).expect("Version serialises")),
        "DcpVersionTimestampMs": head.map(|h| h.ts_ms),
        "DcpVersionSender": head.map(|h| h.sender.clone()),
        "DcpHeads": heads.iter().map(|h| h.id.clone()).collect::<Vec<_>>(),
        "DcpNowMs": now,
        // Every retained version, only when asked (`?dcp_history=1`): plain
        // WOPI has no history call, and Collabora never asks.
        "DcpVersions": with_history.then(|| versions.iter().map(version_json).collect::<Vec<_>>()),
    }))
}

fn version_json(v: &Stamped<Version>) -> Value {
    json!({
        "id": v.id,
        "ts_ms": v.ts_ms,
        "sender": v.sender,
        "version": serde_json::to_value(&v.body).expect("Version serialises"),
    })
}

// ---- GetFile -----------------------------------------------------------------

async fn get_file(
    State(host): State<WopiHost>,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let s = match host.authenticate(&id, &q, &headers) {
        Ok(s) => s,
        Err(r) => return r,
    };
    match serve_file(&s, q.get("dcp_version").map(String::as_str)).await {
        Ok(r) => r,
        Err(r) => r,
    }
}

/// `GetFile`. With `?dcp_version=<id>` (a host extension; the standard call has
/// no way to name a version) the file of that retained version is served, so a
/// session can open an older version and save a child of it (spec 6.2).
async fn serve_file(s: &Session, requested: Option<&str>) -> Result<Response, Response> {
    let versions = s.store.versions(&s.doc).await.map_err(store_err)?;
    let heads = heads_of(&versions);
    let served = match requested {
        Some(id) => Some(
            versions
                .iter()
                .find(|v| v.id == id)
                .ok_or_else(|| text(StatusCode::NOT_FOUND, format!("no retained version {id}")))?,
        ),
        None => heads.first(),
    };
    let Some(head) = served else {
        // Spec version rule 7: a document with no version has no content.
        return Ok(respond(
            StatusCode::OK,
            &[("content-type", "application/octet-stream"), ("x-wopi-itemversion", "")],
            Body::empty(),
        ));
    };
    let bytes = s.store.fetch_file(&s.doc, head).await.map_err(store_err)?;
    // Spec version rule 2: recompute sha256 and size, discard on mismatch.
    let sha = hex::encode(Sha256::digest(&bytes));
    if bytes.len() as u64 != head.body.file.size || !sha.eq_ignore_ascii_case(&head.body.file.sha256) {
        return Err(text(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("version {} failed its integrity check (size or sha256 differs)", head.id),
        ));
    }
    *s.opened.lock() = Some(head.id.clone());
    Ok(respond(
        StatusCode::OK,
        &[
            ("content-type", "application/octet-stream"),
            ("x-wopi-itemversion", &head.id),
            ("x-dcp-heads", &heads.len().to_string()),
        ],
        bytes,
    ))
}

// ---- PutFile -----------------------------------------------------------------

async fn put_file(
    State(host): State<WopiHost>,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let s = match host.authenticate(&id, &q, &headers) {
        Ok(s) => s,
        Err(r) => return r,
    };
    if let Some(op) = hdr(&headers, "x-wopi-override") {
        if !op.eq_ignore_ascii_case("PUT") {
            return text(StatusCode::NOT_IMPLEMENTED, format!("unsupported override {op}"));
        }
    }
    let _guard = host.doc_guard(&s.doc).await;
    match do_put(&host, &s, &headers, body.to_vec()).await {
        Ok(r) | Err(r) => r,
    }
}

async fn do_put(
    host: &WopiHost,
    s: &Session,
    headers: &HeaderMap,
    bytes: Vec<u8>,
) -> Result<Response, Response> {
    require_write(s).await?;
    let lease = active_lease(s).await?;
    let supplied = hdr(headers, "x-wopi-lock");
    let versions = s.store.versions(&s.doc).await.map_err(store_err)?;
    let heads = heads_of(&versions);
    let head = heads.first();
    let opened = s.opened.lock().clone();

    // 1. Lock check (MS PutFile): a held lock must match; the response to a
    //    mismatch carries the current lock.
    match (&lease, &supplied) {
        (Some(l), Some(sup)) if l.body.token.as_deref() == Some(sup.as_str()) => {}
        (Some(_), _) => {
            return Err(lock_mismatch(&lease_token(&lease), "PutFile lock does not match the held lease"));
        }
        (None, Some(sup)) if !sup.is_empty() => {
            // MS: an unlocked file with a lock string supplied is a mismatch,
            // reported with the empty lock.
            return Err(lock_mismatch("", "no lease is held (it may have expired)"));
        }
        (None, _) => {
            // No lease, no lock header. The task contract: accept only when
            // this session's base is the head; otherwise publish nothing.
            if let Some(h) = head {
                if opened.as_deref() != Some(h.id.as_str()) {
                    return Err(lock_mismatch(
                        "",
                        "no lease is held and the head is not the version this session opened",
                    ));
                }
            }
        }
    }

    // Spec 8.2 rule 1: a token that has read no version may not save over a
    // document that has one; there is no parent to give. Empty X-WOPI-Lock,
    // nothing published.
    if opened.is_none() && head.is_some() {
        return Err(lock_mismatch("", "this session has not read a version of the document"));
    }

    // 2. Collabora's timestamp precondition (X-COOL-WOPI-Timestamp, legacy
    //    X-LOOL-WOPI-Timestamp): the LastModifiedTime it was last given.
    if let (Some(h), Some(ts)) = (
        head,
        hdr(headers, "x-cool-wopi-timestamp").or_else(|| hdr(headers, "x-lool-wopi-timestamp")),
    ) {
        let theirs = parse_iso8601_ms(&ts);
        if theirs != Some(h.ts_ms) {
            return Err(doc_changed());
        }
    }

    // 3. Spec version rule 3: only publish a package that opens.
    let meta = s.store.meta(&s.doc).await.map_err(store_err)?;
    let mime = head.map_or(meta.mime.clone(), |h| h.body.file.mime.clone());
    if host.inner.cfg.validate_packages {
        package::validate(&mime, &bytes).map_err(|why| text(StatusCode::BAD_REQUEST, why))?;
    }

    // 4. Publish a child of the version this session read (rule 4). Rule 1
    //    refused a session that never read a version of a document that has
    //    one, so `opened` is only `None` here for a document with no version.
    let parents: Vec<String> = opened.iter().cloned().collect();
    let autosave = hdr(headers, "x-cool-wopi-isautosave").as_deref() == Some("true");
    let exit_save = hdr(headers, "x-cool-wopi-isexitsave").as_deref() == Some("true");
    let message = Some(format!("WOPI PutFile (autosave={autosave}, exit_save={exit_save})"));
    let published = s
        .store
        .publish_version(&s.doc, parents, NewFile { mime, bytes }, message)
        .await
        .map_err(store_err)?;
    *s.opened.lock() = Some(published.id.clone());
    Ok(respond(
        StatusCode::OK,
        &[("content-type", "application/json"), ("x-wopi-itemversion", &published.id)],
        json!({
            "LastModifiedTime": iso8601_ms(published.ts_ms),
            // Vendor extensions, as in CheckFileInfo: the simulator rebuilds
            // the published version from the response and never has to race a
            // second CheckFileInfo against another writer.
            "DcpVersion": serde_json::to_value(&published.body).expect("Version serialises"),
            "DcpVersionTimestampMs": published.ts_ms,
            "DcpVersionSender": published.sender,
        })
        .to_string(),
    ))
}

// ---- the lock family (POST /wopi/files/{id}) -------------------------------

async fn file_operation(
    State(host): State<WopiHost>,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let s = match host.authenticate(&id, &q, &headers) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let op = hdr(&headers, "x-wopi-override").unwrap_or_default().to_ascii_uppercase();
    let _guard = host.doc_guard(&s.doc).await;
    let result = match op.as_str() {
        "LOCK" if headers.contains_key("x-wopi-oldlock") => unlock_and_relock(&host, &s, &headers).await,
        "LOCK" => lock(&host, &s, &headers).await,
        "GET_LOCK" => get_lock(&s).await,
        "REFRESH_LOCK" => refresh_lock(&host, &s, &headers).await,
        "UNLOCK" => unlock(&s, &headers).await,
        // PUT_RELATIVE, RENAME_FILE, DELETE, PUT_USER_INFO, ...: CheckFileInfo
        // says UserCanNotWriteRelative/SupportsRename=false.
        other => Ok(text(
            StatusCode::NOT_IMPLEMENTED,
            format!("WOPI operation {other:?} is not supported by this host"),
        )),
    };
    match result {
        Ok(r) | Err(r) => r,
    }
}

fn lock_header(headers: &HeaderMap, name: &str) -> Result<String, Response> {
    match hdr(headers, name) {
        Some(v) if !v.is_empty() && v.chars().count() <= MAX_LOCK_LEN => Ok(v),
        Some(v) if !v.is_empty() => Err(text(
            StatusCode::BAD_REQUEST,
            format!("lock string longer than {MAX_LOCK_LEN} (DCP lease token limit)"),
        )),
        // MS Lock: 400 when X-WOPI-Lock was not provided or was empty.
        _ => Err(text(StatusCode::BAD_REQUEST, format!("{name} missing or empty"))),
    }
}

fn ok_empty() -> Response {
    respond(StatusCode::OK, &[], Body::empty())
}

/// The version the lease names as `base_version` (spec 8.2 rule 1): the one
/// this session last read or wrote, else the head, else `None`: a document
/// with no version has no base and the field is omitted, never filled with a
/// placeholder (spec 4.3).
async fn base_version_for(s: &Session) -> Result<Option<String>, Response> {
    if let Some(o) = s.opened.lock().clone() {
        return Ok(Some(o));
    }
    let versions = s.store.versions(&s.doc).await.map_err(store_err)?;
    Ok(heads_of(&versions).first().map(|h| h.id.clone()))
}

/// Write a held lease, then confirm it (spec 4.3 rules 3 and 8): read the
/// record over which it is written to get `renewal`, write, read back, wait
/// the settle interval, read back again, and answer `Ok` only when both
/// read-backs carry the holder, token and renewal written. `Err` is the 409
/// to return otherwise. The duration is always the WOPI one (spec 8.2 rule 4).
async fn write_held(
    s: &Session,
    host: &WopiHost,
    token: &str,
    holder: Holder,
    base_version: Option<String>,
) -> Result<(), Response> {
    let current = s.store.lease(&s.doc).await.map_err(store_err)?;
    let renewal = lease::next_renewal(current.as_ref().map(|c| &c.body));
    let lease = Lease {
        dcp: 1,
        state: LeaseState::Held,
        holder: Some(holder.clone()),
        token: Some(token.to_string()),
        base_version,
        renewal: Some(renewal),
        duration_ms: Some(lease::WOPI_DURATION_MS),
    };
    s.store.write_lease(&s.doc, lease).await.map_err(store_err)?;
    let ours = |r: &Stamped<Lease>| {
        r.body.state == LeaseState::Held
            && r.body.token.as_deref() == Some(token)
            && r.body.holder.as_ref() == Some(&holder)
            && r.body.renewal == Some(renewal)
    };
    let first = s.store.lease(&s.doc).await.map_err(store_err)?;
    if host.inner.cfg.lease_settle_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(host.inner.cfg.lease_settle_ms)).await;
    }
    let second = s.store.lease(&s.doc).await.map_err(store_err)?;
    match (first, second) {
        (Some(a), Some(b)) if ours(&a) && ours(&b) => Ok(()),
        (_, other) => Err(lock_mismatch(
            other.as_ref().and_then(|r| r.body.token.as_deref()).unwrap_or(""),
            "another writer won the lease read-back",
        )),
    }
}

fn my_holder(s: &Session) -> Holder {
    Holder { principal: s.principal.clone(), session: s.session_id.clone() }
}

async fn lock(host: &WopiHost, s: &Session, headers: &HeaderMap) -> Result<Response, Response> {
    let token = lock_header(headers, "x-wopi-lock")?;
    require_write(s).await?;
    let current = active_lease(s).await?;
    match current {
        None => {
            let base = base_version_for(s).await?;
            write_held(s, host, &token, my_holder(s), base).await?;
            Ok(ok_empty())
        }
        // MS Lock: same lock string on a locked file is a RefreshLock.
        Some(l) if l.body.token.as_deref() == Some(token.as_str()) => {
            refresh_existing(s, host, l).await?;
            Ok(ok_empty())
        }
        Some(l) => Err(lock_mismatch(
            &lease_token(&Some(l.clone())),
            &format!("locked by {}", l.body.holder.map_or_else(String::new, |h| h.principal)),
        )),
    }
}

/// Re-stamp a lease with the same token, holder and base, so the binding
/// timestamp (and so the expiry) moves. The new record is written over a held
/// one, so its `renewal` is the old one plus 1 (lease rule 8).
async fn refresh_existing(s: &Session, host: &WopiHost, l: Stamped<Lease>) -> Result<(), Response> {
    let token = l.body.token.clone().unwrap_or_default();
    let holder = l.body.holder.clone().unwrap_or_else(|| my_holder(s));
    let base = match l.body.base_version.clone() {
        Some(b) => Some(b),
        None => base_version_for(s).await?,
    };
    write_held(s, host, &token, holder, base).await
}

async fn get_lock(s: &Session) -> Result<Response, Response> {
    let current = active_lease(s).await?;
    Ok(respond(StatusCode::OK, &[("x-wopi-lock", &lease_token(&current))], Body::empty()))
}

async fn refresh_lock(
    host: &WopiHost,
    s: &Session,
    headers: &HeaderMap,
) -> Result<Response, Response> {
    let token = lock_header(headers, "x-wopi-lock")?;
    require_write(s).await?;
    match active_lease(s).await? {
        Some(l) if l.body.token.as_deref() == Some(token.as_str()) => {
            refresh_existing(s, host, l).await?;
            Ok(ok_empty())
        }
        other => Err(lock_mismatch(
            &lease_token(&other),
            if other.is_some() { "locked by another lock string" } else { "file is not locked" },
        )),
    }
}

async fn unlock(s: &Session, headers: &HeaderMap) -> Result<Response, Response> {
    let token = lock_header(headers, "x-wopi-lock")?;
    require_write(s).await?;
    match active_lease(s).await? {
        Some(l) if l.body.token.as_deref() == Some(token.as_str()) => {
            // Lease rule 8: a released record carries no `renewal`.
            s.store.write_lease(&s.doc, lease::released()).await.map_err(store_err)?;
            Ok(ok_empty())
        }
        other => Err(lock_mismatch(
            &lease_token(&other),
            if other.is_some() { "locked by another lock string" } else { "file is not locked" },
        )),
    }
}

async fn unlock_and_relock(
    host: &WopiHost,
    s: &Session,
    headers: &HeaderMap,
) -> Result<Response, Response> {
    let new_token = lock_header(headers, "x-wopi-lock")?;
    let old_token = lock_header(headers, "x-wopi-oldlock")?;
    require_write(s).await?;
    match active_lease(s).await? {
        Some(l) if l.body.token.as_deref() == Some(old_token.as_str()) => {
            // One lease record replaces the other: atomic as far as readers of
            // the lease can tell. It is written over a held record, so its
            // renewal is the replaced one plus 1 (lease rule 8).
            let holder = l.body.holder.clone().unwrap_or_else(|| my_holder(s));
            let base = match l.body.base_version.clone() {
                Some(b) => Some(b),
                None => base_version_for(s).await?,
            };
            write_held(s, host, &new_token, holder, base).await?;
            Ok(ok_empty())
        }
        other => Err(lock_mismatch(
            &lease_token(&other),
            if other.is_some() { "OldLock does not match" } else { "file is not locked" },
        )),
    }
}

// ---- request log ---------------------------------------------------------------

fn redact_query(q: &str) -> String {
    q.split('&')
        .map(|kv| match kv.split_once('=') {
            Some(("access_token", _)) => "access_token=<redacted>".to_string(),
            _ => kv.to_string(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

async fn log_requests(State(host): State<WopiHost>, req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let query = redact_query(req.uri().query().unwrap_or(""));
    let mut interesting = Vec::new();
    for (k, v) in req.headers() {
        let k = k.as_str();
        if k.starts_with("x-wopi-")
            || k.starts_with("x-cool-wopi-")
            || k.starts_with("x-lool-wopi-")
            || k == "user-agent"
            || k == "content-length"
        {
            interesting.push((k.to_string(), v.to_str().unwrap_or("<non-ascii>").to_string()));
        }
    }
    let operation = hdr(req.headers(), "x-wopi-override");
    let resp = next.run(req).await;
    let (parts, body) = resp.into_parts();
    // Only error bodies are previewed (they are small); file contents never are.
    let (body, preview) = if parts.status.as_u16() >= 400 {
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap_or_default();
        let p = String::from_utf8_lossy(&bytes[..bytes.len().min(300)]).into_owned();
        (Body::from(bytes), p)
    } else {
        (body, String::new())
    };
    let entry = LoggedRequest {
        seq: host.inner.seq.fetch_add(1, Ordering::Relaxed),
        method,
        path,
        query,
        operation,
        headers: interesting,
        status: parts.status.as_u16(),
        response_lock: hdr(&parts.headers, "x-wopi-lock"),
        response_body_preview: preview,
    };
    if host.inner.cfg.log_stderr {
        eprintln!(
            "[wopi-host] #{} {} {}{} op={:?} req_headers={:?} -> {} resp_lock={:?} body={:?}",
            entry.seq,
            entry.method,
            entry.path,
            if entry.query.is_empty() { String::new() } else { format!("?{}", entry.query) },
            entry.operation,
            entry.headers,
            entry.status,
            entry.response_lock,
            entry.response_body_preview,
        );
    }
    host.inner.log.lock().push(entry);
    Response::from_parts(parts, body)
}
