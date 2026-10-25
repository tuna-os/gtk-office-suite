//! Connections, sync sessions, the anti-entropy loop and the barrier.

use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::Duration;

use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::EndpointId;
use p2panda_core::{Hash, VerifyingKey};
use tokio::sync::mpsc;

use crate::envelope::principal_of;
use crate::net::{
    key_of, read_hello, read_status, run_session, write_hello, write_status, Hello, SessionStats,
    CONNECT_TIMEOUT, STATUS_DENIED, STATUS_OK, SYNC_ALPN,
};
use crate::replica::{Doc, Inner};

const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
const BARRIER_WAIT: Duration = Duration::from_secs(8);
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);

impl Inner {
    pub async fn connect(&self, peer: VerifyingKey, alpn: &[u8]) -> Result<Connection, String> {
        if !self.online.get() {
            return Err("this node is offline".into());
        }
        let cacheable = alpn == SYNC_ALPN;
        if cacheable {
            if let Some(c) = self.conns.borrow().get(&peer) {
                if c.close_reason().is_none() {
                    return Ok(c.clone());
                }
            }
        }
        let addr = self
            .peers
            .borrow()
            .get(&peer)
            .cloned()
            .ok_or_else(|| format!("no address known for {}", principal_of(&peer)))?;
        let started = crate::pathlog::now_us();
        let conn = tokio::time::timeout(CONNECT_TIMEOUT, self.net.endpoint.connect(addr, alpn))
            .await
            .map_err(|_| "connecting timed out".to_string())?
            .map_err(|e| e.to_string())?;
        crate::pathlog::track(&self.pathlog, &conn, "dial", started);
        if cacheable {
            self.conns.borrow_mut().insert(peer, conn.clone());
        }
        Ok(conn)
    }

    pub fn set_online(&self, online: bool) {
        self.online.set(online);
        self.acl.set_online(online);
        if !online {
            for (_, c) in self.conns.borrow_mut().drain() {
                c.close(0u32.into(), b"offline");
            }
        } else {
            self.changed.notify_one();
        }
    }

    // ------------------------------------------------------------------ acceptor

    pub async fn accept_loop(self: Rc<Self>, mut rx: mpsc::UnboundedReceiver<Connection>) {
        while let Some(conn) = rx.recv().await {
            let this = self.clone();
            tokio::task::spawn_local(async move { this.serve_connection(conn).await });
        }
    }

    async fn serve_connection(self: Rc<Self>, conn: Connection) {
        let remote = conn.remote_id();
        crate::pathlog::track(&self.pathlog, &conn, "accept", crate::pathlog::now_us());
        loop {
            let (send, recv) = match conn.accept_bi().await {
                Ok(p) => p,
                Err(_) => return,
            };
            let this = self.clone();
            tokio::task::spawn_local(async move {
                if let Err(e) = this.serve_stream(remote, send, recv).await {
                    tracing::debug!("sync stream from {} ended: {e}", remote.fmt_short());
                }
            });
        }
    }

    fn sync_allowed(&self, doc: &Doc, remote: &VerifyingKey) -> bool {
        if !self.cfg.enforce_acl {
            return true;
        }
        let st = doc.st.borrow();
        if st.members.iter().any(|(k, _)| k == remote) {
            return true;
        }
        // A device that has just joined knows no members yet: only the creator may reach it.
        st.members.is_empty() && *remote == doc.creator
    }

    async fn serve_stream(
        self: Rc<Self>,
        remote: EndpointId,
        mut send: SendStream,
        mut recv: RecvStream,
    ) -> Result<(), String> {
        let hello = tokio::time::timeout(HELLO_TIMEOUT, read_hello(&mut recv))
            .await
            .map_err(|_| "no hello".to_string())??;
        let remote_key = key_of(&remote)?;
        let doc = match self.doc(&hello.doc) {
            Ok(d) if self.online.get() => d,
            _ => {
                write_status(&mut send, STATUS_DENIED).await?;
                let _ = send.finish();
                return Err(format!("unknown document {}", hello.doc));
            }
        };
        // Take the document's read lock BEFORE the membership check: a writer that changes
        // membership holds it exclusively until the new member list has reached the gate, so a
        // session either runs entirely before the change or sees it entirely, never the new
        // operations with the old gate.
        let guard = doc.serving.read().await;
        if !self.sync_allowed(&doc, &remote_key) {
            drop(guard);
            doc.st.borrow_mut().counters.sync_denied += 1;
            write_status(&mut send, STATUS_DENIED).await?;
            let _ = send.finish();
            return Err(format!("{} is not a member of {}", principal_of(&remote_key), doc.id));
        }
        if let Some(op) = hello.barrier {
            drop(guard);
            let ok = match op.parse::<Hash>() {
                Ok(h) => self.wait_applied(&doc, h, BARRIER_WAIT).await,
                Err(_) => false,
            };
            write_status(&mut send, if ok { STATUS_OK } else { STATUS_DENIED }).await?;
            let _ = send.finish();
            return Ok(());
        }
        write_status(&mut send, STATUS_OK).await?;
        let logs = doc.logs(&self.me);
        let out = run_session(&doc.store, logs, send, recv).await;
        drop(guard);
        let got = out.ops.len();
        self.ingest(&doc, out.ops).await.map_err(|e| e.to_string())?;
        if got > 0 {
            self.changed.notify_one();
        }
        match out.error {
            None => {
                doc.st.borrow_mut().counters.sync_sessions += 1;
                Ok(())
            }
            Some(e) => {
                doc.st.borrow_mut().counters.sync_failures += 1;
                Err(e)
            }
        }
    }

    async fn wait_applied(&self, doc: &Rc<Doc>, op: Hash, max: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + max;
        loop {
            {
                let st = doc.st.borrow();
                if st.applied.contains(&op) {
                    return true;
                }
                if st.rejected.contains_key(&op) {
                    return false;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    // ------------------------------------------------------------------ initiator

    /// The peers worth talking to about `doc`: its current members, and the creator it joined
    /// through, that this node has an address for. A removed member is no longer one of them.
    pub fn sync_targets(&self, doc: &Doc) -> Vec<VerifyingKey> {
        let st = doc.st.borrow();
        let mut set: BTreeSet<VerifyingKey> = st.members.iter().map(|(k, _)| *k).collect();
        set.extend(st.sync_peers.iter().copied());
        set.remove(&self.me);
        let peers = self.peers.borrow();
        set.into_iter().filter(|k| peers.contains_key(k)).collect()
    }

    pub async fn sync_once(self: &Rc<Self>, doc: &Rc<Doc>, peer: VerifyingKey) -> Result<SessionStats, String> {
        let result = self.sync_inner(doc, peer).await;
        match &result {
            Ok(_) => {}
            Err(e) => {
                doc.st.borrow_mut().counters.sync_failures += 1;
                self.conns.borrow_mut().remove(&peer);
                tracing::debug!("sync with {} failed: {e}", principal_of(&peer));
            }
        }
        result
    }

    /// Is `peer` someone this node may talk to about `doc` right now: a current member, or the
    /// creator it joined through?
    fn may_sync_with(&self, doc: &Doc, peer: &VerifyingKey) -> bool {
        if !self.cfg.enforce_acl {
            return true;
        }
        let st = doc.st.borrow();
        st.members.iter().any(|(k, _)| k == peer) || st.sync_peers.contains(peer)
    }

    async fn sync_inner(self: &Rc<Self>, doc: &Rc<Doc>, peer: VerifyingKey) -> Result<SessionStats, String> {
        let conn = self.connect(peer, SYNC_ALPN).await?;
        // The decision to talk to `peer` may be older than this moment (the session was queued or
        // was waiting for the connection): take the read lock, which a membership change holds
        // exclusively until the new list has reached the gate, and decide again under it.
        let guard = doc.serving.read().await;
        if !self.may_sync_with(doc, &peer) {
            return Err(format!("{} is no longer a member of {}", principal_of(&peer), doc.id));
        }
        let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
        write_hello(
            &mut send,
            &Hello {
                doc: doc.id.clone(),
                barrier: None,
            },
        )
        .await?;
        if read_status(&mut recv).await? != STATUS_OK {
            doc.st.borrow_mut().counters.sync_denied += 1;
            return Err(format!("{} refused to sync {}", principal_of(&peer), doc.id));
        }
        let logs = doc.logs(&self.me);
        let out = run_session(&doc.store, logs, send, recv).await;
        drop(guard);
        self.ingest(doc, out.ops).await.map_err(|e| e.to_string())?;
        match (out.stats, out.error) {
            (Some(stats), None) => {
                doc.st.borrow_mut().counters.sync_sessions += 1;
                if stats.received_operations > 0 || stats.sent_operations > 0 {
                    self.changed.notify_one();
                }
                Ok(stats)
            }
            (_, Some(e)) => Err(e),
            (None, None) => Err("the session produced no result".into()),
        }
    }

    /// Ask for a sync with `peer` now; at most one session per (document, peer) runs at a time,
    /// and a request that arrives meanwhile makes it run once more.
    pub fn kick(self: &Rc<Self>, doc: &Rc<Doc>, peer: VerifyingKey) {
        let key = (doc.id.clone(), peer);
        {
            let mut slots = self.slots.borrow_mut();
            let slot = slots.entry(key.clone()).or_default();
            if slot.running {
                slot.again = true;
                return;
            }
            slot.running = true;
        }
        let this = self.clone();
        let doc = doc.clone();
        tokio::task::spawn_local(async move {
            loop {
                let r = this.sync_once(&doc, peer).await;
                let rerun = {
                    let mut slots = this.slots.borrow_mut();
                    let slot = slots.get_mut(&key).expect("slot exists while running");
                    let again = std::mem::take(&mut slot.again);
                    if r.is_err() || !again {
                        slot.running = false;
                        false
                    } else {
                        true
                    }
                };
                if !rerun {
                    break;
                }
            }
        });
    }

    pub async fn anti_entropy(self: Rc<Self>) {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(self.cfg.sync_interval) => {}
                _ = self.changed.notified() => {}
            }
            if !self.online.get() {
                continue;
            }
            let docs: Vec<Rc<Doc>> = self.docs.borrow().values().cloned().collect();
            for doc in docs {
                for peer in self.sync_targets(&doc) {
                    self.kick(&doc, peer);
                }
            }
        }
    }

    // ------------------------------------------------------------------ barrier

    /// Push the operations to every reachable member and wait until each says it has applied
    /// `op`. Best effort: a member that cannot be reached within the timeout is skipped.
    pub async fn confirm_applied(self: &Rc<Self>, doc: &Rc<Doc>, op: Hash) {
        let mut tasks = Vec::new();
        for peer in self.sync_targets(doc) {
            let this = self.clone();
            let doc = doc.clone();
            tasks.push(tokio::task::spawn_local(async move {
                let _ = tokio::time::timeout(CONFIRM_TIMEOUT, this.push_and_barrier(&doc, peer, op)).await;
            }));
        }
        for t in tasks {
            let _ = t.await;
        }
    }

    async fn push_and_barrier(self: &Rc<Self>, doc: &Rc<Doc>, peer: VerifyingKey, op: Hash) -> Result<(), String> {
        self.sync_once(doc, peer).await?;
        let conn = self.connect(peer, SYNC_ALPN).await?;
        let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
        write_hello(
            &mut send,
            &Hello {
                doc: doc.id.clone(),
                barrier: Some(op.to_hex()),
            },
        )
        .await?;
        let status = read_status(&mut recv).await?;
        let _ = send.finish();
        if status == STATUS_OK {
            Ok(())
        } else {
            Err("the member did not confirm".into())
        }
    }
}
