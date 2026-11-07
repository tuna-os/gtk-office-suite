//! Receiving: validate, store, order causally, hand to p2panda-spaces, collect entries.

use std::collections::BTreeSet;
use std::rc::Rc;

use dcp_core::StoreError;
use p2panda_auth::group::GroupAction;
use p2panda_core::{validate_operation, Hash, Operation};
use p2panda_spaces::{Event, SpacesArgs};
use p2panda_store::operations::OperationStore;
use p2panda_store::Transaction;

use crate::envelope::Envelope;
use crate::ext::DcpExt;
use crate::msg::DcpMsg;
use crate::replica::{tr, Doc, Entry, Inner};
use crate::signed;

impl Inner {
    /// Store a batch of operations in ONE transaction, so that a peer asking this node for
    /// operations sees all of the batch or none of it. (No lock against sessions is taken here: a
    /// receiver that waited for the sessions' read lock deadlocked with its peer, which waited the
    /// same way, in a CI run of the journeys.)
    async fn store_ops(&self, doc: &Doc, ops: &[Operation<DcpExt>]) -> Result<(), StoreError> {
        if ops.is_empty() {
            return Ok(());
        }
        let permit = doc.store.begin().await.map_err(tr)?;
        let mut result = Ok(());
        for op in ops {
            if let Err(e) = doc.store.insert_operation(&op.hash, op, &doc.id).await {
                result = Err(tr(e));
                break;
            }
        }
        match result {
            Ok(()) => doc.store.commit(permit).await.map_err(tr),
            Err(e) => {
                let _ = doc.store.rollback(permit).await;
                Err(e)
            }
        }
    }

    /// Operations that arrived from a peer (or from `import`): check, store, apply what is ready.
    pub async fn ingest(
        self: &Rc<Self>,
        doc: &Rc<Doc>,
        ops: Vec<Operation<DcpExt>>,
    ) -> Result<(), StoreError> {
        let _guard = doc.lock.lock().await;
        let mut fresh: Vec<Operation<DcpExt>> = Vec::new();
        for op in ops {
            doc.st.borrow_mut().counters.ops_received += 1;
            if validate_operation(&op).is_err() {
                doc.st.borrow_mut().counters.ops_invalid += 1;
                continue;
            }
            let h = op.hash;
            {
                let st = doc.st.borrow();
                if st.applied.contains(&h)
                    || st.pending.contains_key(&h)
                    || st.rejected.contains_key(&h)
                    || fresh.iter().any(|o| o.hash == h)
                {
                    continue;
                }
            }
            fresh.push(op);
        }
        self.store_ops(doc, &fresh).await?;
        {
            let mut st = doc.st.borrow_mut();
            for op in fresh {
                st.authors.insert(op.header.verifying_key);
                st.pending.insert(op.hash, op);
            }
        }
        self.drain_locked(doc).await
    }

    /// Apply, in causal order, every pending operation whose dependencies are applied.
    pub async fn drain_locked(self: &Rc<Self>, doc: &Rc<Doc>) -> Result<(), StoreError> {
        let mut membership_changed = false;
        loop {
            let ready: Vec<Operation<DcpExt>> = {
                let st = doc.st.borrow();
                st.pending
                    .values()
                    .filter(|op| {
                        op.header
                            .extensions
                            .args
                            .dependencies()
                            .iter()
                            .all(|d| st.applied.contains(d))
                    })
                    .cloned()
                    .collect()
            };
            if ready.is_empty() {
                break;
            }
            for op in ready {
                doc.st.borrow_mut().pending.remove(&op.hash);
                if matches!(
                    op.header.extensions.args,
                    SpacesArgs::Auth { .. } | SpacesArgs::SpaceMembership { .. }
                ) {
                    membership_changed = true;
                }
                self.apply(doc, op).await;
            }
        }
        if membership_changed {
            doc.refresh_members(&self.acl).await?;
            self.changed.notify_one();
        }
        Ok(())
    }

    async fn apply(self: &Rc<Self>, doc: &Rc<Doc>, op: Operation<DcpExt>) {
        let h = op.hash;
        let author = op.header.verifying_key;
        let args = op.header.extensions.args.clone();
        let msg = DcpMsg { op };
        let verdict: Result<(), String> = match &args {
            SpacesArgs::KeyBundle { .. } => Ok(()),
            SpacesArgs::SpaceUpdate { .. } => {
                Err("SpaceUpdate is unimplemented in p2panda-spaces 0.7.1 and is refused".into())
            }
            SpacesArgs::Auth { group_action, .. } => match group_action {
                GroupAction::Promote { .. } | GroupAction::Demote { .. } => Err(
                    "role changes are refused: p2panda-spaces 0.7.1 panics (unimplemented!) on a promote or demote"
                        .into(),
                ),
                GroupAction::Create { .. } if author != doc.creator => {
                    Err("a group was created by someone other than the document's creator".into())
                }
                GroupAction::Add { member, .. } if member.is_group() => {
                    Err("nested groups are not used by this binding".into())
                }
                _ => self.process_msg(doc, &msg).await,
            },
            SpacesArgs::SpaceMembership { space_id, .. } | SpacesArgs::Application { space_id, .. }
                if *space_id != doc.space_id =>
            {
                Err("the message belongs to another space".into())
            }
            SpacesArgs::SpaceMembership { .. } | SpacesArgs::Application { .. } => {
                self.process_msg(doc, &msg).await
            }
        };
        let mut st = doc.st.borrow_mut();
        match verdict {
            Ok(()) => {
                st.applied.insert(h);
                st.counters.ops_applied += 1;
            }
            Err(e) => {
                st.counters.ops_rejected += 1;
                tracing::debug!(op = %h.to_hex(), "operation refused: {e}");
                st.rejected.insert(h, e);
            }
        }
    }

    async fn process_msg(self: &Rc<Self>, doc: &Rc<Doc>, msg: &DcpMsg) -> Result<(), String> {
        let (groups_y, space_y, events) = doc.manager.process(msg).await.map_err(|e| e.to_string())?;
        doc.persist(groups_y.as_ref(), space_y)
            .await
            .map_err(|e| e.to_string())?;
        for ev in events {
            if let Event::Application { data, .. } = ev {
                self.accept_plaintext(doc, &data);
            }
        }
        Ok(())
    }

    /// A decrypted application payload: verify its signature and keep it as an entry.
    pub fn accept_plaintext(self: &Rc<Self>, doc: &Rc<Doc>, data: &[u8]) {
        match signed::verify(data, &doc.id) {
            Ok(v) => {
                let heads: BTreeSet<Hash> = v
                    .body
                    .heads
                    .iter()
                    .filter_map(|h| h.parse::<Hash>().ok())
                    .collect();
                let entry = Entry {
                    id: v.id,
                    author: v.author,
                    ts_us: v.body.ts_us,
                    seen_ms: self.now_ms(),
                    heads,
                    env: v.body.env,
                };
                self.add_entry(doc, entry);
            }
            Err(e) => {
                doc.st.borrow_mut().counters.entries_invalid += 1;
                tracing::debug!("entry refused: {e}");
            }
        }
    }

    pub fn add_entry(self: &Rc<Self>, doc: &Rc<Doc>, entry: Entry) {
        let mut st = doc.st.borrow_mut();
        if !st.entry_ids.insert(entry.id.clone()) {
            return;
        }
        if let Envelope::Lease { lamport, .. } = &entry.env {
            st.lease_lamport = st.lease_lamport.max(*lamport);
        }
        let blob = match &entry.env {
            Envelope::Version { blob, .. } => Some(blob.clone()),
            _ => None,
        };
        let author = entry.author;
        st.entries.push(entry);
        drop(st);
        if let Some(blob) = blob {
            self.schedule_blob(doc, blob, author);
        }
    }
}
