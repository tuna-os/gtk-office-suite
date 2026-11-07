//! Who may fetch what from this node. Shared between the replica (which writes it) and the
//! iroh-blobs event handler (which reads it on its own task).
//!
//! The access rule is the p2panda-auth membership of the document: only a current member of the
//! document whose file it is may get the blob. This is a second line of defence. The first is
//! that the file is encrypted with a key that only the (space-encrypted) version entry holds.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use iroh::EndpointId;

#[derive(Default)]
struct Inner {
    /// document -> endpoints that are members (any access level) at the moment.
    members: HashMap<String, HashSet<EndpointId>>,
    /// blake3 hash of a sealed file -> document it belongs to.
    blob_doc: HashMap<iroh_blobs::Hash, String>,
}

pub struct Acl {
    inner: RwLock<Inner>,
    online: AtomicBool,
    enforce: AtomicBool,
}

impl Acl {
    pub fn new(enforce: bool) -> Self {
        Acl {
            inner: RwLock::new(Inner::default()),
            online: AtomicBool::new(true),
            enforce: AtomicBool::new(enforce),
        }
    }

    pub fn set_online(&self, online: bool) {
        self.online.store(online, Ordering::SeqCst);
    }

    pub fn online(&self) -> bool {
        self.online.load(Ordering::SeqCst)
    }

    pub fn set_members(&self, doc: &str, members: HashSet<EndpointId>) {
        self.inner
            .write()
            .expect("acl lock")
            .members
            .insert(doc.to_owned(), members);
    }

    pub fn register_blob(&self, hash: iroh_blobs::Hash, doc: &str) {
        self.inner
            .write()
            .expect("acl lock")
            .blob_doc
            .insert(hash, doc.to_owned());
    }

    pub fn is_member(&self, doc: &str, id: &EndpointId) -> bool {
        if !self.enforce.load(Ordering::SeqCst) {
            return true;
        }
        self.inner
            .read()
            .expect("acl lock")
            .members
            .get(doc)
            .is_some_and(|m| m.contains(id))
    }

    /// A connection to the blob protocol is worth accepting at all: the node is a member of some
    /// document, and this node is online.
    pub fn may_connect(&self, id: &EndpointId) -> bool {
        if !self.online() {
            return false;
        }
        if !self.enforce.load(Ordering::SeqCst) {
            return true;
        }
        self.inner
            .read()
            .expect("acl lock")
            .members
            .values()
            .any(|m| m.contains(id))
    }

    /// May `id` get the blob `hash`: it must belong to a document `id` is a member of.
    pub fn may_get(&self, id: &EndpointId, hash: &iroh_blobs::Hash) -> bool {
        if !self.online() {
            return false;
        }
        if !self.enforce.load(Ordering::SeqCst) {
            return true;
        }
        let g = self.inner.read().expect("acl lock");
        match g.blob_doc.get(hash) {
            Some(doc) => g.members.get(doc).is_some_and(|m| m.contains(id)),
            None => false,
        }
    }
}
