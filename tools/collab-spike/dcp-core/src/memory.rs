//! The in-memory reference implementation of [`DocumentStore`].
//!
//! One [`MemoryBackend`] holds every document; each principal gets its own
//! [`MemoryStore`] handle on it. Roles are enforced here, in the store, not
//! by callers (spec conformance test 6). The clock is injectable and the
//! event identifiers are deterministic (`$ev000001`, ...), so a failing
//! journey replays identically.
//!
//! Revocation model (spec 11): every published item records who could read it
//! at that moment. A revoked principal keeps what they were already given and
//! can read nothing published after. Never-members read nothing.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::{Mutex, MutexGuard};

use crate::graph;
use crate::integrity;
use crate::lease;
use crate::schema::{self, Kind};
use crate::{
    Author, DocId, DocumentStore, Lease, LeaseState, Locator, Meta, NewFile, Result, Role, Stamped, StoreError,
    Update, Version,
};

/// The time source of a backend, in milliseconds. This is the "binding's own
/// clock" of lease rule 1.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// A clock that only moves when told to.
#[derive(Debug)]
pub struct ManualClock(AtomicU64);

impl ManualClock {
    pub fn new(start_ms: u64) -> Self {
        ManualClock(AtomicU64::new(start_ms))
    }

    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }

    pub fn set(&self, ms: u64) {
        self.0.store(ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// Wall-clock time.
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MemoryConfig {
    /// `true`: a lease write that would take an active lease from another
    /// session is refused with `Conflict` (a binding with compare-and-set).
    /// `false`: every write by an editor succeeds and the last one wins, like
    /// a Matrix state event; only the read-back (lease rule 3) resolves it.
    pub cas_leases: bool,
    /// `false` skips the read-back of uploaded bytes (version rule 2). This
    /// is a deliberate defect for mutation tests, never a sensible setting.
    pub verify_uploads: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        MemoryConfig { cas_leases: true, verify_uploads: true }
    }
}

#[derive(Debug, Clone)]
struct Item<T> {
    stamped: Stamped<T>,
    /// Who could read it when it was published.
    readers: BTreeSet<String>,
}

#[derive(Debug)]
struct Doc {
    meta: Meta,
    members: BTreeMap<String, Role>,
    /// Everyone who has ever been a member. A first-time member reads the
    /// whole history (`history_visibility: shared`, spec 8.1); someone who
    /// was removed and comes back does not get what was published meanwhile.
    ever_members: BTreeSet<String>,
    versions: Vec<Item<Version>>,
    updates: Vec<Item<Update>>,
    lease: Option<Stamped<Lease>>,
}

#[derive(Default)]
struct State {
    docs: BTreeMap<DocId, Doc>,
    /// Uploaded file bytes by locator href.
    files: HashMap<String, Vec<u8>>,
    next_event: u64,
    next_doc: u64,
    truncate_next_upload: bool,
}

struct Inner {
    clock: Arc<dyn Clock>,
    config: MemoryConfig,
    state: Mutex<State>,
}

/// All documents of one in-memory "server".
#[derive(Clone)]
pub struct MemoryBackend {
    inner: Arc<Inner>,
}

impl MemoryBackend {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self::with_config(clock, MemoryConfig::default())
    }

    pub fn with_config(clock: Arc<dyn Clock>, config: MemoryConfig) -> Self {
        MemoryBackend { inner: Arc::new(Inner { clock, config, state: Mutex::new(State::default()) }) }
    }

    /// A handle that acts as `principal` from `device`.
    pub fn store(&self, principal: &str, device: &str) -> MemoryStore {
        MemoryStore { backend: self.clone(), principal: principal.to_owned(), device: device.to_owned() }
    }

    /// Fault injection: the next upload is cut in half on its way to storage.
    pub fn truncate_next_upload(&self) {
        self.lock().truncate_next_upload = true;
    }

    pub fn config(&self) -> MemoryConfig {
        self.inner.config
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock()
    }
}

/// One principal's handle on a [`MemoryBackend`].
#[derive(Clone)]
pub struct MemoryStore {
    backend: MemoryBackend,
    principal: String,
    device: String,
}

fn not_found(doc: &str) -> StoreError {
    StoreError::NotFound(format!("document {doc}"))
}

impl State {
    fn event_id(&mut self) -> String {
        self.next_event += 1;
        format!("$ev{:06}:mem", self.next_event)
    }

    fn doc(&self, doc: &str) -> Result<&Doc> {
        self.docs.get(doc).ok_or_else(|| not_found(doc))
    }

    fn doc_mut(&mut self, doc: &str) -> Result<&mut Doc> {
        self.docs.get_mut(doc).ok_or_else(|| not_found(doc))
    }
}

impl Doc {
    fn role_of(&self, principal: &str) -> Result<Role> {
        self.members
            .get(principal)
            .copied()
            .ok_or_else(|| StoreError::Forbidden(format!("{principal} is not a member")))
    }

    fn require(&self, principal: &str, at_least: Role, what: &str) -> Result<Role> {
        let role = self.role_of(principal)?;
        if role < at_least {
            return Err(StoreError::Forbidden(format!("{principal} is {role:?}; {what} needs {at_least:?}")));
        }
        Ok(role)
    }

    fn visible_versions(&self, principal: &str) -> Vec<Stamped<Version>> {
        self.versions.iter().filter(|i| i.readers.contains(principal)).map(|i| i.stamped.clone()).collect()
    }
}

impl MemoryStore {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.backend.lock()
    }

    fn now(&self) -> u64 {
        self.backend.inner.clock.now_ms()
    }

    fn stamp<T>(&self, id: String, body: T) -> Stamped<T> {
        Stamped::new(id, self.now(), self.principal.clone(), body).with_device(self.device.clone())
    }

    /// Version rule 2 and the role check, shared by `create_document` and
    /// `publish_version`. The caller holds the lock.
    fn publish_locked(
        &self,
        state: &mut State,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
    ) -> Result<Stamped<Version>> {
        let config = self.backend.inner.config;
        {
            let d = state.doc(doc)?;
            d.require(&self.principal, Role::Editor, "publishing a version")?;
            graph::check_new_parents(&d.visible_versions(&self.principal), &parents)
                .map_err(|e| StoreError::Invalid(format!("parents: {e}")))?;
        }
        let truncate = std::mem::take(&mut state.truncate_next_upload);
        let id = state.event_id();
        let href = format!("https://mem.invalid/{doc}/{id}");
        // Upload, read the stored bytes back, check them against the
        // reference built from what the caller meant to upload.
        let file_ref = integrity::file_ref(&file.mime, &file.bytes, Locator::Url { href: href.clone() });
        let mut stored = file.bytes;
        if truncate {
            stored.truncate(stored.len() / 2);
        }
        if config.verify_uploads {
            integrity::verify(&file_ref, &stored)
                .map_err(|e| StoreError::Transport(format!("upload failed verification, not published: {e}")))?;
        }
        let version = Version {
            dcp: 0,
            parents,
            file: file_ref,
            created_by: Author { principal: self.principal.clone(), device: self.device.clone() },
            message,
            covers: None,
        };
        schema::validate_typed(Kind::Version, &version).map_err(|e| StoreError::Invalid(e.to_string()))?;
        state.files.insert(href, stored);
        let stamped = self.stamp(id, version);
        let d = state.doc_mut(doc)?;
        let readers = d.members.keys().cloned().collect();
        d.versions.push(Item { stamped: stamped.clone(), readers });
        Ok(stamped)
    }
}

#[async_trait]
impl DocumentStore for MemoryStore {
    fn principal(&self) -> String {
        self.principal.clone()
    }

    fn device(&self) -> String {
        self.device.clone()
    }

    async fn create_document(&self, meta: &Meta, first: Option<NewFile>) -> Result<DocId> {
        schema::validate_typed(Kind::Meta, meta).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let mut state = self.lock();
        state.next_doc += 1;
        let id: DocId = format!("!doc{:03}:mem", state.next_doc);
        state.docs.insert(
            id.clone(),
            Doc {
                meta: meta.clone(),
                members: BTreeMap::from([(self.principal.clone(), Role::Owner)]),
                ever_members: BTreeSet::from([self.principal.clone()]),
                versions: Vec::new(),
                updates: Vec::new(),
                lease: None,
            },
        );
        if let Some(file) = first {
            if let Err(e) = self.publish_locked(&mut state, &id, Vec::new(), file, None) {
                state.docs.remove(&id);
                return Err(e);
            }
        }
        Ok(id)
    }

    async fn meta(&self, doc: &DocId) -> Result<Meta> {
        let state = self.lock();
        let d = state.doc(doc)?;
        d.role_of(&self.principal)?;
        Ok(d.meta.clone())
    }

    async fn role(&self, doc: &DocId) -> Result<Role> {
        self.lock().doc(doc)?.role_of(&self.principal)
    }

    async fn grant(&self, doc: &DocId, principal: &str, role: Role) -> Result<()> {
        let mut state = self.lock();
        let d = state.doc_mut(doc)?;
        d.require(&self.principal, Role::Owner, "changing roles")?;
        if d.ever_members.insert(principal.to_owned()) {
            for readers in d.versions.iter_mut().map(|i| &mut i.readers).chain(d.updates.iter_mut().map(|i| &mut i.readers)) {
                readers.insert(principal.to_owned());
            }
        }
        d.members.insert(principal.to_owned(), role);
        Ok(())
    }

    async fn revoke(&self, doc: &DocId, principal: &str) -> Result<()> {
        let mut state = self.lock();
        let d = state.doc_mut(doc)?;
        d.require(&self.principal, Role::Owner, "removing a member")?;
        if principal == self.principal {
            return Err(StoreError::Invalid("an owner cannot remove themselves".into()));
        }
        d.members.remove(principal);
        Ok(())
    }

    async fn versions(&self, doc: &DocId) -> Result<Vec<Stamped<Version>>> {
        let state = self.lock();
        let d = state.doc(doc)?;
        let seen = d.visible_versions(&self.principal);
        if seen.is_empty() && !d.members.contains_key(&self.principal) {
            return Err(StoreError::Forbidden(format!("{} has no access", self.principal)));
        }
        Ok(seen)
    }

    async fn fetch_file(&self, doc: &DocId, version: &Stamped<Version>) -> Result<Vec<u8>> {
        let state = self.lock();
        let d = state.doc(doc)?;
        let item = d
            .versions
            .iter()
            .find(|i| i.stamped.id == version.id && i.readers.contains(&self.principal))
            .ok_or_else(|| StoreError::NotFound(format!("version {} (not readable by {})", version.id, self.principal)))?;
        let Locator::Url { href } = &item.stamped.body.file.locator else {
            return Err(StoreError::Invalid("memory store only holds url locators".into()));
        };
        state.files.get(href).cloned().ok_or_else(|| StoreError::NotFound(href.clone()))
    }

    async fn publish_version(
        &self,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
    ) -> Result<Stamped<Version>> {
        let mut state = self.lock();
        self.publish_locked(&mut state, doc, parents, file, message)
    }

    async fn lease(&self, doc: &DocId) -> Result<Option<Stamped<Lease>>> {
        let state = self.lock();
        let d = state.doc(doc)?;
        d.role_of(&self.principal)?;
        Ok(d.lease.clone())
    }

    async fn write_lease(&self, doc: &DocId, record: Lease) -> Result<Stamped<Lease>> {
        schema::validate_typed(Kind::Lease, &record).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let mut state = self.lock();
        let id = state.event_id();
        let now = self.now();
        let cas = self.backend.inner.config.cas_leases;
        let d = state.doc_mut(doc)?;
        let role = d.require(&self.principal, Role::Editor, "writing a lease")?;
        if record.state == LeaseState::Held {
            let holder = record.holder.as_ref().expect("schema: held lease has a holder");
            if holder.principal != self.principal {
                return Err(StoreError::Forbidden(format!(
                    "{} cannot write a lease held by {}",
                    self.principal, holder.principal
                )));
            }
        }
        if cas && role != Role::Owner {
            if let Some(current) = &d.lease {
                if lease::is_active(current, now) {
                    let current_holder = current.body.holder.as_ref().expect("held lease has a holder");
                    let same_session = match &record.holder {
                        Some(h) => h == current_holder,
                        // a release carries no holder: the principal decides
                        None => current_holder.principal == self.principal,
                    };
                    if !same_session {
                        return Err(StoreError::Conflict(format!(
                            "lease is held by {}/{}",
                            current_holder.principal, current_holder.session
                        )));
                    }
                }
            }
        }
        let stamped = self.stamp(id, record);
        d.lease = Some(stamped.clone());
        Ok(stamped)
    }

    async fn now_ms(&self, doc: &DocId) -> Result<u64> {
        self.lock().doc(doc)?;
        Ok(self.now())
    }

    async fn publish_update(&self, doc: &DocId, update: Update) -> Result<Stamped<Update>> {
        schema::validate_typed(Kind::Update, &update).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let mut state = self.lock();
        let id = state.event_id();
        let d = state.doc_mut(doc)?;
        d.require(&self.principal, Role::Editor, "publishing an update")?;
        let stamped = self.stamp(id, update);
        let readers = d.members.keys().cloned().collect();
        d.updates.push(Item { stamped: stamped.clone(), readers });
        Ok(stamped)
    }

    async fn updates(&self, doc: &DocId) -> Result<Vec<Stamped<Update>>> {
        let state = self.lock();
        let d = state.doc(doc)?;
        if !d.members.contains_key(&self.principal) && !d.updates.iter().any(|i| i.readers.contains(&self.principal)) {
            return Err(StoreError::Forbidden(format!("{} has no access", self.principal)));
        }
        Ok(d.updates.iter().filter(|i| i.readers.contains(&self.principal)).map(|i| i.stamped.clone()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Level, Payload};

    fn meta() -> Meta {
        Meta {
            dcp: 0,
            title: "Doc".into(),
            mime: "text/plain".into(),
            levels: vec![Level::L1, Level::L2],
            algorithms: Some(vec!["append/1".into()]),
            editor_services: None,
        }
    }

    fn file(s: &str) -> NewFile {
        NewFile { mime: "text/plain".into(), bytes: s.as_bytes().to_vec() }
    }

    struct Rig {
        clock: Arc<ManualClock>,
        backend: MemoryBackend,
        owner: MemoryStore,
        editor: MemoryStore,
        viewer: MemoryStore,
        doc: DocId,
    }

    async fn rig(config: MemoryConfig) -> Rig {
        let clock = Arc::new(ManualClock::new(10_000));
        let backend = MemoryBackend::with_config(clock.clone(), config);
        let owner = backend.store("@o:mem", "OD");
        let editor = backend.store("@e:mem", "ED");
        let viewer = backend.store("@v:mem", "VD");
        let doc = owner.create_document(&meta(), Some(file("one"))).await.unwrap();
        owner.grant(&doc, "@e:mem", Role::Editor).await.unwrap();
        owner.grant(&doc, "@v:mem", Role::Viewer).await.unwrap();
        Rig { clock, backend, owner, editor, viewer, doc }
    }

    #[tokio::test]
    async fn create_publishes_the_first_version_with_no_parents() {
        let r = rig(MemoryConfig::default()).await;
        let v = r.owner.versions(&r.doc).await.unwrap();
        assert_eq!(v.len(), 1);
        assert!(v[0].body.parents.is_empty());
        assert_eq!(v[0].id, "$ev000001:mem", "deterministic ids");
        assert_eq!(v[0].sender, "@o:mem");
        assert_eq!(v[0].ts_ms, 10_000);
        assert_eq!(r.owner.fetch_file(&r.doc, &v[0]).await.unwrap(), b"one");
    }

    #[tokio::test]
    async fn roles_are_enforced_in_the_store() {
        let r = rig(MemoryConfig::default()).await;
        let head = r.owner.versions(&r.doc).await.unwrap()[0].id.clone();
        assert_eq!(r.viewer.role(&r.doc).await.unwrap(), Role::Viewer);
        let denied = r.viewer.publish_version(&r.doc, vec![head.clone()], file("x"), None).await;
        assert!(matches!(denied, Err(StoreError::Forbidden(_))), "{denied:?}");
        assert!(matches!(r.viewer.grant(&r.doc, "@v:mem", Role::Owner).await, Err(StoreError::Forbidden(_))));
        assert!(matches!(r.editor.grant(&r.doc, "@e:mem", Role::Owner).await, Err(StoreError::Forbidden(_))));
        assert!(matches!(r.editor.revoke(&r.doc, "@v:mem").await, Err(StoreError::Forbidden(_))));
        r.editor.publish_version(&r.doc, vec![head], file("two"), None).await.unwrap();
        assert_eq!(r.owner.versions(&r.doc).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_rejected_publish_leaves_no_version_behind() {
        let r = rig(MemoryConfig::default()).await;
        let _ = r.viewer.publish_version(&r.doc, vec!["$ev000001:mem".into()], file("x"), None).await;
        assert_eq!(r.owner.versions(&r.doc).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn bad_parents_are_refused() {
        let r = rig(MemoryConfig::default()).await;
        assert!(matches!(
            r.editor.publish_version(&r.doc, vec![], file("x"), None).await,
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            r.editor.publish_version(&r.doc, vec!["$nope".into()], file("x"), None).await,
            Err(StoreError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn revoked_member_cannot_read_what_is_published_afterwards() {
        let r = rig(MemoryConfig::default()).await;
        let old = r.viewer.versions(&r.doc).await.unwrap();
        assert_eq!(old.len(), 1);
        r.owner.revoke(&r.doc, "@v:mem").await.unwrap();
        let new = r.editor.publish_version(&r.doc, vec![old[0].id.clone()], file("secret"), None).await.unwrap();

        // what they already had stays readable (spec 11) ...
        assert_eq!(r.viewer.fetch_file(&r.doc, &old[0]).await.unwrap(), b"one");
        // ... what came later is not.
        let seen = r.viewer.versions(&r.doc).await.unwrap();
        assert!(seen.iter().all(|v| v.id != new.id));
        assert!(matches!(r.viewer.fetch_file(&r.doc, &new).await, Err(StoreError::NotFound(_))));
        assert!(matches!(r.viewer.role(&r.doc).await, Err(StoreError::Forbidden(_))));
        assert!(matches!(r.viewer.lease(&r.doc).await, Err(StoreError::Forbidden(_))));
    }

    #[tokio::test]
    async fn regranting_does_not_reveal_what_was_published_while_removed() {
        let r = rig(MemoryConfig::default()).await;
        let head = r.owner.versions(&r.doc).await.unwrap()[0].id.clone();
        r.owner.revoke(&r.doc, "@v:mem").await.unwrap();
        let hidden = r.editor.publish_version(&r.doc, vec![head], file("gap"), None).await.unwrap();
        r.owner.grant(&r.doc, "@v:mem", Role::Viewer).await.unwrap();
        assert!(r.viewer.versions(&r.doc).await.unwrap().iter().all(|v| v.id != hidden.id));
    }

    #[tokio::test]
    async fn a_stranger_reads_nothing() {
        let r = rig(MemoryConfig::default()).await;
        let stranger = r.backend.store("@x:mem", "XD");
        assert!(matches!(stranger.versions(&r.doc).await, Err(StoreError::Forbidden(_))));
        assert!(matches!(stranger.updates(&r.doc).await, Err(StoreError::Forbidden(_))));
        assert!(matches!(stranger.meta(&r.doc).await, Err(StoreError::Forbidden(_))));
    }

    #[tokio::test]
    async fn truncated_upload_is_refused_and_never_published() {
        let r = rig(MemoryConfig::default()).await;
        let head = r.owner.versions(&r.doc).await.unwrap()[0].id.clone();
        r.backend.truncate_next_upload();
        let res = r.editor.publish_version(&r.doc, vec![head.clone()], file("a long enough file"), None).await;
        assert!(matches!(res, Err(StoreError::Transport(_))), "{res:?}");
        assert_eq!(r.owner.versions(&r.doc).await.unwrap().len(), 1);
        // the fault was one-shot
        r.editor.publish_version(&r.doc, vec![head], file("fine"), None).await.unwrap();
    }

    #[tokio::test]
    async fn skipping_upload_verification_lets_a_truncated_file_through() {
        // The deliberate defect the conformance mutation tests rely on.
        let r = rig(MemoryConfig { verify_uploads: false, ..MemoryConfig::default() }).await;
        let head = r.owner.versions(&r.doc).await.unwrap()[0].id.clone();
        r.backend.truncate_next_upload();
        let v = r.editor.publish_version(&r.doc, vec![head], file("a long enough file"), None).await.unwrap();
        let bytes = r.editor.fetch_file(&r.doc, &v).await.unwrap();
        assert!(integrity::verify(&v.body.file, &bytes).is_err());
    }

    #[tokio::test]
    async fn lease_cas_refuses_a_second_session_but_lets_an_owner_break() {
        let r = rig(MemoryConfig::default()).await;
        let claim = |p: &str, s: &str| lease::Claim::new(p, s, format!("t-{s}"), "$ev000001:mem", 60_000);
        r.editor.write_lease(&r.doc, claim("@e:mem", "s1").lease()).await.unwrap();
        // the same session may rewrite (renew); another session is refused
        r.editor.write_lease(&r.doc, claim("@e:mem", "s1").lease()).await.unwrap();
        let other = r.backend.store("@e2:mem", "E2");
        r.owner.grant(&r.doc, "@e2:mem", Role::Editor).await.unwrap();
        assert!(matches!(other.write_lease(&r.doc, claim("@e2:mem", "s2").lease()).await, Err(StoreError::Conflict(_))));
        assert!(matches!(other.write_lease(&r.doc, lease::released()).await, Err(StoreError::Conflict(_))));
        r.owner.write_lease(&r.doc, claim("@o:mem", "so").lease()).await.unwrap();
        // after expiry anyone may write
        r.clock.advance(60_000);
        other.write_lease(&r.doc, claim("@e2:mem", "s2").lease()).await.unwrap();
    }

    #[tokio::test]
    async fn lease_holder_must_be_the_authenticated_principal() {
        let r = rig(MemoryConfig::default()).await;
        let spoof = lease::Claim::new("@o:mem", "s", "t", "$v", 5_000).lease();
        assert!(matches!(r.editor.write_lease(&r.doc, spoof).await, Err(StoreError::Forbidden(_))));
    }

    #[tokio::test]
    async fn racy_lease_accepts_both_writers_last_wins() {
        let r = rig(MemoryConfig { cas_leases: false, ..MemoryConfig::default() }).await;
        r.owner.grant(&r.doc, "@e2:mem", Role::Editor).await.unwrap();
        let other = r.backend.store("@e2:mem", "E2");
        let c1 = lease::Claim::new("@e:mem", "s1", "t1", "$v", 60_000);
        let c2 = lease::Claim::new("@e2:mem", "s2", "t2", "$v", 60_000);
        r.editor.write_lease(&r.doc, c1.lease()).await.unwrap();
        other.write_lease(&r.doc, c2.lease()).await.unwrap();
        let rec = r.editor.lease(&r.doc).await.unwrap().unwrap();
        assert_eq!(rec.body.holder, Some(c2.holder));
    }

    #[tokio::test]
    async fn invalid_lease_and_update_are_refused_by_the_schema() {
        let r = rig(MemoryConfig::default()).await;
        let mut bad = lease::Claim::new("@e:mem", "s", "t", "$v", 5_000).lease();
        bad.duration_ms = Some(7_200_000);
        assert!(matches!(r.editor.write_lease(&r.doc, bad).await, Err(StoreError::Invalid(_))));
        let bad_update = Update {
            dcp: 0,
            algorithm: "Loro".into(),
            doc_model: "text/1".into(),
            base_version: "$v".into(),
            seq: 0,
            payload: Payload::Base64 { data: String::new(), size: 0 },
        };
        assert!(matches!(r.editor.publish_update(&r.doc, bad_update).await, Err(StoreError::Invalid(_))));
    }

    #[tokio::test]
    async fn updates_carry_sender_and_device_and_respect_roles_and_revocation() {
        let r = rig(MemoryConfig::default()).await;
        let mut w = crate::updates::StreamWriter::new("append/1", "text/1");
        let u = w.next("$ev000001:mem", b"hi").unwrap();
        assert!(matches!(r.viewer.publish_update(&r.doc, u.clone()).await, Err(StoreError::Forbidden(_))));
        let s = r.editor.publish_update(&r.doc, u).await.unwrap();
        assert_eq!((s.sender.as_str(), s.device.as_deref()), ("@e:mem", Some("ED")));
        assert_eq!(r.viewer.updates(&r.doc).await.unwrap().len(), 1);
        r.owner.revoke(&r.doc, "@v:mem").await.unwrap();
        let u2 = w.next("$ev000001:mem", b"later").unwrap();
        r.editor.publish_update(&r.doc, u2).await.unwrap();
        assert_eq!(r.viewer.updates(&r.doc).await.unwrap().len(), 1, "the later update is unreadable");
        assert_eq!(r.owner.updates(&r.doc).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn clock_is_injectable_and_now_ms_follows_it() {
        let r = rig(MemoryConfig::default()).await;
        assert_eq!(r.owner.now_ms(&r.doc).await.unwrap(), 10_000);
        r.clock.advance(5);
        assert_eq!(r.viewer.now_ms(&r.doc).await.unwrap(), 10_005);
    }
}
