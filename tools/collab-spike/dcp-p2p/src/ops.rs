//! The operations behind the `DocumentStore` trait, on the store's own thread.

use std::collections::{BTreeSet, HashSet};
use std::rc::Rc;
use std::str::FromStr;

use dcp_core::graph;
use dcp_core::integrity;
use dcp_core::schema::{self, Kind};
use dcp_core::{
    Author, DocId, Lease, Meta, NewFile, Role, Stamped, StoreError, Update, Version,
};
use p2panda_auth::{Access, AccessLevel};
use p2panda_core::{Hash, Header, Operation, VerifyingKey};
use p2panda_spaces::space::SpaceError;
use p2panda_spaces::SpacesArgs;

use crate::auth_check::authorized as authorized_raw;
use crate::blobcrypt;
use crate::envelope::{blob_locator, blob_of_locator, parse_principal, principal_of, Envelope};
use crate::ext::DcpExt;
use crate::msg::{DcpMsg, KEYS_LOG};
use crate::replica::{role_of_level, tr, Doc, DocState, Entry, Inner};
use crate::signed::{self, Body};

fn authorized(
    authorize: bool,
    state: &p2panda_spaces::SpacesStoreState<()>,
    heads: &BTreeSet<Hash>,
    who: &VerifyingKey,
    need: AccessLevel,
) -> bool {
    !authorize || authorized_raw(state, heads, who, need)
}

fn need_for(env: &Envelope) -> AccessLevel {
    match env {
        Envelope::Meta { .. } => AccessLevel::Manage,
        _ => AccessLevel::Write,
    }
}

fn map_space_err(e: SpaceError<crate::msg::DcpForge, (), p2panda_spaces::StrongRemoveResolver<()>>) -> StoreError {
    match e {
        SpaceError::NotWelcomed(_) => {
            StoreError::Forbidden("this device is not (yet) a member of the document's group".into())
        }
        other => StoreError::Invalid(other.to_string()),
    }
}

impl Inner {
    // ----------------------------------------------------------------- identity and contact cards

    /// This principal's key bundle for `doc`, as a signed operation (header bytes). Another node
    /// registers it before it can add this principal to the document.
    pub async fn card(&self, doc: &Rc<Doc>) -> Result<Vec<u8>, StoreError> {
        let _g = doc.lock.lock().await;
        doc.forge.set_context(KEYS_LOG);
        let msg = doc.manager.key_bundle_message().await.map_err(tr)?;
        Ok(msg.op.header.encode())
    }

    pub async fn register_card(&self, doc: &Rc<Doc>, card: &[u8]) -> Result<VerifyingKey, StoreError> {
        let header = Header::<DcpExt>::decode(card).map_err(|e| StoreError::Invalid(format!("contact card: {e}")))?;
        if !matches!(header.extensions.args, SpacesArgs::KeyBundle { .. }) {
            return Err(StoreError::Invalid("contact card: not a key bundle".into()));
        }
        let owner = header.verifying_key;
        let msg = DcpMsg {
            op: Operation::from_parts(header, None),
        };
        let _g = doc.lock.lock().await;
        doc.manager.process(&msg).await.map_err(tr)?;
        Ok(owner)
    }

    // ----------------------------------------------------------------- documents

    pub async fn create_document(
        self: &Rc<Self>,
        meta: Meta,
        first: Option<NewFile>,
    ) -> Result<DocId, StoreError> {
        schema::validate_typed(Kind::Meta, &meta).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let topic: [u8; 32] = self.rng.random_array().map_err(tr)?;
        let id = hex::encode(topic);
        let doc = self.new_doc(id.clone(), Hash::from(topic), self.me).await?;
        let _g = doc.lock.lock().await;
        doc.forge.set_context(&doc.id);
        let none: &[(VerifyingKey, Access<()>)] = &[];
        let (groups_y, space_y, messages) = doc
            .manager
            .create_space(doc.space_id, none)
            .await
            .map_err(tr)?;
        doc.persist(Some(&groups_y), Some(space_y)).await?;
        {
            let mut st = doc.st.borrow_mut();
            for m in &messages {
                st.applied.insert(m.op.hash);
            }
        }
        doc.refresh_members(&self.acl).await?;
        self.publish_entry(&doc, Envelope::Meta { meta }, true).await?;
        if let Some(file) = first {
            self.publish_version_locked(&doc, Vec::new(), file, None, true).await?;
        }
        Ok(id)
    }

    /// Prepare to join `doc_id`, created by `creator`: makes the local database and returns the
    /// contact card the creator must register before it can grant this principal a role. The
    /// creator is the first peer to sync with.
    pub async fn join_document(
        self: &Rc<Self>,
        doc_id: &str,
        creator: &str,
    ) -> Result<Vec<u8>, StoreError> {
        let creator = parse_principal(creator)
            .ok_or_else(|| StoreError::Invalid("the creator is not a public key".into()))?;
        let topic = hex::decode(doc_id).map_err(|_| StoreError::Invalid("document id".into()))?;
        let topic: [u8; 32] = topic
            .try_into()
            .map_err(|_| StoreError::Invalid("document id must be 32 bytes".into()))?;
        let doc = match self.doc(doc_id) {
            Ok(d) => d,
            Err(_) => self.new_doc(doc_id.to_owned(), Hash::from(topic), creator).await?,
        };
        doc.st.borrow_mut().sync_peers.insert(creator);
        self.changed.notify_one();
        self.card(&doc).await
    }

    // ----------------------------------------------------------------- roles

    pub fn role(&self, doc: &Rc<Doc>) -> Result<Role, StoreError> {
        doc.level_of(&self.me)
            .and_then(|l| role_of_level(&l))
            .ok_or_else(|| StoreError::Forbidden(format!("{} is not a member of {}", self.principal, doc.id)))
    }

    fn access_for(role: Role) -> Result<Access<()>, StoreError> {
        match role {
            Role::Viewer => Ok(Access::read()),
            Role::Editor => Ok(Access::write()),
            Role::Owner => Ok(Access::manage()),
            Role::Commenter => Err(StoreError::Invalid(
                "the commenter role cannot be expressed: p2panda-auth has Pull, Read, Write and Manage"
                    .into(),
            )),
        }
    }

    pub async fn grant(
        self: &Rc<Self>,
        doc: &Rc<Doc>,
        principal: &str,
        role: Role,
    ) -> Result<(), StoreError> {
        let _g = doc.lock.lock().await;
        self.require_manager(doc, "granting a role")?;
        let target = parse_principal(principal)
            .ok_or_else(|| StoreError::Invalid("the principal is not a public key".into()))?;
        let access = Self::access_for(role)?;
        match doc.level_of(&target) {
            Some(l) if l == access.level => Ok(()),
            Some(_) => {
                // Both operations become visible to sessions together.
                let _both = self.hold_sessions(doc).await;
                // p2panda-auth has promote and demote, but p2panda-spaces 0.7.1 panics when a
                // space processes either (`unimplemented!()` in `encryption/message.rs`), so a
                // role change is a removal followed by an addition: the group key rotates twice.
                self.remove_member(doc, target).await?;
                self.add_member(doc, target, access).await
            }
            None => self.add_member(doc, target, access).await,
        }
    }

    pub async fn revoke(self: &Rc<Self>, doc: &Rc<Doc>, principal: &str) -> Result<(), StoreError> {
        let _g = doc.lock.lock().await;
        self.require_manager(doc, "removing a member")?;
        let target = parse_principal(principal)
            .ok_or_else(|| StoreError::Invalid("the principal is not a public key".into()))?;
        if doc.level_of(&target).is_none() {
            return Err(StoreError::NotFound(format!("{principal} is not a member")));
        }
        // No session may read the removal before the gate knows the member is gone.
        let space_msg = {
            let _w = self.hold_sessions(doc).await;
            self.remove_member(doc, target).await?
        };
        drop(_g);
        // Wait until every reachable remaining member has applied the removal, so what they
        // publish next is encrypted to the rotated key. An unreachable member cannot be waited for:
        // see the results document.
        self.confirm_applied(doc, space_msg).await;
        Ok(())
    }

    /// Keep the document's sessions out while a membership change is being made. A session holds
    /// the lock across network waits, so the wait is bounded: two nodes that both change membership
    /// while sessions run between them would otherwise wait for each other. On a timeout the change
    /// goes ahead without the exclusion (the removal may then be served before the gate knows it).
    async fn hold_sessions<'a>(&self, doc: &'a Doc) -> Option<tokio::sync::RwLockWriteGuard<'a, ()>> {
        match tokio::time::timeout(std::time::Duration::from_secs(5), doc.serving.write()).await {
            Ok(g) => Some(g),
            Err(_) => {
                tracing::warn!("sessions did not finish in 5 s; changing membership without excluding them");
                None
            }
        }
    }

    fn require_manager(&self, doc: &Doc, what: &str) -> Result<(), StoreError> {
        match doc.level_of(&self.me) {
            Some(AccessLevel::Manage) => Ok(()),
            Some(l) => Err(StoreError::Forbidden(format!(
                "{what} needs the owner role; this principal is {:?}",
                role_of_level(&l)
            ))),
            None => Err(StoreError::Forbidden(format!("{} is not a member", self.principal))),
        }
    }

    async fn add_member(
        self: &Rc<Self>,
        doc: &Rc<Doc>,
        target: VerifyingKey,
        access: Access<()>,
    ) -> Result<(), StoreError> {
        doc.forge.set_context(&doc.id);
        let space = doc
            .manager
            .space(doc.space_id)
            .await
            .map_err(tr)?
            .ok_or_else(|| StoreError::NotFound("space".into()))?;
        let (groups_y, space_y, auth_msg, space_msg) =
            space.add(target, access).await.map_err(map_space_err)?;
        doc.persist(Some(&groups_y), Some(space_y)).await?;
        {
            let mut st = doc.st.borrow_mut();
            st.applied.insert(auth_msg.op.hash);
            st.applied.insert(space_msg.op.hash);
        }
        doc.refresh_members(&self.acl).await?;
        self.changed.notify_one();
        Ok(())
    }

    /// Returns the hash of the space-membership message that carries the key rotation.
    async fn remove_member(self: &Rc<Self>, doc: &Rc<Doc>, target: VerifyingKey) -> Result<Hash, StoreError> {
        doc.forge.set_context(&doc.id);
        let space = doc
            .manager
            .space(doc.space_id)
            .await
            .map_err(tr)?
            .ok_or_else(|| StoreError::NotFound("space".into()))?;
        let (groups_y, space_y, auth_msg, space_msg) =
            space.remove(target).await.map_err(map_space_err)?;
        doc.persist(Some(&groups_y), Some(space_y)).await?;
        let rotation = space_msg.op.hash;
        {
            let mut st = doc.st.borrow_mut();
            st.applied.insert(auth_msg.op.hash);
            st.applied.insert(rotation);
        }
        doc.refresh_members(&self.acl).await?;
        self.changed.notify_one();
        Ok(rotation)
    }

    // ----------------------------------------------------------------- entries

    /// Sign, encrypt to the group, append to the log. The caller holds the document lock.
    pub async fn publish_entry(
        self: &Rc<Self>,
        doc: &Rc<Doc>,
        env: Envelope,
        check_role: bool,
    ) -> Result<(String, u64), StoreError> {
        if check_role {
            let need = need_for(&env);
            match doc.level_of(&self.me) {
                Some(l) if l >= need => {}
                Some(l) => {
                    return Err(StoreError::Forbidden(format!(
                        "{:?} may not publish this; it needs {:?}",
                        role_of_level(&l),
                        role_of_level(&need)
                    )))
                }
                None => {
                    return Err(StoreError::Forbidden(format!(
                        "{} is not a member of {}",
                        self.principal, doc.id
                    )))
                }
            }
        }
        doc.forge.set_context(&doc.id);
        let space = doc
            .manager
            .space(doc.space_id)
            .await
            .map_err(tr)?
            .ok_or_else(|| StoreError::Forbidden("nothing of this document has been synced yet".into()))?;
        let state = doc.space_state().await?;
        let mut heads: Vec<String> = state
            .groups_y
            .inner
            .heads()
            .into_iter()
            .map(|h| h.to_hex())
            .collect();
        heads.sort();
        let nonce: [u8; 16] = self.rng.random_array().map_err(tr)?;
        let ts_us = self.cfg.clock.now_us();
        let body = Body {
            author: self.principal.clone(),
            doc: doc.id.clone(),
            heads: heads.clone(),
            ts_us,
            nonce: hex::encode(nonce),
            env: env.clone(),
        };
        let (plaintext, id) = signed::sign(&self.key, &body)?;
        let (space_y, msg) = space.publish(&plaintext).await.map_err(map_space_err)?;
        doc.persist(None, Some(space_y)).await?;
        {
            let mut st = doc.st.borrow_mut();
            st.applied.insert(msg.op.hash);
            st.authors.insert(self.me);
        }
        let heads: BTreeSet<Hash> = heads.iter().filter_map(|h| h.parse::<Hash>().ok()).collect();
        self.add_entry(
            doc,
            Entry {
                id: id.clone(),
                author: self.me,
                ts_us,
                seen_ms: self.now_ms(),
                heads,
                env,
            },
        );
        self.changed.notify_one();
        Ok((id, ts_us))
    }

    pub async fn publish_version_locked(
        self: &Rc<Self>,
        doc: &Rc<Doc>,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
        check_role: bool,
    ) -> Result<Stamped<Version>, StoreError> {
        if check_role {
            match doc.level_of(&self.me) {
                Some(l) if l >= AccessLevel::Write => {}
                _ => {
                    return Err(StoreError::Forbidden(
                        "publishing a version needs the editor role".into(),
                    ))
                }
            }
        }
        let existing = self.versions_locked(doc).await?;
        graph::check_new_parents(&existing, &parents)
            .map_err(|e| StoreError::Invalid(format!("parents: {e}")))?;

        // Upload: seal the file, put it in the blob store, read it back and check it (version rule 2).
        let (sealed, key) = blobcrypt::seal(&self.rng, &file.bytes).map_err(tr)?;
        let intended = iroh_blobs::Hash::new(&sealed);
        let blob_hex = intended.to_hex();
        let locator = blob_locator(&blob_hex);
        let file_ref = integrity::file_ref(&file.mime, &file.bytes, locator);
        let stored: Vec<u8> = if self.truncate_next.swap(false, std::sync::atomic::Ordering::SeqCst) {
            sealed[..sealed.len() / 2].to_vec()
        } else {
            sealed
        };
        let tag = self
            .net
            .blobs
            .add_bytes(bytes::Bytes::from(stored))
            .await
            .map_err(tr)?;
        if tag.hash != intended {
            return Err(StoreError::Transport(
                "upload failed verification, not published: the stored blob is not the one meant".into(),
            ));
        }
        let back = self.net.blobs.get_bytes(intended).await.map_err(tr)?;
        let plain = blobcrypt::open(&key, &back).map_err(tr)?;
        integrity::verify(&file_ref, &plain)
            .map_err(|e| StoreError::Transport(format!("upload failed verification, not published: {e}")))?;
        drop(back);

        let version = Version {
            dcp: 1,
            parents,
            file: file_ref,
            created_by: Author {
                principal: self.principal.clone(),
                device: self.device.clone(),
            },
            message,
            covers: None,
        };
        schema::validate_typed(Kind::Version, &version).map_err(|e| StoreError::Invalid(e.to_string()))?;
        doc.st.borrow_mut().blob_ready.insert(blob_hex.clone());
        self.acl.register_blob(intended, &doc.id);
        let env = Envelope::Version {
            version: version.clone(),
            blob: blob_hex,
            key: hex::encode(key),
        };
        let (id, ts_us) = self.publish_entry(doc, env, check_role).await?;
        Ok(Stamped::new(id, ts_us / 1000, self.principal.clone(), version).with_device(self.device.clone()))
    }

    pub async fn publish_version(
        self: &Rc<Self>,
        doc: &Rc<Doc>,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
    ) -> Result<Stamped<Version>, StoreError> {
        let _g = doc.lock.lock().await;
        self.publish_version_locked(doc, parents, file, message, true).await
    }

    /// What a hostile editor-less member does: sign and encrypt a version entry (it holds the
    /// group key) without the role check. The readers' authorisation rule is what refuses it.
    pub async fn forge_version(
        self: &Rc<Self>,
        doc: &Rc<Doc>,
        version: Version,
    ) -> Result<String, StoreError> {
        let _g = doc.lock.lock().await;
        let blob = blob_of_locator(&version.file.locator).unwrap_or_default();
        // The hostile member holds the group key, so it can read the file key of the version it
        // copies; with the right key the only thing standing between the forgery and a reader is
        // the authorisation rule.
        let key = doc
            .st
            .borrow()
            .entries
            .iter()
            .find_map(|e| match &e.env {
                Envelope::Version { blob: b, key, .. } if *b == blob => Some(key.clone()),
                _ => None,
            })
            .unwrap_or_else(|| hex::encode([0u8; 32]));
        let env = Envelope::Version { version, blob, key };
        let (id, _) = self.publish_entry(doc, env, false).await?;
        Ok(id)
    }

    // ----------------------------------------------------------------- reads

    fn collect_versions(
        st: &DocState,
        state: &p2panda_spaces::SpacesStoreState<()>,
        authorize: bool,
    ) -> Vec<Stamped<Version>> {
        st.entries
            .iter()
            .filter_map(|e| match &e.env {
                Envelope::Version { version, blob, .. }
                    if version.created_by.principal == principal_of(&e.author)
                        && blob_of_locator(&version.file.locator).as_deref() == Some(blob.as_str())
                        && st.blob_ready.contains(blob)
                        && authorized(authorize, state, &e.heads, &e.author, AccessLevel::Write) =>
                {
                    Some(
                        Stamped::new(
                            e.id.clone(),
                            e.ts_us / 1000,
                            principal_of(&e.author),
                            version.clone(),
                        )
                        .with_device(version.created_by.device.clone()),
                    )
                }
                _ => None,
            })
            .collect()
    }

    pub async fn versions_locked(&self, doc: &Rc<Doc>) -> Result<Vec<Stamped<Version>>, StoreError> {
        let Ok(state) = doc.space_state().await else {
            return Ok(Vec::new());
        };
        let st = doc.st.borrow();
        Ok(Self::collect_versions(&st, &state, self.cfg.authorize))
    }

    pub async fn versions(&self, doc: &Rc<Doc>) -> Result<Vec<Stamped<Version>>, StoreError> {
        self.require_member(doc)?;
        self.versions_locked(doc).await
    }

    fn require_member(&self, doc: &Doc) -> Result<(), StoreError> {
        if doc.level_of(&self.me).is_some() {
            Ok(())
        } else {
            Err(StoreError::Forbidden(format!(
                "{} is not a member of {}",
                self.principal, doc.id
            )))
        }
    }

    pub async fn meta(&self, doc: &Rc<Doc>) -> Result<Meta, StoreError> {
        self.require_member(doc)?;
        let state = doc.space_state().await?;
        let st = doc.st.borrow();
        st.entries
            .iter()
            .find_map(|e| match &e.env {
                Envelope::Meta { meta } if authorized(self.cfg.authorize, &state, &e.heads, &e.author, AccessLevel::Manage) => {
                    Some(meta.clone())
                }
                _ => None,
            })
            .ok_or_else(|| StoreError::NotFound("the document's metadata has not arrived".into()))
    }

    pub async fn fetch_file(&self, doc: &Rc<Doc>, version: &Stamped<Version>) -> Result<Vec<u8>, StoreError> {
        self.require_member(doc)?;
        let (blob, key) = {
            let st = doc.st.borrow();
            st.entries
                .iter()
                .find_map(|e| match &e.env {
                    Envelope::Version { blob, key, .. } if e.id == version.id => {
                        Some((blob.clone(), key.clone()))
                    }
                    _ => None,
                })
                .ok_or_else(|| StoreError::NotFound(format!("version {}", version.id)))?
        };
        let hash = iroh_blobs::Hash::from_str(&blob).map_err(tr)?;
        if !self.net.blobs.has(hash).await.map_err(tr)? {
            return Err(StoreError::NotFound(format!("the file of version {} is not here yet", version.id)));
        }
        let sealed = self.net.blobs.get_bytes(hash).await.map_err(tr)?;
        let key: [u8; 32] = hex::decode(&key)
            .map_err(tr)?
            .try_into()
            .map_err(|_| StoreError::Invalid("file key".into()))?;
        blobcrypt::open(&key, &sealed).map_err(tr)
    }

    pub async fn lease(&self, doc: &Rc<Doc>) -> Result<Option<Stamped<Lease>>, StoreError> {
        self.require_member(doc)?;
        let state = doc.space_state().await?;
        let st = doc.st.borrow();
        let best = st
            .entries
            .iter()
            .filter_map(|e| match &e.env {
                Envelope::Lease { lease, lamport }
                    if authorized(self.cfg.authorize, &state, &e.heads, &e.author, AccessLevel::Write) =>
                {
                    Some((*lamport, e, lease))
                }
                _ => None,
            })
            // The deterministic winner: highest Lamport value, the entry id breaking a tie.
            .max_by(|a, b| (a.0, &a.1.id).cmp(&(b.0, &b.1.id)));
        Ok(best.map(|(_, e, lease)| {
            // The stamp is this replica's own first-seen time (lease rule 1, see the results
            // document); the negative control uses the author's clock instead.
            let ts = if self.cfg.author_stamped_leases { e.ts_us / 1000 } else { e.seen_ms };
            Stamped::new(e.id.clone(), ts, principal_of(&e.author), lease.clone())
        }))
    }

    pub async fn write_lease(self: &Rc<Self>, doc: &Rc<Doc>, lease: Lease) -> Result<Stamped<Lease>, StoreError> {
        schema::validate_typed(Kind::Lease, &lease).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let _g = doc.lock.lock().await;
        let lamport = doc.st.borrow().lease_lamport + 1;
        let (id, _) = self
            .publish_entry(doc, Envelope::Lease { lease: lease.clone(), lamport }, true)
            .await?;
        Ok(Stamped::new(id, self.now_ms(), self.principal.clone(), lease))
    }

    pub async fn publish_update(self: &Rc<Self>, doc: &Rc<Doc>, update: Update) -> Result<Stamped<Update>, StoreError> {
        schema::validate_typed(Kind::Update, &update).map_err(|e| StoreError::Invalid(e.to_string()))?;
        if matches!(update.payload, dcp_core::Payload::File { .. }) {
            return Err(StoreError::Invalid(
                "file-encoded updates are not supported by this binding".into(),
            ));
        }
        let _g = doc.lock.lock().await;
        let env = Envelope::Update {
            update: update.clone(),
            device: self.device.clone(),
        };
        let (id, ts_us) = self.publish_entry(doc, env, true).await?;
        Ok(Stamped::new(id, ts_us / 1000, self.principal.clone(), update).with_device(self.device.clone()))
    }

    pub async fn updates(&self, doc: &Rc<Doc>) -> Result<Vec<Stamped<Update>>, StoreError> {
        self.require_member(doc)?;
        let state = doc.space_state().await?;
        let st = doc.st.borrow();
        Ok(st
            .entries
            .iter()
            .filter_map(|e| match &e.env {
                Envelope::Update { update, device }
                    if authorized(self.cfg.authorize, &state, &e.heads, &e.author, AccessLevel::Write) =>
                {
                    Some(
                        Stamped::new(e.id.clone(), e.ts_us / 1000, principal_of(&e.author), update.clone())
                            .with_device(device.clone()),
                    )
                }
                _ => None,
            })
            .collect())
    }

    // ----------------------------------------------------------------- blobs

    /// A version entry arrived: make sure its file gets fetched (and registered with the gate).
    pub fn schedule_blob(self: &Rc<Self>, doc: &Rc<Doc>, blob: String, hint: VerifyingKey) {
        {
            let mut st = doc.st.borrow_mut();
            if st.blob_ready.contains(&blob) || !st.blob_fetching.insert(blob.clone()) {
                return;
            }
        }
        let this = self.clone();
        let doc = doc.clone();
        tokio::task::spawn_local(async move {
            this.fetch_blob_task(doc, blob, hint).await;
        });
    }

    async fn fetch_blob_task(self: Rc<Self>, doc: Rc<Doc>, blob: String, hint: VerifyingKey) {
        let Ok(hash) = iroh_blobs::Hash::from_str(&blob) else {
            doc.st.borrow_mut().blob_fetching.remove(&blob);
            return;
        };
        let deadline = tokio::time::Instant::now() + self.cfg.fetch_timeout;
        loop {
            // Only entries a reader accepts are worth fetching. The entry may need auth operations
            // that arrive later, so a refusal is retried until the deadline.
            let wanted = {
                let entry = doc
                    .st
                    .borrow()
                    .entries
                    .iter()
                    .find(|e| matches!(&e.env, Envelope::Version { blob: b, .. } if *b == blob))
                    .cloned();
                match (entry, doc.space_state().await) {
                    (Some(e), Ok(state)) => {
                        authorized(self.cfg.authorize, &state, &e.heads, &e.author, AccessLevel::Write)
                    }
                    _ => false,
                }
            };
            if wanted {
                self.acl.register_blob(hash, &doc.id);
                if self.net.blobs.has(hash).await.unwrap_or(false) {
                    break;
                }
                let mut candidates: Vec<VerifyingKey> = vec![hint];
                {
                    let st = doc.st.borrow();
                    candidates.extend(st.members.iter().map(|(k, _)| *k));
                    candidates.extend(st.sync_peers.iter().copied());
                }
                let mut seen = HashSet::new();
                let mut done = false;
                for peer in candidates {
                    if peer == self.me || !seen.insert(peer) || !self.online.get() {
                        continue;
                    }
                    let started = std::time::Instant::now();
                    let Ok(conn) = self.connect(peer, iroh_blobs::ALPN).await else {
                        continue;
                    };
                    if self.net.blobs.remote().fetch(conn, hash).await.is_ok() {
                        let micros = started.elapsed().as_micros() as u64;
                        if self.net.blobs.has(hash).await.unwrap_or(false) {
                            let bytes = self
                                .net
                                .blobs
                                .get_bytes(hash)
                                .await
                                .map(|b| b.len() as u64)
                                .unwrap_or(0);
                            doc.st.borrow_mut().blob_fetches.push(crate::replica::BlobFetch {
                                blob: blob.clone(),
                                bytes,
                                micros,
                            });
                            done = true;
                            break;
                        }
                    }
                }
                if done {
                    break;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                doc.st.borrow_mut().blob_fetching.remove(&blob);
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        {
            let mut st = doc.st.borrow_mut();
            st.blob_ready.insert(blob.clone());
            st.blob_fetching.remove(&blob);
        }
        self.changed.notify_one();
    }

    // ----------------------------------------------------------------- test hooks

    /// The encoded headers of every operation stored for the document (what goes on the wire).
    pub async fn raw_op_headers(&self, doc: &Rc<Doc>) -> Result<Vec<Vec<u8>>, StoreError> {
        let authors: Vec<VerifyingKey> = doc.st.borrow().authors.iter().copied().collect();
        let mut out = Vec::new();
        for a in authors {
            let entries = <p2panda_store::SqliteStore as p2panda_store::logs::LogStore<
                Operation<DcpExt>,
                VerifyingKey,
                String,
                p2panda_core::SeqNum,
                Hash,
            >>::get_log_entries(&doc.store, &a, &doc.id, None, None)
            .await
            .map_err(tr)?;
            if let Some(v) = entries {
                out.extend(v.into_iter().map(|(_, header)| header));
            }
        }
        Ok(out)
    }

    /// Fetch a blob from `peer` by its BLAKE3 hash, without any version entry: what a node that
    /// learned a hash out of band (a removed member, say) can do. Returns the sealed bytes.
    pub async fn fetch_raw_blob(&self, peer: VerifyingKey, blake3_hex: &str) -> Result<Vec<u8>, StoreError> {
        let hash = iroh_blobs::Hash::from_str(blake3_hex).map_err(tr)?;
        let conn = self
            .connect(peer, iroh_blobs::ALPN)
            .await
            .map_err(StoreError::Transport)?;
        self.net.blobs.remote().fetch(conn, hash).await.map_err(tr)?;
        let b = self.net.blobs.get_bytes(hash).await.map_err(tr)?;
        Ok(b.to_vec())
    }

    /// Size in bytes of the persisted p2panda-spaces state of the document (CBOR), which
    /// p2panda-spaces 0.7.1 reads and writes in full for every message.
    pub async fn space_state_bytes(&self, doc: &Rc<Doc>) -> Result<usize, StoreError> {
        let st = doc.space_state().await?;
        let bytes = p2panda_core::cbor::encode_cbor(&*st).map_err(tr)?;
        Ok(bytes.len())
    }
}
