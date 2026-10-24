//! Focused journeys for the Matrix binding against a real Synapse. These
//! assert behaviour the design requires. Anything whose outcome is itself the
//! finding (latency, limits, late joiners, ...) lives in `measure.rs`.
//!
//! Needs `DCP_SYNAPSE_URL` (scripts/synapse-up.sh); fails without it unless
//! `DCP_SKIP_SYNAPSE=1`.

mod common;

use common::*;
use dcp_core::*;
use dcp_matrix::{state, MatrixStore};
use serde_json::json;
use sha2::{Digest, Sha256};

fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

fn heads(vs: &[Stamped<Version>]) -> Vec<String> {
    let parents: std::collections::HashSet<&str> = vs.iter().flat_map(|v| v.body.parents.iter().map(String::as_str)).collect();
    vs.iter().filter(|v| !parents.contains(v.id.as_str())).map(|v| v.id.clone()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_users_converge_on_the_same_head_and_files_verify() {
    let Some(syn) = require_synapse!() else { return };
    let alice = syn.user("alice").await;
    let bob = syn.user("bob").await;
    assert!(alice.init_warnings().is_empty(), "alice init warnings: {:?}", alice.init_warnings());

    let doc = alice.create_document(&meta("Quarterly report"), None).await.unwrap();
    assert_eq!(alice.role(&doc).await.unwrap(), Role::Owner);
    alice.grant(&doc, &bob.principal(), Role::Editor).await.unwrap();
    assert_eq!(bob.role(&doc).await.unwrap(), Role::Editor);
    assert_eq!(bob.meta(&doc).await.unwrap().title, "Quarterly report");

    let bytes1 = noise(5000, 1);
    let v1 = alice.publish_version(&doc, vec![], file(&bytes1), Some("first".into())).await.unwrap();
    assert_eq!(v1.sender, alice.principal());
    assert_eq!(v1.body.created_by, Author { principal: alice.principal(), device: alice.device() });
    assert_eq!(v1.body.file.sha256, sha(&bytes1));
    assert_eq!(v1.body.file.size, 5000);
    assert!(matches!(v1.body.file.locator, Locator::MatrixEncryptedMedia { .. }));
    assert!(v1.id.starts_with('$'), "version id is a Matrix event id: {}", v1.id);

    let seen = bob.versions(&doc).await.unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].id, v1.id);
    assert_eq!(seen[0].ts_ms, v1.ts_ms, "origin_server_ts is the same for both readers");
    assert_eq!(seen[0].device.as_deref(), Some(alice.device().as_str()), "the sending device comes from the megolm session");
    let fetched = bob.fetch_file(&doc, &seen[0]).await.unwrap();
    assert_eq!(fetched, bytes1);

    let bytes2 = noise(7000, 2);
    let v2 = bob.publish_version(&doc, vec![v1.id.clone()], file(&bytes2), None).await.unwrap();
    let a = alice.versions(&doc).await.unwrap();
    let b = bob.versions(&doc).await.unwrap();
    assert_eq!(a.len(), 2);
    assert_eq!(heads(&a), vec![v2.id.clone()]);
    assert_eq!(heads(&a), heads(&b), "both instances converge on the same head");
    let head = a.iter().find(|v| v.id == v2.id).unwrap();
    assert_eq!(alice.fetch_file(&doc, head).await.unwrap(), bytes2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn viewer_is_refused_by_the_store_and_by_the_server() {
    let Some(syn) = require_synapse!() else { return };
    let alice = syn.user("alice").await;
    let vera = syn.user("vera").await;
    let doc = alice.create_document(&meta("Roles"), Some(file(b"v1"))).await.unwrap();
    alice.grant(&doc, &vera.principal(), Role::Viewer).await.unwrap();
    assert_eq!(vera.role(&doc).await.unwrap(), Role::Viewer);

    // The store's own check.
    let e = vera.publish_version(&doc, vec![], file(b"x"), None).await.unwrap_err();
    assert!(matches!(e, StoreError::Forbidden(_)), "{e:?}");
    let lease = Lease { dcp: 1, state: LeaseState::Held, holder: Some(Holder { principal: vera.principal(), session: "s".into() }), token: Some("t".into()), base_version: Some("$x".into()), renewal: Some(0), duration_ms: Some(60_000) };
    assert!(matches!(vera.write_lease(&doc, lease.clone()).await.unwrap_err(), StoreError::Forbidden(_)));
    assert!(matches!(vera.grant(&doc, &alice.principal(), Role::Viewer).await.unwrap_err(), StoreError::Forbidden(_)));

    // The homeserver's check, bypassing the store: this is "enforced at the
    // binding, not only in the UI". A DCP timeline event in an encrypted room is
    // an `m.room.encrypted` event to the server, so it is that type's level that
    // counts (see the results doc).
    let v = serde_json::to_value(Version { dcp: 1, parents: vec![], file: FileRef { mime: "x/y".into(), size: 1, sha256: sha(b"x"), locator: Locator::MatrixMedia { url: "mxc://localhost/x".into() } }, created_by: Author { principal: vera.principal(), device: vera.device() }, message: None, covers: None }).unwrap();
    let e = vera.send_unchecked(&doc, state::VERSION, v).await.unwrap_err();
    assert!(matches!(e, StoreError::Forbidden(_)), "server must refuse a viewer's encrypted timeline event: {e:?}");
    let e = vera.send_state_unchecked(&doc, state::LEASE, "", serde_json::to_value(&lease).unwrap()).await.unwrap_err();
    assert!(matches!(e, StoreError::Forbidden(_)), "server must refuse a viewer's lease state event: {e:?}");
    let e = vera.send_state_unchecked(&doc, state::META, "", serde_json::to_value(meta("hijack")).unwrap()).await.unwrap_err();
    assert!(matches!(e, StoreError::Forbidden(_)), "{e:?}");

    // An editor is not an owner.
    let ed = syn.user("ed").await;
    alice.grant(&doc, &ed.principal(), Role::Editor).await.unwrap();
    ed.publish_version(&doc, vec![], file(b"fine"), None).await.unwrap();
    assert!(matches!(ed.grant(&doc, &vera.principal(), Role::Editor).await.unwrap_err(), StoreError::Forbidden(_)));
    assert!(matches!(ed.send_state_unchecked(&doc, state::META, "", serde_json::to_value(meta("hijack")).unwrap()).await.unwrap_err(), StoreError::Forbidden(_)));
    assert!(matches!(alice.grant(&doc, &ed.principal(), Role::Commenter).await.unwrap_err(), StoreError::Invalid(_)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spec_literal_power_table_does_not_stop_a_viewers_encrypted_event_but_the_reader_does() {
    let Some(syn) = require_synapse!() else { return };
    // The spec's table, taken literally: nothing for `m.room.encrypted`.
    let alice = syn.user_with("alice", |c| c.enforce_encrypted_pl = false).await;
    let vera = syn.user("vera").await;
    let doc = alice.create_document(&meta("Literal"), Some(file(b"v1"))).await.unwrap();
    alice.grant(&doc, &vera.principal(), Role::Viewer).await.unwrap();
    let snap = alice.snapshot(&doc).await.unwrap();
    assert!(!snap.power.server_gates_encrypted_timeline());
    assert_eq!(snap.power.events.get(state::VERSION), Some(&50));

    let forged = serde_json::to_value(Version { dcp: 1, parents: vec![], file: FileRef { mime: "x/y".into(), size: 1, sha256: sha(b"x"), locator: Locator::MatrixMedia { url: "mxc://localhost/x".into() } }, created_by: Author { principal: vera.principal(), device: vera.device() }, message: Some("forged by a viewer".into()), covers: None }).unwrap();
    // Server accepts it: this is the hole in the spec's table.
    vera.send_unchecked(&doc, state::VERSION, forged).await.expect("server accepts a viewer's encrypted timeline event under the spec's literal table");

    let rep = alice.read_report(&doc).await.unwrap();
    assert_eq!(rep.versions.len(), 1, "only the owner's real version is returned");
    assert_eq!(rep.rejected_unauthorised, 1, "the reader's own check caught the forged one");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn update_roundtrip_and_inline_limit() {
    let Some(syn) = require_synapse!() else { return };
    use base64::Engine;
    let alice = syn.user("alice").await;
    let bob = syn.user("bob").await;
    let doc = alice.create_document(&meta("Updates"), Some(file(b"v1"))).await.unwrap();
    alice.grant(&doc, &bob.principal(), Role::Editor).await.unwrap();

    let mk = |len: usize, seq: u64, size_field: Option<u64>| {
        let raw = noise(len, seq);
        Update { dcp: 1, algorithm: "loro/1".into(), doc_model: "tables/1".into(), base_version: "$base".into(), seq, payload: Payload::Base64 { data: base64::engine::general_purpose::STANDARD.encode(&raw), size: size_field.unwrap_or(len as u64) } }
    };
    let u0 = mk(1000, 0, None);
    let sent = alice.publish_update(&doc, u0.clone()).await.unwrap();
    assert_eq!(sent.body, u0);
    let seen = bob.updates(&doc).await.unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].id, sent.id);
    assert_eq!(seen[0].body, u0);
    assert_eq!(seen[0].device.as_deref(), Some(alice.device().as_str()));

    assert!(matches!(alice.publish_update(&doc, mk(34_001, 1, None)).await.unwrap_err(), StoreError::Invalid(_)), "over the spec limit");
    assert!(matches!(alice.publish_update(&doc, mk(10, 1, Some(11))).await.unwrap_err(), StoreError::Invalid(_)), "size field must match");
    // Updates are not versions.
    assert_eq!(bob.versions(&doc).await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lease_is_a_state_event_with_server_timestamps_and_read_back() {
    let Some(syn) = require_synapse!() else { return };
    let alice = syn.user("alice").await;
    let bob = syn.user("bob").await;
    let vera = syn.user("vera").await;
    let doc = alice.create_document(&meta("Lease"), Some(file(b"v1"))).await.unwrap();
    alice.grant(&doc, &bob.principal(), Role::Editor).await.unwrap();
    alice.grant(&doc, &vera.principal(), Role::Viewer).await.unwrap();
    assert!(alice.lease(&doc).await.unwrap().is_none(), "never written");

    let v1 = alice.versions(&doc).await.unwrap().remove(0);
    // Rule 8: renewal is 0 over a record that is not held, previous + 1 over a held one.
    let held = |who: &MatrixStore, session: &str, renewal: u64| Lease { dcp: 1, state: LeaseState::Held, holder: Some(Holder { principal: who.principal(), session: session.into() }), token: Some(format!("tok-{session}")), base_version: Some(v1.id.clone()), renewal: Some(renewal), duration_ms: Some(60_000) };
    let written = alice.write_lease(&doc, held(&alice, "alice-s1", 0)).await.unwrap();
    let read = bob.lease(&doc).await.unwrap().expect("bob reads alice's lease");
    assert_eq!(read.id, written.id);
    assert_eq!(read.ts_ms, written.ts_ms);
    assert_eq!(read.body.holder.as_ref().unwrap().session, "alice-s1");
    assert_eq!(read.sender, alice.principal());
    assert_eq!(vera.lease(&doc).await.unwrap().unwrap().id, written.id, "a viewer can read it");

    // The binding's clock and the lease's server timestamp are comparable.
    let now = bob.now_ms(&doc).await.unwrap();
    let skew = now as i64 - written.ts_ms as i64;
    assert!((-2_000..30_000).contains(&skew), "Date-header clock vs origin_server_ts skew {skew} ms");

    // Bob overwrites (no compare-and-set); the read-back names him.
    let b = bob.write_lease(&doc, held(&bob, "bob-s1", 1)).await.unwrap();
    let back = alice.lease(&doc).await.unwrap().unwrap();
    assert_eq!(back.id, b.id);
    assert_eq!(back.body.holder.unwrap().principal, bob.principal());
    assert!(back.ts_ms >= written.ts_ms);

    // Release.
    let released = Lease { dcp: 1, state: LeaseState::Released, holder: None, token: None, base_version: None, renewal: None, duration_ms: None };
    bob.write_lease(&doc, released.clone()).await.unwrap();
    assert_eq!(alice.lease(&doc).await.unwrap().unwrap().body, released);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn simultaneous_lease_writers_agree_on_one_winner_after_settling() {
    let Some(syn) = require_synapse!() else { return };
    let alice = syn.user("alice").await;
    let bob = syn.user("bob").await;
    let doc = alice.create_document(&meta("Race"), Some(file(b"v1"))).await.unwrap();
    alice.grant(&doc, &bob.principal(), Role::Editor).await.unwrap();
    let base = alice.versions(&doc).await.unwrap().remove(0).id;
    let held = |who: &MatrixStore, s: &str, renewal: u64| Lease { dcp: 1, state: LeaseState::Held, holder: Some(Holder { principal: who.principal(), session: s.into() }), token: Some(s.into()), base_version: Some(base.clone()), renewal: Some(renewal), duration_ms: Some(60_000) };
    // Warm both so the race is the writes, not first-use setup.
    alice.write_lease(&doc, held(&alice, "warm-a", 0)).await.unwrap();
    bob.write_lease(&doc, held(&bob, "warm-b", 1)).await.unwrap();

    for round in 0..5 {
        let (sa, sb) = (format!("a{round}"), format!("b{round}"));
        // Both replace the same held record, so both carry the same next renewal.
        let renewal = round + 2;
        let (wa, wb) = tokio::join!(alice.write_lease(&doc, held(&alice, &sa, renewal)), bob.write_lease(&doc, held(&bob, &sb, renewal)));
        let (wa, wb) = (wa.unwrap(), wb.unwrap());
        assert_ne!(wa.id, wb.id, "no compare-and-set: both writes succeed");
        sleep_ms(600).await;
        let (ra, rb) = (alice.lease(&doc).await.unwrap().unwrap(), bob.lease(&doc).await.unwrap().unwrap());
        assert_eq!(ra.id, rb.id, "after settling, both read the same record");
        assert!(ra.id == wa.id || ra.id == wb.id);
        let winner = ra.body.holder.unwrap().session;
        assert!(winner == sa || winner == sb);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removed_member_cannot_read_the_next_version_with_a_real_device() {
    let Some(syn) = require_synapse!() else { return };
    let alice = syn.user("alice").await;
    let bob = syn.user("bob").await;
    let carol = syn.user("carol").await;
    let v1bytes = noise(3000, 11);
    let doc = alice.create_document(&meta("Revoke"), Some(file(&v1bytes))).await.unwrap();
    alice.grant(&doc, &bob.principal(), Role::Editor).await.unwrap();
    alice.grant(&doc, &carol.principal(), Role::Viewer).await.unwrap();
    // Both members join and read v1.
    let v1 = alice.versions(&doc).await.unwrap().remove(0);
    for s in [&bob, &carol] {
        let vs = s.versions(&doc).await.unwrap();
        assert_eq!(vs.len(), 1, "member sees v1");
        assert_eq!(s.fetch_file(&doc, &vs[0]).await.unwrap(), v1bytes);
    }

    alice.revoke(&doc, &bob.principal()).await.unwrap();
    let v2bytes = noise(3000, 12);
    let v2 = alice.publish_version(&doc, vec![v1.id.clone()], file(&v2bytes), None).await.unwrap();

    // Remaining member reads it.
    assert_eq!(carol.versions(&doc).await.unwrap().len(), 2);

    // The removed principal cannot read through the store...
    assert!(bob.versions(&doc).await.is_err(), "removed principal must not list versions");
    assert!(matches!(bob.role(&doc).await, Err(StoreError::Forbidden(_))));
    // ...the homeserver refuses the event to them...
    let code = server_event(&syn, &bob, &doc, &v2.id).await.unwrap_err();
    assert!(code == 403 || code == 404, "server answered the removed member with {code}");

    // ...and, taking the server out of the equation (the ciphertext is handed to
    // the removed device by someone who can fetch it), the device's keys do not
    // open v2, but do still open v1 (spec 11: content already received stays readable).
    let enc1 = server_event(&syn, &alice, &doc, &v1.id).await.unwrap();
    let enc2 = server_event(&syn, &alice, &doc, &v2.id).await.unwrap();
    assert_eq!(enc2["type"], "m.room.encrypted");
    assert!(can_decrypt(&carol, &doc, &enc2).await, "control: a remaining member's device opens v2");
    assert!(can_decrypt(&bob, &doc, &enc1).await, "removed device keeps v1 (already received)");
    assert!(!can_decrypt(&bob, &doc, &enc2).await, "removed device must NOT open v2");
    let s1 = session_id_of(&alice, &doc, &v1.id).await.expect("v1 session");
    let s2 = session_id_of(&alice, &doc, &v2.id).await.expect("v2 session");
    assert_ne!(s1, s2, "the group session was rotated when the member was removed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_plain_room_is_not_a_document() {
    let Some(syn) = require_synapse!() else { return };
    let alice = syn.user("alice").await;
    use dcp_matrix::matrix_sdk::ruma::api::client::room::create_room;
    let room = alice.client().create_room(create_room::v3::Request::new()).await.unwrap();
    let e = alice.meta(&room.room_id().to_string()).await.unwrap_err();
    assert!(matches!(e, StoreError::NotFound(_)), "{e:?}");
    let _ = json!({});
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn renewal_counter_is_what_makes_a_renewal_a_new_event() {
    let Some(syn) = require_synapse!() else { return };
    let alice = syn.user("alice").await;
    let doc = alice.create_document(&meta("Renew"), Some(file(b"v1"))).await.unwrap();
    let base = alice.versions(&doc).await.unwrap().remove(0).id;
    let l = Lease { dcp: 1, state: LeaseState::Held, holder: Some(Holder { principal: alice.principal(), session: "s".into() }), token: Some("t".into()), base_version: Some(base), renewal: Some(0), duration_ms: Some(1_800_000) };
    let first = alice.write_lease(&doc, l.clone()).await.unwrap();
    sleep_ms(1100).await;

    // Why spec 4.3 rule 8 exists: the same record again from the same sender
    // is silently ignored by Synapse, which answers with the old event.
    let again = alice.write_lease(&doc, l.clone()).await.unwrap();
    assert_eq!(again.id, first.id, "Synapse hands back the old event for identical state content");
    assert_eq!(alice.lease(&doc).await.unwrap().unwrap().ts_ms, first.ts_ms, "and nothing was extended");

    // A renewal over a held record carries renewal + 1 and is a new event.
    let renewed = Lease { renewal: Some(1), ..l.clone() };
    let second = alice.write_lease(&doc, renewed.clone()).await.unwrap();
    assert_ne!(first.id, second.id, "a renewal must be a new event, not the old one handed back");
    assert!(second.ts_ms > first.ts_ms);
    let read = alice.lease(&doc).await.unwrap().unwrap();
    assert_eq!(read.id, second.id);
    assert_eq!(read.body, renewed, "the binding writes exactly what it is given");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_editor_granted_right_after_creation_reads_the_first_version_at_once() {
    let Some(syn) = require_synapse!() else { return };
    // Regression for a measured failure: the invite is followed at once by a
    // power-level event, and the invitee joins by room ID straight away. If the
    // store joins before its own sync has delivered the invite, matrix-sdk never
    // imports the shared history and the first version stays undecryptable.
    for _ in 0..3 {
        let alice = syn.user_with("alice", |c| c.events_per_sec = 0.0).await;
        let late = syn.user("late").await;
        let bytes = noise(900, 5);
        let doc = alice.create_document(&meta("Late editor"), Some(file(&bytes))).await.unwrap();
        alice.grant(&doc, &late.principal(), Role::Editor).await.unwrap();
        let rep = late.read_report(&doc).await.unwrap();
        assert_eq!(rep.versions.len(), 1, "undecryptable: {}", rep.undecryptable);
        assert_eq!(late.fetch_file(&doc, &rep.versions[0]).await.unwrap(), bytes);
    }
}
