//! Crash / reconnect recovery of the update path, end to end over the
//! in-memory store: [`Outbox`] (writer) -> `DocumentStore` -> [`Receiver`].
//!
//! Scenarios taken from a comparison with Frostnotes' sync lifecycle (#1785).
//! Each one pins what a user can see: after a restart or reconnect, edits are
//! neither lost nor applied twice, and a removed writer stays out.

use std::sync::Arc;

use dcp_core::memory::{ManualClock, MemoryBackend, MemoryStore};
use dcp_core::outbox::{Outbox, Step};
use dcp_core::updates::{Receiver, StreamWriter, Target};
use dcp_core::{DocId, DocumentStore, Level, Meta, NewFile, Role, StoreError, Update};
use parking_lot::Mutex;

struct Append(Vec<u8>);

impl Target for Append {
    fn supports(&self, algorithm: &str, doc_model: &str) -> bool {
        algorithm == "append/1" && doc_model == "text/1"
    }
    fn dry_run(&self, _: &str, _: &str, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn apply(&mut self, _: &str, _: &str, payload: &[u8]) {
        self.0.extend_from_slice(payload);
    }
}

struct Rig {
    owner: MemoryStore,
    editor: MemoryStore,
    reader: MemoryStore,
    doc: DocId,
    base: String,
}

async fn rig() -> Rig {
    let backend = MemoryBackend::new(Arc::new(ManualClock::new(10_000)));
    let owner = backend.store("@o:mem", "OD");
    let editor = backend.store("@e:mem", "ED");
    let reader = backend.store("@r:mem", "RD");
    let meta = Meta {
        dcp: 1,
        title: "Doc".into(),
        mime: "text/plain".into(),
        levels: vec![Level::L1, Level::L2],
        algorithms: Some(vec!["append/1".into()]),
        editor_services: None,
    };
    let doc = owner
        .create_document(
            &meta,
            Some(NewFile {
                mime: "text/plain".into(),
                bytes: b"one".to_vec(),
            }),
        )
        .await
        .unwrap();
    owner.grant(&doc, "@e:mem", Role::Editor).await.unwrap();
    owner.grant(&doc, "@r:mem", Role::Viewer).await.unwrap();
    let base = owner.versions(&doc).await.unwrap()[0].id.clone();
    Rig {
        owner,
        editor,
        reader,
        doc,
        base,
    }
}

fn queue(base: &str, parts: &[&str]) -> Outbox<Update> {
    let mut w = StreamWriter::new("append/1", "text/1");
    let mut o = Outbox::new(3);
    for p in parts {
        o.push(w.next(base, p.as_bytes()).unwrap());
    }
    o
}

/// Drive the outbox until it is empty or refuses; `fail` decides, per send
/// attempt, whether the response is lost *after* the store accepted the update.
async fn drain(
    o: &mut Outbox<Update>,
    store: &MemoryStore,
    doc: &DocId,
    lose_response: &Mutex<Vec<bool>>,
) -> Step {
    let mut now = 0;
    loop {
        let step = o
            .step(now, |u| async move {
                store.publish_update(doc, u).await?;
                if lose_response.lock().pop().unwrap_or(false) {
                    return Err(StoreError::Transport("response lost".into()));
                }
                Ok(())
            })
            .await;
        match step {
            Step::Idle => return step,
            Step::Sent => {}
            Step::Wait { until_ms } | Step::BackedOff { until_ms } => now = until_ms,
            Step::Refused(_) => return step,
        }
    }
}

async fn accept(store: &MemoryStore, doc: &DocId, u: Update) -> Result<(), StoreError> {
    store.publish_update(doc, u).await.map(|_| ())
}

async fn read(r: &Rig, who: &MemoryStore) -> (String, usize) {
    let log = who.updates(&r.doc).await.unwrap();
    let mut rx = Receiver::new(Some(r.base.clone()));
    let mut t = Append(b"one:".to_vec());
    rx.ingest(&log, &mut t);
    (String::from_utf8(t.0).unwrap(), log.len())
}

#[tokio::test]
async fn a_crash_after_the_store_accepted_replays_the_update_and_it_applies_once() {
    let r = rig().await;
    let mut o = queue(&r.base, &["a", "b", "c"]);
    // The first send is accepted but its reply is lost (crash/timeout): the
    // outbox cannot know, keeps the head and sends it again.
    let lose = Mutex::new(vec![false, false, true]);
    assert_eq!(drain(&mut o, &r.editor, &r.doc, &lose).await, Step::Idle);
    assert!(o.is_empty(), "nothing left queued");

    let (text, stored) = read(&r, &r.reader).await;
    assert_eq!(stored, 4, "the replay is stored twice by the binding");
    assert_eq!(text, "one:abc", "not lost, not duplicated, in creation order");
}

#[tokio::test]
async fn edits_made_while_offline_arrive_in_order_after_reconnect() {
    let r = rig().await;
    let mut o = queue(&r.base, &["a", "b"]);
    // Offline: the transport fails; nothing is dropped and nothing is stored.
    let down = |_u: Update| async { Err::<(), _>(StoreError::Transport("offline".into())) };
    assert!(matches!(o.step(0, down).await, Step::BackedOff { .. }));
    assert_eq!(o.len(), 2);
    assert_eq!(read(&r, &r.owner).await, ("one:".to_string(), 0));

    // Back online.
    let lose = Mutex::new(vec![]);
    assert_eq!(drain(&mut o, &r.editor, &r.doc, &lose).await, Step::Idle);
    assert_eq!(read(&r, &r.owner).await, ("one:ab".to_string(), 2));
}

#[tokio::test]
async fn a_removed_editor_is_refused_keeps_its_queue_and_writes_nothing() {
    let r = rig().await;
    let mut o = queue(&r.base, &["a", "b"]);
    let lose = Mutex::new(vec![]);
    // One edit lands while the editor is still a member ...
    assert_eq!(o.step(0, |u| accept(&r.editor, &r.doc, u)).await, Step::Sent);
    // ... then the owner removes them (revoked/expired credentials).
    r.owner.revoke(&r.doc, "@e:mem").await.unwrap();

    let step = drain(&mut o, &r.editor, &r.doc, &lose).await;
    assert!(matches!(step, Step::Refused(StoreError::Forbidden(_))), "{step:?}");
    assert_eq!(o.len(), 1, "the refused edit is kept, not silently dropped");

    // Retrying never gets through, and the log only has the pre-removal edit.
    let again = o.step(1_000_000, |u| accept(&r.editor, &r.doc, u)).await;
    assert!(matches!(again, Step::Refused(StoreError::Forbidden(_))));
    assert_eq!(read(&r, &r.owner).await, ("one:a".to_string(), 1));

    // A removed member cannot publish a version either.
    let head = r.owner.versions(&r.doc).await.unwrap()[0].id.clone();
    let v = r
        .editor
        .publish_version(
            &r.doc,
            vec![head],
            NewFile {
                mime: "text/plain".into(),
                bytes: b"x".to_vec(),
            },
            None,
        )
        .await;
    assert!(matches!(v, Err(StoreError::Forbidden(_))), "{v:?}");
    assert_eq!(r.owner.versions(&r.doc).await.unwrap().len(), 1);
}
