//! [`WebdavStore`]: the DCP [`DocumentStore`] over Nextcloud WebDAV and OCS.
//!
//! ## Layout (why it is not "one file and its ETag")
//!
//! A document `DCP-<12 hex>` is **two top-level folders** in the owner's home:
//!
//! ```text
//! DCP-<id>.v/            append-only: shared create-only with editors
//!     meta.json          the DCP metadata (4.1), written once
//!     <vid>.bin          the file of one version
//!     <vid>.json         the DCP version object (4.2): parents, file reference, author
//! DCP-<id>.l/            the lease log: shared with update and delete rights
//!     00000000.json      lease records, one new file per write; the highest number is the lease
//! ```
//!
//! Every version is its own pair of files, created with `If-None-Match: *`, so
//! publishing can never overwrite anything and two heads are two files. Spike E
//! measured why the alternatives do not carry L1 (see
//! `docs/rfc/0004-spike-e-results.md`): an ETag is not a content version, a
//! conditional overwrite is not atomic, Nextcloud's file versions are keyed by the
//! second and carry no parents, and custom properties are per user.
//!
//! The roles are the share permissions of the two folders: viewer 1 and 1;
//! editor 5 (read and create, no update, no delete) and 15; owner 31 and 31. An
//! editor therefore cannot overwrite or delete a version: the server enforces
//! version rules 1 and 4 for a well-behaved client and a hostile one alike.
//!
//! ## Create-if-absent is not a compare-and-set
//!
//! The lease log creates a name that is free: a writer that last read record `n`
//! creates record `n + 1` with `If-None-Match: *`. Of two writers one usually gets
//! `201`, the other `412` or `423` (that is [`StoreError::Conflict`]), and sometimes
//! both get `423`. It is NOT atomic: Nextcloud checks the precondition with a lookup
//! that is not under the file lock, so a creator that passed it just before the
//! other's record became visible can replace that record (answered `204`, or `201`)
//! while the first still holds its `201`. A write the server accepted is therefore
//! not an acquisition: lease rule 3's read-backs decide, and `lease()` reads the
//! newest record from the server every time.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use dcp_core::integrity;
use dcp_core::schema::{self, Kind};
use dcp_core::{
    DocId, DocumentStore, Lease, Locator, Meta, NewFile, Result, Role, Stamped, StoreError,
    Update, Version,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::clock::ServerClock;
use crate::dav::{percent_encode, status_error, Dav};
use crate::xml::{DAV, OC};

/// How to reach one Nextcloud account.
#[derive(Clone, Debug)]
pub struct WebdavConfig {
    /// `http://nc-a`
    pub base_url: String,
    pub user: String,
    pub password: String,
    /// The device identifier of this store (spec section 3).
    pub device: String,
    /// The scheme put in front of a remote host when sharing with a principal on
    /// another instance (`user@http://host`): Nextcloud only tries https
    /// otherwise, which the CI instances do not speak.
    pub remote_scheme: String,
}

/// One envelope per lease record. DCP's lease object has no place for the time or
/// the writer, and a WebDAV `Last-Modified` is whole seconds.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LeaseFile {
    /// The authenticated user who wrote it, as the writer says (WebDAV does not
    /// say who created a file).
    by: String,
    /// The writer's estimate of the server's clock when it sent the record
    /// ([`ServerClock`]), in milliseconds.
    at_ms: u64,
    lease: Lease,
}

pub struct WebdavStore {
    dav: Dav,
    device: String,
    remote_scheme: String,
    clock: Arc<ServerClock>,
    /// Set by the conformance fixture's fault hook: the next version upload is
    /// cut to half its bytes.
    truncate_next: Arc<AtomicBool>,
    /// The newest lease record this handle has read or written, per document.
    /// The next lease write is created one above its number, which is the
    /// compare-and-set; and a read only has to ask for the record after it.
    seen_lease: Mutex<HashMap<String, LeaseCache>>,
    /// How long this handle's last lease upload took, in milliseconds. The server
    /// stamps the record in the middle of the request, so half of it is added to
    /// the time the record carries.
    last_put_ms: AtomicU64,
}

#[derive(Clone, Default)]
struct LeaseCache {
    seq: Option<u64>,
    record: Option<Stamped<Lease>>,
}

fn rand_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    getrandom::fill(&mut b).expect("the OS has randomness");
    hex::encode(b)
}

fn v_dir(doc: &str) -> String {
    format!("{doc}.v")
}

fn l_dir(doc: &str) -> String {
    format!("{doc}.l")
}

fn check_doc(doc: &str) -> Result<()> {
    let ok = doc.len() == 16
        && doc.starts_with("DCP-")
        && doc[4..].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
    if ok {
        Ok(())
    } else {
        Err(StoreError::Invalid(format!("not a document id: {doc}")))
    }
}

/// A refused WRITE. Nextcloud answers `404` to a user whose share was removed,
/// the same as for a name that never existed, so for a write to a document the
/// caller already has an id for it is a refusal, not a missing document.
fn write_error(what: &str, r: &crate::dav::Resp) -> StoreError {
    if r.status == 404 {
        return StoreError::Forbidden(format!(
            "{what}: HTTP 404 (Nextcloud hides a document from a user who has no share of it)"
        ));
    }
    status_error(what, r)
}

/// (share permissions of `.v`, of `.l`) for a role.
pub fn share_permissions(role: Role) -> Option<(u32, u32)> {
    match role {
        Role::Viewer => Some((1, 1)),
        // read + create, and read + update + create + delete
        Role::Editor => Some((5, 15)),
        Role::Owner => Some((31, 31)),
        // Nextcloud's sharing has no commenter: comments are a separate app that
        // anyone with read access may use.
        Role::Commenter => None,
    }
}

#[derive(Debug, Clone)]
struct Share {
    id: String,
    share_type: i64,
    share_with: String,
    permissions: u32,
}

impl WebdavStore {
    pub async fn connect(cfg: &WebdavConfig) -> Result<WebdavStore> {
        let dav = Dav::new(&cfg.base_url, &cfg.user, &cfg.password);
        let clock = ServerClock::calibrate(&dav).await?;
        Ok(Self::with_parts(cfg, clock, Arc::new(AtomicBool::new(false))))
    }

    /// Share a calibrated clock and a fault flag between several handles.
    pub fn with_parts(cfg: &WebdavConfig, clock: Arc<ServerClock>, truncate_next: Arc<AtomicBool>) -> WebdavStore {
        WebdavStore {
            dav: Dav::new(&cfg.base_url, &cfg.user, &cfg.password),
            device: cfg.device.clone(),
            remote_scheme: cfg.remote_scheme.clone(),
            clock,
            truncate_next,
            seen_lease: Mutex::new(HashMap::new()),
            last_put_ms: AtomicU64::new(0),
        }
    }

    pub fn dav(&self) -> &Dav {
        &self.dav
    }

    pub fn clock(&self) -> &Arc<ServerClock> {
        &self.clock
    }

    /// Arm the fault: the next version upload by any handle sharing this flag is
    /// cut to half its bytes (and still answered `201` by Nextcloud).
    pub fn truncate_next_upload(&self) {
        self.truncate_next.store(true, Ordering::SeqCst);
    }

    fn new_version_id(&self) -> String {
        format!("{:012x}-{}", self.clock.now_ms(), rand_hex(4))
    }

    // ----------------------------------------------------------- lease log

    fn lease_name(seq: u64) -> String {
        format!("{seq:08}.json")
    }

    fn parse_lease(&self, r: &crate::dav::Resp, seq: u64) -> Result<Stamped<Lease>> {
        let file: LeaseFile = serde_json::from_slice(&r.body)
            .map_err(|e| StoreError::Invalid(format!("lease record {seq}: {e}")))?;
        schema::validate_typed(Kind::Lease, &file.lease).map_err(|e| StoreError::Invalid(e.to_string()))?;
        // The record's own time is a calibrated estimate; the server's
        // Last-Modified is the binding's stamp. Believe the estimate only if the
        // stamp agrees with it to within two seconds.
        let stamp = r.server_time_ms().unwrap_or(file.at_ms);
        let ts = if file.at_ms.abs_diff(stamp) < 2000 { file.at_ms } else { stamp };
        Ok(Stamped::new(format!("{seq:08}"), ts, file.by, file.lease))
    }

    /// The newest lease record, with its number.
    ///
    /// A handle that has read record `n` asks for `n + 1`, then `n + 2`, until the server
    /// says there is none, and then reads the newest record again: two requests when
    /// nothing changed. The first read of a document lists the folder. The newest record
    /// is read every time, not remembered, because creating a name is not atomic on
    /// Nextcloud: a creator that lost the race can replace the winner's record and be
    /// answered `204` (spike E, "The lease"), so a copy of the newest record, a handle's
    /// own write included, says nothing about what the log holds now. (A member whose
    /// share was removed gets `404` for `n + 1` and for `n`, so a removed member's
    /// `lease()` keeps answering with what it last saw.)
    async fn lease_read(&self, doc: &str) -> Result<Option<(u64, Stamped<Lease>)>> {
        let dir = l_dir(doc);
        let cached = self.seen_lease.lock().get(doc).cloned();
        // Whether the newest record in `cache` was read from the server by this call.
        let mut fresh = cached.is_none();
        let mut cache = match cached {
            Some(c) => c,
            None => {
                let (_, entries) = self
                    .dav
                    .propfind(&self.dav.files_path(&format!("{dir}/")), 1, &["d:getlastmodified"])
                    .await?;
                let newest = entries
                    .iter()
                    .filter_map(|e| {
                        let n = e.name();
                        n.strip_suffix(".json")
                            .filter(|s| s.len() == 8)
                            .and_then(|s| s.parse::<u64>().ok())
                    })
                    .max();
                match newest {
                    None => LeaseCache::default(),
                    Some(seq) => {
                        let r = self.dav.get(&format!("{dir}/{}", Self::lease_name(seq))).await?;
                        if !r.ok() {
                            return Err(status_error("read the lease", &r));
                        }
                        LeaseCache { seq: Some(seq), record: Some(self.parse_lease(&r, seq)?) }
                    }
                }
            }
        };
        loop {
            let next = cache.seq.map_or(0, |s| s + 1);
            let r = self.dav.get(&format!("{dir}/{}", Self::lease_name(next))).await?;
            match r.status {
                404 => break,
                200..=299 => {
                    cache = LeaseCache { seq: Some(next), record: Some(self.parse_lease(&r, next)?) };
                    fresh = true;
                }
                _ => return Err(status_error("read the lease", &r)),
            }
        }
        if let (false, Some(seq)) = (fresh, cache.seq) {
            let r = self.dav.get(&format!("{dir}/{}", Self::lease_name(seq))).await?;
            match r.status {
                // A removed member: what it last saw.
                404 => {}
                200..=299 => cache.record = Some(self.parse_lease(&r, seq)?),
                _ => return Err(status_error("read the lease", &r)),
            }
        }
        self.seen_lease.lock().insert(doc.to_owned(), cache.clone());
        Ok(cache.seq.zip(cache.record))
    }

    // -------------------------------------------------------------- shares

    /// `(shareType, shareWith)` for a principal `user@host[:port]`.
    fn share_target(&self, principal: &str) -> Result<(i64, String)> {
        let (user, host) = principal
            .rsplit_once('@')
            .ok_or_else(|| StoreError::Invalid(format!("not an address: {principal}")))?;
        if host == self.dav.host() {
            Ok((0, user.to_owned()))
        } else {
            Ok((6, format!("{user}@{}://{host}", self.remote_scheme)))
        }
    }

    fn same_sharee(&self, share: &Share, principal: &str) -> bool {
        let Some((user, host)) = principal.rsplit_once('@') else {
            return false;
        };
        let with = share
            .share_with
            .trim_start_matches("http://")
            .replace("@http://", "@")
            .replace("@https://", "@");
        if host == self.dav.host() {
            share.share_type == 0 && share.share_with == user
        } else {
            share.share_type == 6 && with == principal
        }
    }

    async fn list_shares(&self, dir: &str) -> Result<Vec<Share>> {
        let q = format!(
            "apps/files_sharing/api/v1/shares?path={}",
            percent_encode(&format!("/{dir}"))
        );
        let data = self.dav.ocs("GET", &q, &[]).await?;
        let arr = data.as_array().cloned().unwrap_or_default();
        Ok(arr
            .iter()
            .map(|s| Share {
                id: match &s["id"] {
                    serde_json::Value::String(x) => x.clone(),
                    other => other.to_string(),
                },
                share_type: s["share_type"].as_i64().unwrap_or(-1),
                share_with: s["share_with"].as_str().unwrap_or("").to_owned(),
                permissions: s["permissions"].as_u64().unwrap_or(0) as u32,
            })
            .collect())
    }

    /// The shares of the document by the current user, for the audit in tests.
    pub async fn share_permissions_of(&self, doc: &DocId, principal: &str) -> Result<Vec<(String, u32)>> {
        check_doc(doc)?;
        let mut out = Vec::new();
        for dir in [v_dir(doc), l_dir(doc)] {
            for s in self.list_shares(&dir).await? {
                if self.same_sharee(&s, principal) {
                    out.push((dir.clone(), s.permissions));
                }
            }
        }
        Ok(out)
    }

    /// What a hostile editor's client does: write a version object straight into
    /// the document folder, skipping every check this store makes. `Ok(Some(name))`
    /// if Nextcloud accepted it, `Ok(None)` if it refused it for lack of permission.
    pub async fn put_raw_version(&self, doc: &DocId, name: &str, version: &Version) -> Result<Option<String>> {
        check_doc(doc)?;
        let body = serde_json::to_vec(version).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let r = self.dav.put_new(&format!("{}/{name}.json", v_dir(doc)), body).await?;
        match r.status {
            201 | 204 => Ok(Some(name.to_owned())),
            401 | 403 | 404 => Ok(None),
            _ => Err(status_error("raw version", &r)),
        }
    }
}

#[async_trait]
impl DocumentStore for WebdavStore {
    fn principal(&self) -> String {
        self.dav.cloud_id()
    }

    fn device(&self) -> String {
        self.device.clone()
    }

    async fn create_document(&self, meta: &Meta, first: Option<NewFile>) -> Result<DocId> {
        schema::validate_typed(Kind::Meta, meta).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let doc = format!("DCP-{}", rand_hex(6));
        for dir in [v_dir(&doc), l_dir(&doc)] {
            let r = self.dav.mkcol(&dir).await?;
            if r.status != 201 {
                return Err(status_error("MKCOL", &r));
            }
        }
        let body = serde_json::to_vec(meta).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let r = self.dav.put_new(&format!("{}/meta.json", v_dir(&doc)), body).await?;
        if !r.ok() {
            return Err(status_error("write meta.json", &r));
        }
        if let Some(file) = first {
            self.publish_version(&doc, Vec::new(), file, None).await?;
        }
        Ok(doc)
    }

    async fn meta(&self, doc: &DocId) -> Result<Meta> {
        check_doc(doc)?;
        let r = self.dav.get(&format!("{}/meta.json", v_dir(doc))).await?;
        if !r.ok() {
            return Err(status_error("read meta.json", &r));
        }
        let raw: serde_json::Value =
            serde_json::from_slice(&r.body).map_err(|e| StoreError::Invalid(e.to_string()))?;
        schema::parse(Kind::Meta, &raw).map_err(|e| StoreError::Invalid(e.to_string()))
    }

    async fn role(&self, doc: &DocId) -> Result<Role> {
        check_doc(doc)?;
        let (_, entries) = self
            .dav
            .propfind(
                &self.dav.files_path(&format!("{}/", v_dir(doc))),
                0,
                &["oc:permissions", "oc:owner-id", "ocm:share-permissions"],
            )
            .await?;
        let e = entries
            .first()
            .ok_or_else(|| StoreError::NotFound(doc.clone()))?;
        let owner = e.get(OC, "owner-id").unwrap_or("");
        if owner == self.dav.user {
            return Ok(Role::Owner);
        }
        // A federated mount names its owner by cloud id (`alice@nc-a`). The
        // receiving server always shows it the share right (`R`) and a full set of
        // write rights, whatever the sender granted: only the OCM permission list
        // says read or write. Owner and create-only are not expressible there.
        if owner.contains('@') {
            let list = e.get("http://open-cloud-mesh.org/ns", "share-permissions").unwrap_or("");
            return Ok(if list.contains("\"write\"") { Role::Editor } else { Role::Viewer });
        }
        // On a mount root: R = may reshare, C = may create files. A viewer has
        // neither (its D and N are the right to unmount).
        let perms = e.get(OC, "permissions").unwrap_or("");
        Ok(if perms.contains('R') {
            Role::Owner
        } else if perms.contains('C') {
            Role::Editor
        } else {
            Role::Viewer
        })
    }

    async fn grant(&self, doc: &DocId, principal: &str, role: Role) -> Result<()> {
        check_doc(doc)?;
        let (pv, pl) = share_permissions(role).ok_or_else(|| {
            StoreError::Invalid("Nextcloud sharing has no commenter role".into())
        })?;
        if self.role(doc).await? != Role::Owner {
            return Err(StoreError::Forbidden("only an owner changes roles".into()));
        }
        let (share_type, with) = self.share_target(principal)?;
        let share_type = share_type.to_string();
        for (dir, perms) in [(v_dir(doc), pv), (l_dir(doc), pl)] {
            let perms = perms.to_string();
            let existing = self
                .list_shares(&dir)
                .await?
                .into_iter()
                .find(|s| self.same_sharee(s, principal));
            match existing {
                Some(s) if s.permissions.to_string() == perms => {}
                Some(s) => {
                    self.dav
                        .ocs(
                            "PUT",
                            &format!("apps/files_sharing/api/v1/shares/{}", s.id),
                            &[("permissions", perms.as_str())],
                        )
                        .await?;
                }
                None => {
                    let path = format!("/{dir}");
                    self.dav
                        .ocs(
                            "POST",
                            "apps/files_sharing/api/v1/shares",
                            &[
                                ("path", path.as_str()),
                                ("shareType", share_type.as_str()),
                                ("shareWith", with.as_str()),
                                ("permissions", perms.as_str()),
                            ],
                        )
                        .await?;
                }
            }
        }
        Ok(())
    }

    async fn revoke(&self, doc: &DocId, principal: &str) -> Result<()> {
        check_doc(doc)?;
        if self.role(doc).await? != Role::Owner {
            return Err(StoreError::Forbidden("only an owner removes a member".into()));
        }
        let mut removed = 0;
        for dir in [v_dir(doc), l_dir(doc)] {
            for s in self.list_shares(&dir).await? {
                if self.same_sharee(&s, principal) {
                    self.dav
                        .ocs("DELETE", &format!("apps/files_sharing/api/v1/shares/{}", s.id), &[])
                        .await?;
                    removed += 1;
                }
            }
        }
        if removed == 0 {
            return Err(StoreError::NotFound(format!("{principal} has no share of {doc}")));
        }
        Ok(())
    }

    async fn versions(&self, doc: &DocId) -> Result<Vec<Stamped<Version>>> {
        check_doc(doc)?;
        let dir = v_dir(doc);
        let (_, entries) = self
            .dav
            .propfind(
                &self.dav.files_path(&format!("{dir}/")),
                1,
                &["d:getlastmodified", "d:getcontentlength"],
            )
            .await?;
        let names: Vec<(String, u64)> = entries
            .iter()
            .filter(|e| !e.is_collection())
            .filter_map(|e| {
                let n = e.name();
                let stem = n.strip_suffix(".json")?.to_owned();
                if stem == "meta" {
                    return None;
                }
                let ts = e
                    .get(DAV, "getlastmodified")
                    .and_then(|v| httpdate::parse_http_date(v).ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                Some((stem, ts))
            })
            .collect();
        let mut out = Vec::with_capacity(names.len());
        for chunk in names.chunks(8) {
            let mut set = tokio::task::JoinSet::new();
            for (stem, ts) in chunk.iter().cloned() {
                let dav = self.dav.clone();
                let path = format!("{dir}/{stem}.json");
                set.spawn(async move { (stem, ts, dav.get(&path).await) });
            }
            while let Some(joined) = set.join_next().await {
                let (stem, ts, resp) =
                    joined.map_err(|e| StoreError::Transport(format!("join: {e}")))?;
                let resp = resp?;
                if resp.status == 404 {
                    continue;
                }
                if !resp.ok() {
                    return Err(status_error(&format!("read version {stem}"), &resp));
                }
                // A client MUST ignore an object that does not parse or validate.
                let Ok(raw) = serde_json::from_slice::<serde_json::Value>(&resp.body) else {
                    continue;
                };
                let Ok(version) = schema::parse::<Version>(Kind::Version, &raw) else {
                    continue;
                };
                let sender = version.created_by.principal.clone();
                let device = version.created_by.device.clone();
                out.push(Stamped::new(stem, ts, sender, version).with_device(device));
            }
        }
        Ok(out)
    }

    async fn fetch_file(&self, doc: &DocId, version: &Stamped<Version>) -> Result<Vec<u8>> {
        check_doc(doc)?;
        let r = self
            .dav
            .get(&format!("{}/{}.bin", v_dir(doc), version.id))
            .await?;
        if !r.ok() {
            return Err(status_error(&format!("fetch {}", version.id), &r));
        }
        Ok(r.body)
    }

    async fn publish_version(
        &self,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
    ) -> Result<Stamped<Version>> {
        check_doc(doc)?;
        let dir = v_dir(doc);
        let id = self.new_version_id();

        // 1. Upload the whole file. Nextcloud answers 201 to whatever arrived.
        let mut sent = file.bytes.clone();
        if self.truncate_next.swap(false, Ordering::SeqCst) {
            sent.truncate(sent.len() / 2);
        }
        let r = self.dav.put_new(&format!("{dir}/{id}.bin"), sent).await?;
        if !r.ok() {
            return Err(write_error("upload", &r));
        }

        // 2. Verify that the stored bytes are the bytes (version rule 2). The
        // upload's own status says nothing about that: OC-Checksum is stored
        // as claimed, not checked, so the file is read back.
        let back = self.dav.get(&format!("{dir}/{id}.bin")).await?;
        if !back.ok() || back.body != file.bytes {
            // Best effort: a create-only editor may not delete its own upload,
            // which leaves an unreferenced file that no reader lists.
            let _ = self.dav.delete(&format!("{dir}/{id}.bin")).await;
            return Err(StoreError::Transport(format!(
                "the stored file differs from the upload ({} of {} bytes); nothing was published",
                back.body.len(),
                file.bytes.len()
            )));
        }

        // 3. Publish: the version object is created last. A reader that lists the
        // folder between 1 and 3 sees no version, only an orphan file.
        let href = format!(
            "https://{}/remote.php/dav/files/{}/{dir}/{id}.bin",
            self.dav.host(),
            self.dav.user
        );
        let version = Version {
            dcp: 1,
            parents,
            file: integrity::file_ref(&file.mime, &file.bytes, Locator::Webdav { href }),
            created_by: dcp_core::Author {
                principal: self.principal(),
                device: self.device(),
            },
            message,
            covers: None,
        };
        schema::validate_typed(Kind::Version, &version)
            .map_err(|e| StoreError::Invalid(e.to_string()))?;
        let body = serde_json::to_vec(&version).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let r = self.dav.put_new(&format!("{dir}/{id}.json"), body).await?;
        if !r.ok() {
            return Err(write_error("publish the version", &r));
        }
        let ts = r.server_time_ms().unwrap_or_else(|| self.clock.now_ms());
        Ok(Stamped::new(id, ts, self.principal(), version).with_device(self.device()))
    }

    async fn lease(&self, doc: &DocId) -> Result<Option<Stamped<Lease>>> {
        check_doc(doc)?;
        Ok(self.lease_read(doc).await?.map(|(_, l)| l))
    }

    async fn write_lease(&self, doc: &DocId, lease: Lease) -> Result<Stamped<Lease>> {
        check_doc(doc)?;
        schema::validate_typed(Kind::Lease, &lease).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let known = self.seen_lease.lock().get(doc.as_str()).map(|c| c.seq);
        let seen = match known {
            Some(s) => s,
            None => self.lease_read(doc).await?.map(|(s, _)| s),
        };
        let next = seen.map_or(0, |s| s + 1);
        let at_ms = self.clock.now_ms() + self.last_put_ms.load(Ordering::Relaxed) / 2;
        let file = LeaseFile {
            by: self.principal(),
            at_ms,
            lease: lease.clone(),
        };
        let body = serde_json::to_vec(&file).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let r = self
            .dav
            .put_new(&format!("{}/{}", l_dir(doc), Self::lease_name(next)), body)
            .await?;
        self.last_put_ms.store(r.elapsed.as_millis() as u64, Ordering::Relaxed);
        match r.status {
            // `204` on a create-only `PUT` is a creator that replaced a record another
            // creator had just made: its write landed, the read-backs say who holds it.
            201 | 204 => {
                let record = Stamped::new(format!("{next:08}"), at_ms, self.principal(), lease);
                self.seen_lease.lock().insert(
                    doc.clone(),
                    LeaseCache { seq: Some(next), record: Some(record.clone()) },
                );
                Ok(record)
            }
            412 | 423 => Err(StoreError::Conflict(format!(
                "lease record {next} was taken first (HTTP {})",
                r.status
            ))),
            _ => Err(write_error("write the lease", &r)),
        }
    }

    async fn now_ms(&self, _doc: &DocId) -> Result<u64> {
        Ok(self.clock.now_ms())
    }

    async fn publish_update(&self, _doc: &DocId, _update: Update) -> Result<Stamped<Update>> {
        Err(StoreError::Invalid(
            "L2: Nextcloud WebDAV has no ordered update stream (RFC-0004 spike E)".into(),
        ))
    }

    async fn updates(&self, _doc: &DocId) -> Result<Vec<Stamped<Update>>> {
        Ok(Vec::new())
    }
}
