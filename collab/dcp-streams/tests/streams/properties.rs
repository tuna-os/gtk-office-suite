//! Properties the conformance journeys cannot see: what a hostile member or a
//! hostile server can do with the bytes, and what the client does about it.
//! Each test says which claim of the results document it backs.

use std::sync::Arc;
use std::time::Duration;

use dcp_conformance::{new_file, standard_meta, ALGORITHM, BASE_FILE, DOC_MODEL};
use dcp_core::updates::{Receiver, StreamWriter, Target};
use dcp_core::{Covers, DocId, DocumentStore, Lease, LeaseState, Role, StoreError, Update};
use dcp_streams::crypto::{self, b64, unb64, Identity};
use dcp_streams::doc::DropReason;
use dcp_streams::http::{HttpError, JSON, START};
use dcp_streams::wire::{Kind, Record};
use dcp_streams::{StreamsConfig, StreamsStore};
use serde_json::Value;

use crate::common::*;
use crate::require_streams;

/// An owner, an editor and a viewer on one document (epoch 2).
pub struct Rig {
    pub owner: Arc<StreamsStore>,
    pub editor: Arc<StreamsStore>,
    pub viewer: Arc<StreamsStore>,
    pub doc: DocId,
    pub first: String,
}

impl Rig {
    pub async fn new(base: &str) -> Rig {
        Self::build(base, StreamsConfig::new("OWNER-DEV")).await
    }

    pub async fn build(base: &str, owner_cfg: StreamsConfig) -> Rig {
        let owner = Arc::new(StreamsStore::new(
            crate::common::http(base),
            Identity::generate(),
            owner_cfg,
        ));
        let editor = store_at(base, "EDITOR-DEV");
        let viewer = store_at(base, "VIEWER-DEV");
        let (a, b, c) = tokio::join!(owner.calibrate(), editor.calibrate(), viewer.calibrate());
        a.unwrap();
        b.unwrap();
        c.unwrap();
        let doc = owner
            .create_document(&standard_meta(), Some(new_file(BASE_FILE)))
            .await
            .unwrap();
        owner
            .grant(&doc, &editor.principal(), Role::Editor)
            .await
            .unwrap();
        owner
            .grant(&doc, &viewer.principal(), Role::Viewer)
            .await
            .unwrap();
        let first = owner.versions(&doc).await.unwrap()[0].id.clone();
        Rig {
            owner,
            editor,
            viewer,
            doc,
            first,
        }
    }

    pub async fn epoch(&self) -> u64 {
        self.owner.sync(&self.doc).await.unwrap();
        self.owner.epochs(&self.doc).await.last().unwrap().0
    }

    pub async fn path(&self, n: u64, which: &str) -> String {
        self.owner.stream_path(&self.doc, n, which).await.unwrap()
    }
}

pub async fn raw(store: &StreamsStore, path: &str) -> Vec<Value> {
    store.http().read_all(path, START).await.unwrap().0
}

pub async fn post(
    store: &StreamsStore,
    path: &str,
    rec: &impl serde::Serialize,
) -> Result<(), HttpError> {
    store
        .http()
        .append(path, JSON, serde_json::to_vec(rec).unwrap(), None, false)
        .await
        .map(|_| ())
}

fn inner_version(rig_first: &str, author: &str, st: u64) -> Value {
    serde_json::json!({
        "st": st,
        "body": {
            "dcp": 1,
            "parents": [rig_first],
            "file": {"mime": "application/x", "size": 3, "sha256": "0".repeat(64),
                     "locator": {"type": "durable-stream", "href": "https://x.invalid/dcp/d/f/zzz"}},
            "created_by": {"principal": author, "device": "EVIL"}
        }
    })
}

fn reasons(d: &[dcp_streams::doc::Dropped]) -> Vec<DropReason> {
    d.iter().map(|x| x.reason.clone()).collect()
}

// ---------------------------------------------------------------- integrity

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tampered_record_is_dropped_and_never_shown() {
    let base = require_streams!();
    let rig = Rig::new(&base).await;
    let v2 = rig
        .editor
        .publish_version(&rig.doc, vec![rig.first.clone()], new_file(b"v2"), None)
        .await
        .unwrap();
    let n = rig.epoch().await;
    let log = rig.path(n, "log").await;
    let good: Record =
        serde_json::from_value(raw(&rig.owner, &log).await.last().unwrap().clone()).unwrap();
    assert_eq!(good.id(), v2.id);

    let attacker = Identity::generate();
    let keys = rig.editor.held_keys(&rig.doc).await;
    let key_n = keys.iter().find(|(e, _)| *e == n).unwrap().1;

    // 1. one bit of the ciphertext flipped: the signature covers it
    let mut flipped = good.clone();
    let mut c = unb64(&flipped.c).unwrap();
    c[0] ^= 1;
    flipped.c = b64(&c);
    // 2. the author field changed: the signature covers it
    let mut renamed = good.clone();
    renamed.a = attacker.principal();
    // 3. re-signed by a stranger who claims to be the author: not in the roster
    let mut resigned = renamed.clone();
    resigned.s = b64(&attacker.sign(&resigned.signed_bytes(&rig.doc)));
    // 4. a member below editor signs a copy of the real ciphertext
    let mut by_viewer = good.clone();
    by_viewer.a = rig.viewer.principal();
    by_viewer.s = b64(&rig
        .viewer
        .identity()
        .sign(&by_viewer.signed_bytes(&rig.doc)));
    // 5. a malicious editor (holds the key, may sign) modifies the ciphertext and re-signs
    let mut by_editor = good.clone();
    by_editor.c = flipped.c.clone();
    by_editor.s = b64(&rig
        .editor
        .identity()
        .sign(&by_editor.signed_bytes(&rig.doc)));
    // 6. the right bytes in the wrong place: an update record replayed into the log
    let mut upd = flipped.clone();
    upd.k = Kind::Upd;

    for r in [&flipped, &renamed, &resigned, &by_viewer, &by_editor, &upd] {
        post(&rig.owner, &log, r).await.unwrap(); // the server stores anything
    }
    // 7. not even a record
    rig.owner
        .http()
        .append(&log, JSON, br#"{"hello":"world"}"#.to_vec(), None, false)
        .await
        .unwrap();
    rig.owner
        .http()
        .append(&log, JSON, br#""just a string""#.to_vec(), None, false)
        .await
        .unwrap();
    // 8. the same good record, twice more (a replay)
    post(&rig.owner, &log, &good).await.unwrap();
    post(&rig.owner, &log, &good).await.unwrap();

    for who in [&rig.owner, &rig.editor, &rig.viewer] {
        let mut ids: Vec<String> = who
            .versions(&rig.doc)
            .await
            .unwrap()
            .into_iter()
            .map(|v| v.id)
            .collect();
        ids.sort();
        let mut want = vec![rig.first.clone(), v2.id.clone()];
        want.sort();
        assert_eq!(
            ids,
            want,
            "{} sees something that was not published",
            who.principal()
        );
    }
    let why = reasons(&rig.viewer.dropped(&rig.doc).await);
    assert!(why.contains(&DropReason::Signature), "{why:?}");
    assert!(why.contains(&DropReason::NotAMember), "{why:?}");
    assert!(why.contains(&DropReason::Role(Role::Viewer)), "{why:?}");
    assert!(why.contains(&DropReason::Decrypt), "{why:?}");
    assert!(why.contains(&DropReason::WrongStream), "{why:?}");
    assert!(why.contains(&DropReason::Malformed), "{why:?}");
    // The malicious editor's variant failed on the AEAD tag, not on trust:
    // prove the tag is what stopped it by decrypting the original.
    let plain = crypto::open(
        &key_n,
        &unb64(&good.n).unwrap(),
        &Record::aad(&rig.doc, good.k, good.e, &good.a),
        &unb64(&good.c).unwrap(),
    )
    .unwrap();
    assert!(!plain.is_empty());
    assert!(crypto::open(
        &key_n,
        &unb64(&by_editor.n).unwrap(),
        &Record::aad(&rig.doc, good.k, good.e, &good.a),
        &unb64(&by_editor.c).unwrap()
    )
    .is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_wrong_key_fails_closed() {
    let base = require_streams!();
    let rig = Rig::new(&base).await;
    let other = Rig::new(&base).await;

    // A stranger with a valid identity and no wrap.
    let stranger = store_at(&base, "STRANGER");
    for r in [
        stranger.versions(&rig.doc).await.map(|_| ()),
        stranger.meta(&rig.doc).await.map(|_| ()),
        stranger.updates(&rig.doc).await.map(|_| ()),
        stranger.role(&rig.doc).await.map(|_| ()),
    ] {
        assert!(matches!(r, Err(StoreError::Forbidden(_))), "{r:?}");
    }
    let listed = rig.owner.versions(&rig.doc).await.unwrap();
    let r = stranger.fetch_file(&rig.doc, &listed[0]).await;
    assert!(matches!(r, Err(StoreError::Forbidden(_))), "{r:?}");

    // A member of another document holds a key that is not this one's.
    let foreign = other.owner.held_keys(&other.doc).await;
    assert!(!foreign.is_empty());
    let log = rig.path(rig.epoch().await, "log").await;
    for v in raw(&rig.owner, &log).await {
        let rec: Record = serde_json::from_value(v).unwrap();
        for (_, k) in &foreign {
            assert!(
                crypto::open(
                    k,
                    &unb64(&rec.n).unwrap(),
                    &Record::aad(&rig.doc, rec.k, rec.e, &rec.a),
                    &unb64(&rec.c).unwrap()
                )
                .is_err(),
                "a key of another document opened a record"
            );
        }
    }
    // Its own store, asked for this document, learns nothing: no valid chain
    // of epochs that names it.
    let r = other.owner.versions(&rig.doc).await;
    assert!(matches!(r, Err(StoreError::Forbidden(_))), "{r:?}");

    // The same file blob under a wrong epoch's key or a wrong file id does not open.
    let v = &listed[0];
    let id = v
        .body
        .file
        .locator_href()
        .rsplit('/')
        .next()
        .unwrap()
        .to_owned();
    let blob = rig
        .owner
        .http()
        .read_bytes(&format!("dcp/{}/f/{id}", rig.doc))
        .await
        .unwrap();
    let key0 = rig.owner.held_keys(&rig.doc).await[0].1;
    let aad = dcp_streams::wire::blob_aad(&rig.doc, &id, 0);
    let nonce = &blob[13..13 + 24];
    let ct = &blob[13 + 24..];
    assert!(
        crypto::open(&key0, nonce, &aad, ct).is_ok(),
        "the right key and context open it"
    );
    assert!(crypto::open(
        &key0,
        nonce,
        &dcp_streams::wire::blob_aad(&rig.doc, "other-id", 0),
        ct
    )
    .is_err());
    assert!(crypto::open(
        &key0,
        nonce,
        &dcp_streams::wire::blob_aad("other-doc", &id, 0),
        ct
    )
    .is_err());
    assert!(crypto::open(&crypto::new_epoch_key(), nonce, &aad, ct).is_err());
}

trait LocatorHref {
    fn locator_href(&self) -> &str;
}
impl LocatorHref for dcp_core::FileRef {
    fn locator_href(&self) -> &str {
        match &self.locator {
            dcp_core::Locator::DurableStream { href } => href,
            _ => panic!("not a durable-stream locator"),
        }
    }
}

// ---------------------------------------------------------------- roles

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn writes_by_lesser_roles_are_accepted_by_the_server_and_dropped_by_every_reader() {
    let base = require_streams!();
    let rig = Rig::new(&base).await;
    let n = rig.epoch().await;
    let v = rig.viewer.principal();

    // A viewer sends an update, a lease and a version straight to the server.
    let upd = Update {
        dcp: 1,
        algorithm: ALGORITHM.into(),
        doc_model: DOC_MODEL.into(),
        base_version: rig.first.clone(),
        seq: 0,
        payload: dcp_core::updates::inline_payload(b"evil").unwrap(),
    };
    let st = rig.viewer.http().clock.estimate().unwrap_or(0);
    let accepted = [
        rig.viewer
            .send_unchecked(&rig.doc, Kind::Upd, &serde_json::json!({"st": st, "dev": "VIEWER-DEV", "body": upd}))
            .await
            .unwrap(),
        rig.viewer
            .send_unchecked(
                &rig.doc,
                Kind::Lease,
                &serde_json::json!({"st": st, "seq": 1, "body": {"dcp":1,"state":"held","holder":{"principal": v, "session":"s"},"token":"t","base_version": rig.first,"renewal":0,"duration_ms":600000}}),
            )
            .await
            .unwrap(),
        rig.viewer
            .send_unchecked(&rig.doc, Kind::Ver, &inner_version(&rig.first, &v, st))
            .await
            .unwrap(),
    ];
    assert!(
        accepted.iter().all(Option::is_some),
        "the reference server has no access control: {accepted:?}"
    );
    // An editor rewrites the title (owner-only) and posts a seal (owner-only).
    let mut meta = standard_meta();
    meta.title = "defaced".into();
    let ed = rig.editor.principal();
    let _ = ed;
    let accepted_meta = rig
        .editor
        .send_unchecked(
            &rig.doc,
            Kind::Meta,
            &serde_json::json!({"st": st, "body": meta}),
        )
        .await
        .unwrap();
    assert!(accepted_meta.is_some());

    for who in [&rig.owner, &rig.editor, &rig.viewer] {
        let ids = who.versions(&rig.doc).await.unwrap();
        assert_eq!(
            ids.len(),
            1,
            "a viewer's forged version is visible to {}",
            who.principal()
        );
        assert!(who.updates(&rig.doc).await.unwrap().is_empty());
        assert!(who.lease(&rig.doc).await.unwrap().is_none());
        assert_eq!(
            who.meta(&rig.doc).await.unwrap().title,
            standard_meta().title
        );
    }
    let why = reasons(&rig.owner.dropped(&rig.doc).await);
    assert_eq!(
        why.iter()
            .filter(|r| **r == DropReason::Role(Role::Viewer))
            .count(),
        3,
        "{why:?}"
    );
    assert!(
        why.contains(&DropReason::Role(Role::Editor)),
        "the editor's meta: {why:?}"
    );

    // A seal by a non-owner is not a seal; it does close the stream.
    let log = rig.path(n, "log").await;
    let fake_seal = {
        let mut r = Record {
            v: dcp_streams::wire::WIRE,
            k: Kind::Seal,
            e: n,
            a: rig.editor.principal(),
            n: String::new(),
            c: String::new(),
            s: String::new(),
            x: Some(n + 1),
        };
        r.s = b64(&rig.editor.identity().sign(&r.signed_bytes(&rig.doc)));
        r
    };
    rig.editor
        .http()
        .append(
            &log,
            JSON,
            serde_json::to_vec(&fake_seal).unwrap(),
            None,
            true,
        )
        .await
        .unwrap();
    let after = rig.owner.versions(&rig.doc).await.unwrap();
    assert_eq!(after.len(), 1, "reading still works");
    assert!(reasons(&rig.owner.dropped(&rig.doc).await).contains(&DropReason::BadSeal));
    let r = rig
        .owner
        .publish_version(&rig.doc, vec![rig.first.clone()], new_file(b"x"), None)
        .await;
    assert!(
        matches!(r, Err(StoreError::Conflict(_))),
        "a member who closes the log wedges the epoch for writers (a server with no access control cannot prevent it): {r:?}"
    );
}

// ---------------------------------------------------------------- revocation

async fn decrypt_everything_with_every_key(
    viewer: &StreamsStore,
    doc: &str,
    epoch_path: &str,
    blob_path: &str,
    blob_id: &str,
    blob_epoch: u64,
) -> (usize, usize) {
    let held = viewer.held_keys(&doc.to_owned()).await;
    let mut records_opened = 0;
    for v in raw(viewer, epoch_path).await {
        let Ok(rec) = serde_json::from_value::<Record>(v) else {
            continue;
        };
        if rec.k == Kind::Seal {
            continue;
        }
        for (_, k) in &held {
            if crypto::open(
                k,
                &unb64(&rec.n).unwrap(),
                &Record::aad(doc, rec.k, rec.e, &rec.a),
                &unb64(&rec.c).unwrap(),
            )
            .is_ok()
            {
                records_opened += 1;
            }
        }
    }
    let blob = viewer.http().read_bytes(blob_path).await.unwrap();
    let mut blobs_opened = 0;
    for (_, k) in &held {
        let aad = dcp_streams::wire::blob_aad(doc, blob_id, blob_epoch);
        if crypto::open(k, &blob[13..37], &aad, &blob[37..]).is_ok() {
            blobs_opened += 1;
        }
    }
    (records_opened, blobs_opened)
}

async fn revocation_scenario(cfg: StreamsConfig) -> (usize, usize, Rig, Vec<u8>) {
    let base = std::env::var("DCP_STREAMS_URL").unwrap();
    let rig = Rig::build(&base, cfg).await;
    rig.viewer.sync(&rig.doc).await.unwrap();
    // Before removal the viewer reads everything.
    let before = rig
        .editor
        .publish_version(
            &rig.doc,
            vec![rig.first.clone()],
            new_file(b"before the removal"),
            None,
        )
        .await
        .unwrap();
    let seen = rig.viewer.versions(&rig.doc).await.unwrap();
    let b = seen.iter().find(|v| v.id == before.id).unwrap();
    assert_eq!(
        rig.viewer.fetch_file(&rig.doc, b).await.unwrap(),
        b"before the removal"
    );

    rig.owner
        .revoke(&rig.doc, &rig.viewer.principal())
        .await
        .unwrap();
    let secret = b"published after the viewer was removed".to_vec();
    let hidden = rig
        .editor
        .publish_version(&rig.doc, vec![before.id.clone()], new_file(&secret), None)
        .await
        .unwrap();
    // the control: a member who stayed reads it
    assert_eq!(
        rig.owner.fetch_file(&rig.doc, &hidden).await.unwrap(),
        secret
    );

    let n = rig.epoch().await;
    // the viewer reads the control stream and the epoch's streams as it can
    let _ = rig.viewer.versions(&rig.doc).await;
    let file_id = hidden
        .body
        .file
        .locator_href()
        .rsplit('/')
        .next()
        .unwrap()
        .to_owned();
    let (rec, blob) = decrypt_everything_with_every_key(
        &rig.viewer,
        &rig.doc,
        &rig.path(n, "log").await,
        &format!("dcp/{}/f/{file_id}", rig.doc),
        &file_id,
        n,
    )
    .await;
    (rec, blob, rig, secret)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_removed_member_with_every_key_it_ever_held_reads_nothing_published_afterwards() {
    let _ = require_streams!();
    let (records, blobs, rig, secret) = revocation_scenario(StreamsConfig::new("OWNER-DEV")).await;
    assert_eq!(
        records, 0,
        "the removed member decrypted a record of the new epoch"
    );
    assert_eq!(blobs, 0, "the removed member decrypted the new file");
    let held = rig.viewer.held_keys(&rig.doc).await;
    assert_eq!(
        held.len(),
        3,
        "the viewer holds the keys of epochs 0, 1, 2 and none newer: {:?}",
        held.iter().map(|h| h.0).collect::<Vec<_>>()
    );

    // What the removed member already had stays readable, and nothing newer is listed or readable.
    let listed = rig.viewer.versions(&rig.doc).await.unwrap();
    assert!(listed.iter().any(|v| v.body.message.is_none()));
    assert!(
        listed.len() <= 2,
        "the viewer lists a version published after its removal"
    );
    let owners = rig.owner.versions(&rig.doc).await.unwrap();
    let hidden = owners
        .iter()
        .find(|v| !listed.iter().any(|l| l.id == v.id))
        .unwrap();
    let r = rig.viewer.fetch_file(&rig.doc, hidden).await;
    assert!(matches!(r, Err(StoreError::Forbidden(_))), "{r:?}");
    assert_ne!(r.ok(), Some(secret));
    let r = rig
        .viewer
        .publish_version(&rig.doc, vec![hidden.id.clone()], new_file(b"x"), None)
        .await;
    assert!(matches!(r, Err(StoreError::Forbidden(_))), "{r:?}");

    // The honest server also refuses its appends to the sealed epochs.
    let old = rig.path(2, "log").await;
    let r = rig
        .viewer
        .http()
        .append(&old, JSON, b"{}".to_vec(), None, false)
        .await;
    assert!(
        matches!(r, Err(HttpError::Conflict { closed: true })),
        "{r:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn negative_control_without_rotating_the_key_the_removed_member_reads_everything() {
    let _ = require_streams!();
    let mut cfg = StreamsConfig::new("OWNER-DEV");
    cfg.reuse_epoch_key = true;
    let (records, blobs, _rig, _) = revocation_scenario(cfg).await;
    assert!(records > 0 && blobs > 0, "the check has no power: it passes even when the key is reused ({records} records, {blobs} blobs)");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn negative_control_a_reader_that_skips_the_role_check_accepts_the_viewers_version() {
    let base = require_streams!();
    let rig = Rig::new(&base).await;
    let lenient = {
        let mut cfg = StreamsConfig::new("LENIENT");
        cfg.lenient_roles = true;
        let s = Arc::new(StreamsStore::new(
            crate::common::http(&base),
            Identity::generate(),
            cfg,
        ));
        rig.owner
            .grant(&rig.doc, &s.principal(), Role::Viewer)
            .await
            .unwrap();
        s
    };
    let v = rig.viewer.principal();
    let st = rig.viewer.http().clock.estimate().unwrap_or(0);
    rig.viewer
        .send_unchecked(&rig.doc, Kind::Ver, &inner_version(&rig.first, &v, st))
        .await
        .unwrap();
    assert_eq!(rig.owner.versions(&rig.doc).await.unwrap().len(), 1);
    assert_eq!(
        lenient.versions(&rig.doc).await.unwrap().len(),
        2,
        "the role check is what drops it"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn until_the_old_epoch_is_sealed_a_removed_editor_can_still_write_into_it() {
    let base = require_streams!();
    let mut cfg = StreamsConfig::new("OWNER-DEV");
    cfg.crash_before_sealing = true; // the owner's client dies after the epoch record, before the seals
    let rig = Rig::build(&base, cfg).await;
    let old = rig.epoch().await;
    rig.owner
        .revoke(&rig.doc, &rig.editor.principal())
        .await
        .unwrap();
    assert_eq!(rig.epoch().await, old + 1);

    // The removed editor still holds the old key, still has a signature the old roster honours,
    // and the old streams are still open: its forgery lands BEFORE any seal.
    let st = rig.editor.http().clock.estimate().unwrap_or(0);
    let forged = inner_version(&rig.first, &rig.editor.principal(), st);
    assert!(rig
        .editor
        .send_unchecked(&rig.doc, Kind::Ver, &forged)
        .await
        .unwrap()
        .is_some());
    let seen = rig.owner.versions(&rig.doc).await.unwrap();
    assert_eq!(
        seen.len(),
        2,
        "readers accept it: it is a valid epoch-{old} record from a then-editor, before any seal"
    );

    // Healing seals the old streams; from then on the server refuses and readers would drop.
    rig.owner.heal(&rig.doc).await.unwrap();
    let r = rig
        .editor
        .send_unchecked(&rig.doc, Kind::Ver, &forged)
        .await;
    assert!(matches!(r, Err(StoreError::Conflict(_))), "{r:?}");
    // What was accepted before the seal stays accepted: a seal is not a retraction.
    assert_eq!(rig.viewer.versions(&rig.doc).await.unwrap().len(), 2);
    // And in the new epoch the removed editor can neither read nor write.
    assert!(rig
        .editor
        .held_keys(&rig.doc)
        .await
        .iter()
        .all(|(e, _)| *e <= old));
}

// ---------------------------------------------------------------- the control stream

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_control_stream_ignores_forged_replayed_and_out_of_order_epochs() {
    let base = require_streams!();
    let rig = Rig::new(&base).await;
    let ctl = format!("dcp/{}/ctl", rig.doc);
    let msgs = raw(&rig.owner, &ctl).await;
    assert_eq!(msgs.len(), 3, "epochs 0, 1, 2");
    let epochs_before = rig.owner.epochs(&rig.doc).await.len();

    // Replays of every real epoch record, and an out-of-order one (n=1 last).
    for m in &msgs {
        rig.viewer
            .http()
            .append(&ctl, JSON, serde_json::to_vec(m).unwrap(), None, false)
            .await
            .unwrap();
    }
    // A forged epoch 3 that promotes the viewer, signed by the editor (not an owner),
    // and one signed by the viewer itself.
    let mut forged: dcp_streams::wire::EpochRecord =
        serde_json::from_value(msgs[2].clone()).unwrap();
    forged.n = 3;
    forged.prev = Some(forged_prev(&msgs[2]));
    forged.prefix = "e3-evil".into();
    forged.roster.iter_mut().for_each(|m| {
        if m.p == rig.viewer.principal() {
            m.r = Role::Owner;
        }
    });
    for signer in [rig.editor.identity(), rig.viewer.identity()] {
        let mut f = forged.clone();
        f.a = signer.principal();
        f.s = b64(&signer.sign(&f.signed_bytes()));
        rig.viewer
            .http()
            .append(&ctl, JSON, serde_json::to_vec(&f).unwrap(), None, false)
            .await
            .unwrap();
    }
    rig.viewer
        .http()
        .append(&ctl, JSON, b"[1,2,3]".to_vec(), None, false)
        .await
        .unwrap();

    for who in [&rig.owner, &rig.editor, &rig.viewer] {
        assert_eq!(
            who.role(&rig.doc).await.unwrap(),
            if Arc::ptr_eq(who, &rig.owner) {
                Role::Owner
            } else if Arc::ptr_eq(who, &rig.editor) {
                Role::Editor
            } else {
                Role::Viewer
            }
        );
    }
    assert_eq!(
        rig.owner.epochs(&rig.doc).await.len(),
        epochs_before,
        "no epoch was added by the noise"
    );
    let why = reasons(&rig.owner.dropped(&rig.doc).await);
    assert!(
        why.iter()
            .any(|r| matches!(r, DropReason::BadEpoch(m) if m.contains("owner"))),
        "{why:?}"
    );
    // The noise does not wedge the stream: the owner can still change the roster.
    let extra = store_at(&base, "EXTRA");
    rig.owner
        .grant(&rig.doc, &extra.principal(), Role::Viewer)
        .await
        .unwrap();
    assert_eq!(extra.role(&rig.doc).await.unwrap(), Role::Viewer);
    assert_eq!(extra.versions(&rig.doc).await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_owners_changing_the_roster_at_once_never_fork_the_document() {
    let base = require_streams!();
    let rig = Rig::new(&base).await;
    rig.owner
        .grant(&rig.doc, &rig.editor.principal(), Role::Owner)
        .await
        .unwrap();
    let (mut both, mut one) = (0, 0);
    let mut granted = Vec::new();
    for round in 0..8 {
        let (x, y) = (store_at(&base, "X"), store_at(&base, "Y"));
        let (xp, yp) = (x.principal(), y.principal());
        let (rx, ry) = tokio::join!(
            rig.owner.grant(&rig.doc, &xp, Role::Viewer),
            rig.editor.grant(&rig.doc, &yp, Role::Viewer)
        );
        match (&rx, &ry) {
            (Ok(()), Ok(())) => both += 1,
            (Ok(()), Err(StoreError::Conflict(_))) | (Err(StoreError::Conflict(_)), Ok(())) => {
                one += 1
            }
            other => panic!("round {round}: {other:?}"),
        }
        if rx.is_ok() {
            granted.push(x);
        }
        if ry.is_ok() {
            granted.push(y);
        }
    }
    println!("ROTATION RACE over 8 rounds: both succeeded in sequence {both}, one lost the race and was told so {one}");
    // One chain: every client agrees on the epochs, and everyone who was told "granted" is a member.
    let a = rig.owner.epochs(&rig.doc).await;
    rig.editor.sync(&rig.doc).await.unwrap();
    rig.owner.sync(&rig.doc).await.unwrap();
    let a2 = rig.owner.epochs(&rig.doc).await;
    let b = rig.editor.epochs(&rig.doc).await;
    assert_eq!(
        a2.iter().map(|e| &e.2).collect::<Vec<_>>(),
        b.iter().map(|e| &e.2).collect::<Vec<_>>()
    );
    assert!(a2.len() >= a.len());
    for g in &granted {
        assert_eq!(g.role(&rig.doc).await.unwrap(), Role::Viewer);
        assert!(!g.versions(&rig.doc).await.unwrap().is_empty());
    }
    // And the document still works.
    rig.editor
        .publish_version(
            &rig.doc,
            vec![rig.first.clone()],
            new_file(b"still fine"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(rig.owner.versions(&rig.doc).await.unwrap().len(), 2);
}

fn forged_prev(last: &Value) -> String {
    let rec: dcp_streams::wire::EpochRecord = serde_json::from_value(last.clone()).unwrap();
    rec.id()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replaying_an_offset_is_idempotent_and_a_fresh_reader_agrees_with_an_incremental_one() {
    let base = require_streams!();
    let rig = Rig::new(&base).await;
    let mut parent = rig.first.clone();
    for i in 0..12 {
        let v = rig
            .editor
            .publish_version(
                &rig.doc,
                vec![parent.clone()],
                new_file(format!("v{i}").as_bytes()),
                None,
            )
            .await
            .unwrap();
        parent = v.id;
        if i % 4 == 0 {
            rig.owner.versions(&rig.doc).await.unwrap(); // the owner reads incrementally
        }
    }
    let n = rig.epoch().await;
    let log = rig.path(n, "log").await;
    // Protocol level: the same offset, read twice, gives the same bytes.
    let a = rig
        .owner
        .http()
        .read_chunk(&log, START, true)
        .await
        .unwrap();
    let b = rig
        .owner
        .http()
        .read_chunk(&log, START, true)
        .await
        .unwrap();
    assert_eq!(a.items, b.items);
    assert_eq!(a.next, b.next);
    let mid = rig
        .owner
        .http()
        .read_chunk(&log, START, true)
        .await
        .unwrap()
        .next;
    let again = rig.owner.http().read_chunk(&log, &mid, true).await.unwrap();
    assert!(
        again.items.is_empty(),
        "nothing lies beyond the tail offset"
    );

    // A fresh client (same identity, empty cache) and the incremental one agree.
    let fresh = Arc::new(StreamsStore::new(
        crate::common::http(&base),
        rig.owner.identity().clone(),
        StreamsConfig::new("FRESH"),
    ));
    let mut ours: Vec<String> = rig
        .owner
        .versions(&rig.doc)
        .await
        .unwrap()
        .into_iter()
        .map(|v| v.id)
        .collect();
    let mut theirs: Vec<String> = fresh
        .versions(&rig.doc)
        .await
        .unwrap()
        .into_iter()
        .map(|v| v.id)
        .collect();
    ours.sort();
    theirs.sort();
    assert_eq!(ours, theirs);
    assert_eq!(ours.len(), 13);
    // Appending every record of the log again (a server that replays, or a
    // client that retries) changes nothing.
    for m in raw(&rig.owner, &log).await {
        rig.owner
            .http()
            .append(&log, JSON, serde_json::to_vec(&m).unwrap(), None, false)
            .await
            .unwrap();
    }
    let mut after: Vec<String> = rig
        .viewer
        .versions(&rig.doc)
        .await
        .unwrap()
        .into_iter()
        .map(|v| v.id)
        .collect();
    after.sort();
    assert_eq!(after, ours);
}

// ---------------------------------------------------------------- the server's own guarantees

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_appends_are_all_kept_once_and_every_reader_sees_one_order() {
    let base = require_streams!();
    let http = crate::common::http(&base);
    let path = format!("dcp/order-{}/s", hex::encode(crypto::random::<6>()));
    http.create(&path, JSON, None, false).await.unwrap();
    let mut joins = Vec::new();
    for w in 0..8 {
        let h = http.clone();
        let p = path.clone();
        joins.push(tokio::spawn(async move {
            for i in 0..50 {
                h.append(
                    &p,
                    JSON,
                    format!("{{\"w\":{w},\"i\":{i}}}").into_bytes(),
                    None,
                    false,
                )
                .await
                .unwrap();
            }
        }));
    }
    for j in joins {
        j.await.unwrap();
    }
    let a = http.read_all(&path, START).await.unwrap().0;
    let b = crate::common::http(&base).read_all(&path, START).await.unwrap().0;
    assert_eq!(a.len(), 400, "no append was lost or doubled");
    assert_eq!(a, b, "two readers see the same order");
    // Each writer's own appends are in the order it sent them.
    for w in 0..8 {
        let mine: Vec<i64> = a
            .iter()
            .filter(|m| m["w"] == w)
            .map(|m| m["i"].as_i64().unwrap())
            .collect();
        assert_eq!(mine, (0..50).collect::<Vec<_>>(), "writer {w}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn idempotent_producer_headers_deduplicate_a_retried_append_though_the_binding_does_not_need_them(
) {
    let base = require_streams!();
    let http = crate::common::http(&base);
    let path = format!("dcp/idem-{}/s", hex::encode(crypto::random::<6>()));
    http.create(&path, JSON, None, false).await.unwrap();
    let h = |seq: u64| {
        vec![
            ("producer-id", "writer-1".to_owned()),
            ("producer-epoch", "0".to_owned()),
            ("producer-seq", seq.to_string()),
        ]
    };
    let first = http
        .append_raw(&path, JSON, b"{\"n\":0}".to_vec(), &h(0))
        .await
        .unwrap();
    let again = http
        .append_raw(&path, JSON, b"{\"n\":0}".to_vec(), &h(0))
        .await
        .unwrap();
    let next = http
        .append_raw(&path, JSON, b"{\"n\":1}".to_vec(), &h(1))
        .await
        .unwrap();
    println!(
        "IDEMPOTENT PRODUCER statuses: first {} retry {} next {}",
        first.0, again.0, next.0
    );
    assert_eq!(
        http.read_all(&path, START).await.unwrap().0.len(),
        2,
        "the retry was stored once"
    );
    let gap = http
        .append_raw(&path, JSON, b"{\"n\":9}".to_vec(), &h(9))
        .await;
    assert!(
        matches!(gap, Err(HttpError::Conflict { .. })),
        "a sequence gap is refused: {gap:?}"
    );
}

// ---------------------------------------------------------------- lease

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_lease_is_a_compare_and_set_on_stream_seq() {
    let base = require_streams!();
    let http = crate::common::http(&base);
    // Raw: 8 appenders send the same sequence number; the server lets exactly one in. 20 rounds.
    let path = format!("dcp/cas-{}/lease", hex::encode(crypto::random::<6>()));
    http.create(&path, JSON, None, false).await.unwrap();
    for round in 1..=20u64 {
        let seq = format!("{round:016x}");
        let mut joins = Vec::new();
        for i in 0..8 {
            let h = http.clone();
            let p = path.clone();
            let s = seq.clone();
            joins.push(tokio::spawn(async move {
                h.append(
                    &p,
                    JSON,
                    format!("{{\"round\":{round},\"who\":{i}}}").into_bytes(),
                    Some(&s),
                    false,
                )
                .await
            }));
        }
        let mut ok = 0;
        let mut conflicts = 0;
        for j in joins {
            match j.await.unwrap() {
                Ok(_) => ok += 1,
                Err(HttpError::Conflict { closed: false }) => conflicts += 1,
                Err(e) => panic!("{e}"),
            }
        }
        assert_eq!(
            (ok, conflicts),
            (1, 7),
            "round {round}: Stream-Seq must admit exactly one of eight"
        );
    }
    // A lower or equal sequence is refused, a higher one accepted.
    assert!(matches!(
        http.append(
            &path,
            JSON,
            b"{}".to_vec(),
            Some(&format!("{:016x}", 3)),
            false
        )
        .await,
        Err(HttpError::Conflict { .. })
    ));
    assert!(http
        .append(
            &path,
            JSON,
            b"{}".to_vec(),
            Some(&format!("{:016x}", 99)),
            false
        )
        .await
        .is_ok());

    // Through the store: two editors race to take a free lease; one wins each round.
    let rig = Rig::new(&base).await;
    rig.owner
        .grant(&rig.doc, &rig.viewer.principal(), Role::Editor)
        .await
        .unwrap();
    let (a, b) = (rig.editor.clone(), rig.viewer.clone());
    let mut wins = 0;
    for round in 0..10 {
        let mk = |who: &Arc<StreamsStore>, s: &str| Lease {
            dcp: 1,
            state: LeaseState::Held,
            holder: Some(dcp_core::Holder {
                principal: who.principal(),
                session: format!("{s}-{round}"),
            }),
            token: Some("t".into()),
            base_version: Some(rig.first.clone()),
            renewal: Some(0),
            duration_ms: Some(60_000),
        };
        let (ra, rb) = tokio::join!(
            a.write_lease(&rig.doc, mk(&a, "a")),
            b.write_lease(&rig.doc, mk(&b, "b"))
        );
        let ok = [&ra, &rb].iter().filter(|r| r.is_ok()).count();
        let conflicts = [&ra, &rb]
            .iter()
            .filter(|r| matches!(r, Err(StoreError::Conflict(_))))
            .count();
        assert_eq!((ok, conflicts), (1, 1), "round {round}: {ra:?} / {rb:?}");
        wins += ok;
        let winner = if ra.is_ok() { &a } else { &b };
        winner
            .write_lease(&rig.doc, dcp_core::lease::released())
            .await
            .unwrap();
        // the release is a record over a held one: renewal is not checked for it
    }
    assert_eq!(wins, 10);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn anyone_who_can_append_can_poison_the_lease_stream() {
    let base = require_streams!();
    let rig = Rig::new(&base).await;
    let n = rig.epoch().await;
    let path = rig.path(n, "lease").await;
    // A viewer appends junk carrying the largest sequence number.
    rig.viewer
        .http()
        .append(
            &path,
            JSON,
            b"{\"junk\":true}".to_vec(),
            Some("ffffffffffffffff"),
            false,
        )
        .await
        .unwrap();
    let claim = Lease {
        dcp: 1,
        state: LeaseState::Held,
        holder: Some(dcp_core::Holder {
            principal: rig.editor.principal(),
            session: "s".into(),
        }),
        token: Some("t".into()),
        base_version: Some(rig.first.clone()),
        renewal: Some(0),
        duration_ms: Some(60_000),
    };
    let r = rig.editor.write_lease(&rig.doc, claim).await;
    assert!(
        matches!(r, Err(StoreError::Conflict(_))),
        "with no access control the lease stream can be wedged for the rest of the epoch: {r:?}"
    );
    // Rotating the epoch (any roster change) gives the lease a fresh stream.
    rig.owner
        .grant(&rig.doc, &store_at(&base, "X").principal(), Role::Viewer)
        .await
        .unwrap();
    let claim = Lease {
        dcp: 1,
        state: LeaseState::Held,
        holder: Some(dcp_core::Holder {
            principal: rig.editor.principal(),
            session: "s".into(),
        }),
        token: Some("t".into()),
        base_version: Some(rig.first.clone()),
        renewal: Some(0),
        duration_ms: Some(60_000),
    };
    rig.editor.write_lease(&rig.doc, claim).await.unwrap();
}

// ---------------------------------------------------------------- the server has no access control

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_the_reference_server_anyone_with_a_url_can_delete_and_close_streams() {
    let base = require_streams!();
    let rig = Rig::new(&base).await;
    let v = rig.owner.versions(&rig.doc).await.unwrap();
    let id = v[0]
        .body
        .file
        .locator_href()
        .rsplit('/')
        .next()
        .unwrap()
        .to_owned();
    let stranger = crate::common::http(&base); // no identity, no key, no token
                                     // A removed member, or anyone who learned the document id.
    stranger
        .delete(&format!("dcp/{}/f/{id}", rig.doc))
        .await
        .unwrap();
    let r = rig.viewer.fetch_file(&rig.doc, &v[0]).await;
    assert!(
        matches!(r, Err(StoreError::NotFound(_))),
        "the file is gone for everyone: {r:?}"
    );
    // The ciphertext was safe; the availability was not.
    let n = rig.epoch().await;
    stranger.delete(&rig.path(n, "upd").await).await.unwrap();
    let r = rig
        .editor
        .publish_update(
            &rig.doc,
            StreamWriter::new(ALGORITHM, DOC_MODEL)
                .next(&rig.first, b"x")
                .unwrap(),
        )
        .await;
    assert!(r.is_err(), "appending to a deleted stream must fail: {r:?}");
}

// ---------------------------------------------------------------- snapshots

struct Append(Vec<u8>);
impl Target for Append {
    fn supports(&self, a: &str, m: &str) -> bool {
        a == ALGORITHM && m == DOC_MODEL
    }
    fn dry_run(&self, _: &str, _: &str, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn apply(&mut self, _: &str, _: &str, p: &[u8]) {
        self.0.extend_from_slice(p);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_joiner_bootstraps_from_a_snapshot_and_reads_only_what_follows() {
    let base = require_streams!();
    let rig = Rig::new(&base).await;
    const BEFORE: usize = 300;
    const AFTER: usize = 5;
    let mut w = StreamWriter::new(ALGORITHM, DOC_MODEL);
    let mut truth = BASE_FILE.to_vec();
    for i in 0..BEFORE {
        let p = format!("[{i}]");
        truth.extend_from_slice(p.as_bytes());
        rig.editor
            .publish_update(&rig.doc, w.next(&rig.first, p.as_bytes()).unwrap())
            .await
            .unwrap();
    }
    // The client snapshot: a version that covers every update so far.
    let cover = rig
        .editor
        .publish_covering_version(
            &rig.doc,
            vec![rig.first.clone()],
            new_file(&truth),
            Some("snapshot".into()),
            ALGORITHM,
        )
        .await
        .unwrap();
    assert!(cover.body.covers.is_some());
    let mut tail = Vec::new();
    for i in 0..AFTER {
        let p = format!("<{i}>");
        tail.extend_from_slice(p.as_bytes());
        rig.editor
            .publish_update(&rig.doc, w.next(&cover.id, p.as_bytes()).unwrap())
            .await
            .unwrap();
    }
    // Two members join late: one reads from the snapshot, one reads everything.
    let late = store_at(&base, "LATE");
    let mut full_cfg = StreamsConfig::new("FULL");
    full_cfg.use_snapshots = false;
    let full = Arc::new(StreamsStore::new(
        crate::common::http(&base),
        Identity::generate(),
        full_cfg,
    ));
    rig.owner
        .grant(&rig.doc, &late.principal(), Role::Viewer)
        .await
        .unwrap();
    rig.owner
        .grant(&rig.doc, &full.principal(), Role::Viewer)
        .await
        .unwrap();

    let before_late = late.http().snapshot();
    let ups = late.updates(&rig.doc).await.unwrap();
    let late_cost = late.http().snapshot();
    let before_full = full.http().snapshot();
    let all = full.updates(&rig.doc).await.unwrap();
    let full_cost = full.http().snapshot();
    assert_eq!(
        ups.len(),
        AFTER,
        "the snapshot skipped the {BEFORE} covered updates"
    );
    assert_eq!(all.len(), BEFORE + AFTER);
    let late_bytes = late_cost.bytes_received - before_late.bytes_received;
    let full_bytes = full_cost.bytes_received - before_full.bytes_received;
    assert!(
        late_bytes * 2 < full_bytes,
        "the snapshot saved nothing: {late_bytes} vs {full_bytes}"
    );
    println!(
        "SNAPSHOT bootstrap: late joiner {} requests {} bytes; without snapshot {} requests {} bytes",
        late_cost.requests - before_late.requests,
        late_bytes,
        full_cost.requests - before_full.requests,
        full_bytes
    );

    // The late joiner's state: the head's file, then the updates based on it.
    let view = dcp_core::view::read_view(&*late, &rig.doc).await.unwrap();
    let head = view
        .heads
        .iter()
        .find(|h| h.version.id == cover.id)
        .expect("the covering version is a head");
    assert_eq!(head.bytes, truth);
    let mut rx = Receiver::new(None);
    rx.reload(&head.version);
    let mut state = Append(head.bytes.clone());
    let report = rx.ingest(&ups, &mut state);
    assert!(report.gaps.is_empty() && !rx.is_stopped(), "{report:?}");
    let mut want = truth.clone();
    want.extend_from_slice(&tail);
    assert_eq!(state.0, want);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_version_through_a_prefix_claims_the_prefix_and_writes_no_resume_hint() {
    let base = require_streams!();
    let rig = Rig::new(&base).await;
    let mut w = StreamWriter::new(ALGORITHM, DOC_MODEL);
    for i in 0..4 {
        let p = format!("[{i}]");
        rig.editor
            .publish_update(&rig.doc, w.next(&rig.first, p.as_bytes()).unwrap())
            .await
            .unwrap();
    }
    let log = rig.editor.updates(&rig.doc).await.unwrap();
    assert_eq!(log.len(), 4);
    // The editor holds the first two updates only.
    let claim = Covers {
        algorithm: ALGORITHM.to_owned(),
        through: log[1].id.clone(),
    };
    let v = rig
        .editor
        .publish_version_through(
            &rig.doc,
            vec![rig.first.clone()],
            new_file(b"after two"),
            None,
            Some(claim.clone()),
        )
        .await
        .unwrap();
    assert_eq!(v.body.covers, Some(claim.clone()));
    let plain = rig
        .editor
        .publish_version_through(
            &rig.doc,
            vec![v.id.clone()],
            new_file(b"no claim"),
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(plain.body.covers, None);

    // A late reader sees the claim as written and still reads the whole log: with
    // no resume hint, nothing lets it skip the two updates the claim leaves out.
    let late = store_at(&base, "LATE");
    rig.owner
        .grant(&rig.doc, &late.principal(), Role::Viewer)
        .await
        .unwrap();
    let versions = late.versions(&rig.doc).await.unwrap();
    let seen = versions.iter().find(|x| x.id == v.id).expect("the version");
    assert_eq!(seen.body.covers, Some(claim));
    let seen_log = late.updates(&rig.doc).await.unwrap();
    assert_eq!(
        seen_log.iter().map(|u| u.id.as_str()).collect::<Vec<_>>(),
        log.iter().map(|u| u.id.as_str()).collect::<Vec<_>>(),
        "the late reader reads every update, in the order the first reader saw"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_snapshot_hint_cannot_be_checked_by_the_server_so_the_reader_bounds_it() {
    let base = require_streams!();
    let http = crate::common::http(&base);
    // What the server says to an offset past the tail (a measurement, whichever server this is).
    let path = format!("dcp/hint-{}/s", hex::encode(crypto::random::<6>()));
    http.create(&path, JSON, Some(b"[{\"a\":1},{\"a\":2}]".to_vec()), false)
        .await
        .unwrap();
    let tail = http.head(&path).await.unwrap().next;
    let beyond = "9999999999999999_9999999999999999";
    let r = http.read_chunk(&path, beyond, true).await;
    println!(
        "HINT beyond the tail ({tail} -> {beyond}): {}",
        match &r {
            Ok(x) => format!("200 with {} items, next {}", x.items.len(), x.next),
            Err(e) => format!("error: {e}"),
        }
    );
    assert!(beyond > tail.as_str(), "offsets sort lexicographically");

    // Through the binding: an editor publishes a covering version whose hint lies.
    let rig = Rig::new(&base).await;
    let mut w = StreamWriter::new(ALGORITHM, DOC_MODEL);
    for i in 0..5 {
        rig.editor
            .publish_update(
                &rig.doc,
                w.next(&rig.first, format!("u{i}").as_bytes()).unwrap(),
            )
            .await
            .unwrap();
    }
    let n = rig.epoch().await;
    let st = rig.editor.http().clock.estimate().unwrap_or(0);
    let mut inner = inner_version(&rig.first, &rig.editor.principal(), st);
    inner["body"]["covers"] = serde_json::json!({"algorithm": ALGORITHM, "through": "x"});
    inner["hint"] = serde_json::json!({"epoch": n, "offset": beyond});
    assert!(rig
        .editor
        .send_unchecked(&rig.doc, Kind::Ver, &inner)
        .await
        .unwrap()
        .is_some());
    let late = store_at(&base, "LATE");
    rig.owner
        .grant(&rig.doc, &late.principal(), Role::Viewer)
        .await
        .unwrap();
    let got = late.updates(&rig.doc).await.unwrap().len();
    assert_eq!(
        got, 5,
        "a hint beyond the tail is ignored and the reader reads everything"
    );
}

// ---------------------------------------------------------------- time

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_server_clock_estimate_is_a_lower_bound_within_the_poll_interval() {
    let base = require_streams!();
    let s = store_at(&base, "CLOCK");
    s.calibrate().await.unwrap();
    let doc = "none".to_owned();
    let mut errs = Vec::new();
    for _ in 0..40 {
        let est = s.now_ms(&doc).await.unwrap() as i64;
        let truth = dcp_streams::clock::local_wall_ms() as i64; // server and test share this host
        errs.push(est - truth);
        tokio::time::sleep(Duration::from_millis(137)).await;
    }
    errs.sort();
    println!(
        "CLOCK error ms (estimate - truth): min {} p50 {} max {}",
        errs[0],
        errs[errs.len() / 2],
        errs[errs.len() - 1]
    );
    assert!(
        errs[0] > -1000,
        "the estimate is never more than the Date resolution behind: {errs:?}"
    );
    assert!(
        *errs.last().unwrap() <= 50,
        "the estimate is a lower bound (to within the host's own jitter): {errs:?}"
    );
}

// ---------------------------------------------------------------- uploads

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upload_cut_off_on_the_wire_leaves_no_stream_behind() {
    let base = require_streams!();
    let tag = hex::encode(crypto::random::<6>());
    let http = crate::common::http(&base);

    // Control: the same hand-written request, finished, makes a stream. Without it "no stream"
    // could just mean the request was malformed.
    let whole = format!("dcp/whole-{tag}/f/x");
    crate::rawtls::cut_upload(&base, &whole, 5000, 5000, true).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let h = http.head(&whole).await;
    assert!(h.is_ok(), "the control upload did not create a stream: {h:?}");

    let cut = format!("dcp/cut-{tag}/f/x");
    crate::rawtls::cut_upload(&base, &cut, 200_000, 5000, false).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let r = http.head(&cut).await;
    assert!(
        matches!(r, Err(HttpError::NotFound)),
        "a half-received upload became a stream: {r:?}"
    );
}
