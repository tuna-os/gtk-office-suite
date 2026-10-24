//! Test support: a small in-test `DocumentStore` (to be swapped for
//! `dcp_core::MemoryStore` once that lands), an ODF builder, and a rig that
//! starts a WopiHost on a loopback port.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use dcp_core::*;
use dcp_wopi::{HostConfig, RunningHost, WopiHost};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};

pub const ODT_MIME: &str = "application/vnd.oasis.opendocument.text";

/// A minimal but real ODT package (mimetype first and stored, manifest,
/// content.xml) holding `text` in one paragraph.
pub fn make_odt(text: &str) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        z.start_file("mimetype", stored).unwrap();
        z.write_all(ODT_MIME.as_bytes()).unwrap();
        z.start_file("META-INF/manifest.xml", deflated).unwrap();
        z.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">
 <manifest:file-entry manifest:full-path="/" manifest:version="1.2" manifest:media-type="application/vnd.oasis.opendocument.text"/>
 <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
</manifest:manifest>"#,
        )
        .unwrap();
        z.start_file("content.xml", deflated).unwrap();
        write!(
            z,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2">
 <office:body><office:text><text:p>{text}</text:p></office:text></office:body>
</office:document-content>"#
        )
        .unwrap();
        z.finish().unwrap();
    }
    buf.into_inner()
}

pub fn content_xml(odt: &[u8]) -> String {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(odt)).expect("ODT is a zip");
    let mut s = String::new();
    z.by_name("content.xml").expect("content.xml").read_to_string(&mut s).unwrap();
    s
}

pub fn meta(title: &str) -> Meta {
    Meta {
        dcp: 0,
        title: title.to_string(),
        mime: ODT_MIME.to_string(),
        levels: vec![Level::L0, Level::L1],
        algorithms: None,
        editor_services: None,
    }
}

// ---- local store ----------------------------------------------------------

#[derive(Default)]
struct DocState {
    meta: Option<Meta>,
    roles: HashMap<String, Role>,
    versions: Vec<Stamped<Version>>,
    files: HashMap<String, Vec<u8>>,
    lease: Option<Stamped<Lease>>,
    last_ts: u64,
}

#[derive(Default)]
struct State {
    docs: HashMap<DocId, DocState>,
    next: u64,
}

pub struct Backend {
    state: Mutex<State>,
    skew_ms: AtomicU64,
}

impl Backend {
    pub fn new() -> Arc<Backend> {
        Arc::new(Backend { state: Mutex::new(State::default()), skew_ms: AtomicU64::new(0) })
    }

    /// Move the binding's clock forward (lease expiry tests).
    pub fn advance(&self, ms: u64) {
        self.skew_ms.fetch_add(ms, Ordering::SeqCst);
    }

    fn now(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + self.skew_ms.load(Ordering::SeqCst)
    }

    pub fn handle(self: &Arc<Self>, principal: &str) -> Arc<Handle> {
        Arc::new(Handle { backend: self.clone(), principal: principal.to_string() })
    }

    /// Flip one byte of a stored file, as a corrupt blob would.
    pub fn corrupt(&self, doc: &str, version: &str) {
        let mut s = self.state.lock();
        let f = s.docs.get_mut(doc).unwrap().files.get_mut(version).unwrap();
        let n = f.len() / 2;
        f[n] ^= 0xFF;
    }
}

pub struct Handle {
    backend: Arc<Backend>,
    principal: String,
}

impl Handle {
    fn role_in(&self, d: &DocState) -> Option<Role> {
        d.roles.get(&self.principal).copied()
    }
}

fn file_ref(mime: &str, bytes: &[u8], id: &str) -> FileRef {
    FileRef {
        mime: mime.to_string(),
        size: bytes.len() as u64,
        sha256: hex::encode(Sha256::digest(bytes)),
        locator: Locator::Url { href: format!("https://store.invalid/{id}") },
    }
}

#[async_trait]
impl DocumentStore for Handle {
    fn principal(&self) -> String {
        self.principal.clone()
    }
    fn device(&self) -> String {
        format!("DEV-{}", self.principal)
    }

    async fn create_document(&self, meta: &Meta, first: Option<NewFile>) -> Result<DocId> {
        let now = self.backend.now();
        let mut s = self.backend.state.lock();
        s.next += 1;
        let id = format!("doc{}", s.next);
        let mut d = DocState { meta: Some(meta.clone()), last_ts: now, ..Default::default() };
        d.roles.insert(self.principal.clone(), Role::Owner);
        if let Some(f) = first {
            let vid = format!("{id}-v1");
            let body = Version {
                dcp: 0,
                parents: vec![],
                file: file_ref(&f.mime, &f.bytes, &vid),
                created_by: Author { principal: self.principal.clone(), device: self.device() },
                message: None,
                covers: None,
            };
            d.versions.push(Stamped::new(vid.clone(), now, self.principal.clone(), body));
            d.files.insert(vid, f.bytes);
        }
        s.docs.insert(id.clone(), d);
        Ok(id)
    }

    async fn meta(&self, doc: &DocId) -> Result<Meta> {
        let s = self.backend.state.lock();
        let d = s.docs.get(doc).ok_or_else(|| StoreError::NotFound(doc.clone()))?;
        self.role_in(d).ok_or_else(|| StoreError::Forbidden("not a member".into()))?;
        Ok(d.meta.clone().unwrap())
    }

    async fn role(&self, doc: &DocId) -> Result<Role> {
        let s = self.backend.state.lock();
        let d = s.docs.get(doc).ok_or_else(|| StoreError::NotFound(doc.clone()))?;
        self.role_in(d).ok_or_else(|| StoreError::Forbidden("not a member".into()))
    }

    async fn grant(&self, doc: &DocId, principal: &str, role: Role) -> Result<()> {
        let mut s = self.backend.state.lock();
        let d = s.docs.get_mut(doc).ok_or_else(|| StoreError::NotFound(doc.clone()))?;
        if self.role_in(d) != Some(Role::Owner) {
            return Err(StoreError::Forbidden("only the owner grants".into()));
        }
        d.roles.insert(principal.to_string(), role);
        Ok(())
    }

    async fn revoke(&self, doc: &DocId, principal: &str) -> Result<()> {
        let mut s = self.backend.state.lock();
        let d = s.docs.get_mut(doc).ok_or_else(|| StoreError::NotFound(doc.clone()))?;
        if self.role_in(d) != Some(Role::Owner) {
            return Err(StoreError::Forbidden("only the owner revokes".into()));
        }
        d.roles.remove(principal);
        Ok(())
    }

    async fn versions(&self, doc: &DocId) -> Result<Vec<Stamped<Version>>> {
        let s = self.backend.state.lock();
        let d = s.docs.get(doc).ok_or_else(|| StoreError::NotFound(doc.clone()))?;
        self.role_in(d).ok_or_else(|| StoreError::Forbidden("not a member".into()))?;
        Ok(d.versions.clone())
    }

    async fn fetch_file(&self, doc: &DocId, version: &Stamped<Version>) -> Result<Vec<u8>> {
        let s = self.backend.state.lock();
        let d = s.docs.get(doc).ok_or_else(|| StoreError::NotFound(doc.clone()))?;
        self.role_in(d).ok_or_else(|| StoreError::Forbidden("not a member".into()))?;
        d.files.get(&version.id).cloned().ok_or_else(|| StoreError::NotFound(version.id.clone()))
    }

    async fn publish_version(
        &self,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
    ) -> Result<Stamped<Version>> {
        let now = self.backend.now();
        let mut s = self.backend.state.lock();
        s.next += 1;
        let n = s.next;
        let d = s.docs.get_mut(doc).ok_or_else(|| StoreError::NotFound(doc.clone()))?;
        if self.role_in(d) < Some(Role::Editor) {
            return Err(StoreError::Forbidden("viewers and commenters cannot publish".into()));
        }
        for p in &parents {
            if !d.versions.iter().any(|v| &v.id == p) {
                return Err(StoreError::Invalid(format!("unknown parent {p}")));
            }
        }
        let id = format!("{doc}-v{n}");
        d.last_ts = now.max(d.last_ts + 1);
        let body = Version {
            dcp: 0,
            parents,
            file: file_ref(&file.mime, &file.bytes, &id),
            created_by: Author { principal: self.principal.clone(), device: self.device() },
            message,
            covers: None,
        };
        let st = Stamped::new(id.clone(), d.last_ts, self.principal.clone(), body);
        d.versions.push(st.clone());
        d.files.insert(id, file.bytes);
        Ok(st)
    }

    async fn lease(&self, doc: &DocId) -> Result<Option<Stamped<Lease>>> {
        let s = self.backend.state.lock();
        let d = s.docs.get(doc).ok_or_else(|| StoreError::NotFound(doc.clone()))?;
        self.role_in(d).ok_or_else(|| StoreError::Forbidden("not a member".into()))?;
        Ok(d.lease.clone())
    }

    async fn write_lease(&self, doc: &DocId, lease: Lease) -> Result<Stamped<Lease>> {
        let now = self.backend.now();
        let mut s = self.backend.state.lock();
        let d = s.docs.get_mut(doc).ok_or_else(|| StoreError::NotFound(doc.clone()))?;
        if self.role_in(d) < Some(Role::Editor) {
            return Err(StoreError::Forbidden("viewers cannot write leases".into()));
        }
        d.last_ts = now.max(d.last_ts + 1);
        let st = Stamped::new(format!("lease-{}", d.last_ts), d.last_ts, self.principal.clone(), lease);
        d.lease = Some(st.clone());
        Ok(st)
    }

    async fn now_ms(&self, _doc: &DocId) -> Result<u64> {
        Ok(self.backend.now())
    }

    async fn publish_update(&self, _doc: &DocId, _u: Update) -> Result<Stamped<Update>> {
        Err(StoreError::Invalid("no updates in the test store".into()))
    }

    async fn updates(&self, _doc: &DocId) -> Result<Vec<Stamped<Update>>> {
        Ok(vec![])
    }
}

// ---- rig ---------------------------------------------------------------------

pub struct Rig {
    pub backend: Arc<Backend>,
    pub host: WopiHost,
    pub server: RunningHost,
    pub doc: DocId,
    pub v1: String,
    pub owner: Arc<Handle>,
}

impl Rig {
    pub async fn new() -> Rig {
        Self::with_config(HostConfig { log_stderr: false, ..HostConfig::default() }).await
    }

    pub async fn with_config(cfg: HostConfig) -> Rig {
        let backend = Backend::new();
        let owner = backend.handle("@alice:test");
        let doc = owner
            .create_document(&meta("Budget"), Some(NewFile { mime: ODT_MIME.into(), bytes: make_odt("base") }))
            .await
            .unwrap();
        let v1 = owner.versions(&doc).await.unwrap()[0].id.clone();
        let host = WopiHost::new(cfg);
        let server = host.serve("127.0.0.1:0".parse().unwrap()).await.unwrap();
        Rig { backend, host, server, doc, v1, owner }
    }

    /// A token for `who` (the principal must already have a role on the doc).
    pub fn token(&self, who: &str) -> String {
        self.host.register(self.doc.clone(), self.backend.handle(who), who)
    }

    pub fn url(&self, contents: bool) -> String {
        format!(
            "{}/wopi/files/{}{}",
            self.server.base_url(),
            self.doc,
            if contents { "/contents" } else { "" }
        )
    }
}

pub fn http() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}
