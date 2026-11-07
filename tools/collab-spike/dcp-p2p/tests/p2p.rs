//! What the conformance journeys cannot see: catching up after being offline, a tampered entry, a
//! removed member who still receives ciphertext, partitions, members that are never online
//! together, blob hashes and the blob gate, and clock skew against the lease.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use dcp_conformance::{new_file, run_one, Outcome};
use dcp_core::view::read_view;
use async_trait::async_trait;
use dcp_core::lease::{self, Acquire, Claim, Pause, Settle};
use dcp_core::{graph, DocumentStore, Role, StoreError};
use dcp_p2p::envelope::blob_of_locator;
use dcp_p2p::net::{read_status, write_hello, Hello, STATUS_OK, SYNC_ALPN};
use dcp_p2p::P2pStore;
use p2panda_core::{Hash, Header};
use p2panda_sync::protocols::LogSyncMessage;
use tokio_util::codec::{FramedRead, FramedWrite};

async fn head_ids(n: &Arc<P2pStore>, doc: &String) -> Vec<String> {
    match read_view(&**n, doc).await {
        Ok(v) => v.head_ids().into_iter().map(str::to_owned).collect(),
        Err(_) => Vec::new(),
    }
}

async fn first_version(n: &Arc<P2pStore>, doc: &String) -> String {
    n.versions(doc).await.unwrap().into_iter().find(|v| v.body.parents.is_empty()).unwrap().id
}

// ------------------------------------------------------------------- offline and catch-up

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_peer_offline_during_publishes_catches_up_on_reconnect() {
    init_logging();
    let (_owner, editor, viewer, doc) = default_trio().await;
    viewer.set_online(false).await.unwrap();
    let mut parent = first_version(&editor, &doc).await;
    for i in 0..5 {
        let v = editor
            .publish_version(&doc, vec![parent.clone()], new_file(format!("edit {i} while the viewer is away").as_bytes()), None)
            .await
            .unwrap();
        parent = v.id;
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        head_ids(&viewer, &doc).await,
        vec![first_version(&viewer, &doc).await],
        "an offline viewer must not see edits made while it was away"
    );
    let t0 = Instant::now();
    viewer.set_online(true).await.unwrap();
    wait_for("the viewer catches up", Duration::from_secs(30), || async {
        (head_ids(&viewer, &doc).await == vec![parent.clone()]).then_some(())
    })
    .await;
    let v = read_view(&*viewer, &doc).await.unwrap();
    assert!(v.rejected.is_empty());
    assert_eq!(v.heads[0].bytes, b"edit 4 while the viewer is away");
    eprintln!("MEASURE catch_up_5_versions_after_reconnect_ms={}", t0.elapsed().as_millis());
}

/// Two members edit while both are offline: on reconnect there are two heads, then a merge.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_publishes_from_two_peers_give_two_heads_and_a_merge_heals_it() {
    init_logging();
    let (owner, editor, viewer, doc) = default_trio().await;
    let first = first_version(&owner, &doc).await;
    owner.set_online(false).await.unwrap();
    editor.set_online(false).await.unwrap();
    let a = owner.publish_version(&doc, vec![first.clone()], new_file(b"owner offline edit"), None).await.unwrap();
    let b = editor.publish_version(&doc, vec![first.clone()], new_file(b"editor offline edit"), None).await.unwrap();
    owner.set_online(true).await.unwrap();
    editor.set_online(true).await.unwrap();
    let t0 = Instant::now();
    for n in [&owner, &editor, &viewer] {
        wait_for("two heads everywhere", Duration::from_secs(30), || async {
            (head_ids(n, &doc).await.len() == 2).then_some(())
        })
        .await;
    }
    eprintln!("MEASURE two_heads_visible_everywhere_after_reconnect_ms={}", t0.elapsed().as_millis());
    let mut ids = head_ids(&viewer, &doc).await;
    ids.sort();
    let mut want = vec![a.id.clone(), b.id.clone()];
    want.sort();
    assert_eq!(ids, want, "the viewer sees both heads, none hidden");
    let view = read_view(&*owner, &doc).await.unwrap();
    let heads: Vec<_> = view.heads.iter().map(|h| &h.version).collect();
    let merged = owner
        .publish_version(&doc, graph::merge_parents(&heads), new_file(b"merged"), None)
        .await
        .unwrap();
    for n in [&owner, &editor, &viewer] {
        wait_for("one head after the merge", Duration::from_secs(30), || async {
            (head_ids(n, &doc).await == vec![merged.id.clone()]).then_some(())
        })
        .await;
    }
}

// ------------------------------------------------------------------- never online together

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn members_never_online_together_need_a_third_member_that_overlaps_both() {
    init_logging();
    let (owner, editor, viewer, doc) = default_trio().await;
    let first = first_version(&owner, &doc).await;

    // Phase 1: the viewer is away; the editor publishes, then goes away; the viewer returns. The
    // editor and the viewer were never online together, but the owner was online with both.
    viewer.set_online(false).await.unwrap();
    let v2 = editor.publish_version(&doc, vec![first.clone()], new_file(b"v2 from the editor"), None).await.unwrap();
    wait_for("the owner holds v2", Duration::from_secs(30), || async {
        owner.versions(&doc).await.ok()?.iter().any(|v| v.id == v2.id).then_some(())
    })
    .await;
    editor.set_online(false).await.unwrap();
    let t0 = Instant::now();
    viewer.set_online(true).await.unwrap();
    wait_for("the viewer gets v2 through the owner", Duration::from_secs(30), || async {
        viewer.versions(&doc).await.ok()?.iter().any(|v| v.id == v2.id).then_some(())
    })
    .await;
    eprintln!("MEASURE via_third_member_ms={}", t0.elapsed().as_millis());

    // Phase 2: nobody else holds v3. The owner and the viewer are away while the editor publishes,
    // and the editor leaves before either returns: v3 exists only on the editor.
    owner.set_online(false).await.unwrap();
    viewer.set_online(false).await.unwrap();
    editor.set_online(true).await.unwrap();
    let v3 = editor.publish_version(&doc, vec![v2.id.clone()], new_file(b"v3 from the editor"), None).await.unwrap();
    editor.set_online(false).await.unwrap();
    owner.set_online(true).await.unwrap();
    viewer.set_online(true).await.unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(
        !viewer.versions(&doc).await.unwrap().iter().any(|v| v.id == v3.id),
        "v3 cannot reach the viewer while its only holder is offline"
    );
    assert!(!owner.versions(&doc).await.unwrap().iter().any(|v| v.id == v3.id));
    // The holder returns: now both get it.
    let t1 = Instant::now();
    editor.set_online(true).await.unwrap();
    for n in [&owner, &viewer] {
        wait_for("v3 arrives once its holder is back", Duration::from_secs(30), || async {
            n.versions(&doc).await.ok()?.iter().any(|v| v.id == v3.id).then_some(())
        })
        .await;
    }
    eprintln!("MEASURE holder_returns_then_all_converge_ms={}", t1.elapsed().as_millis());
}

// ------------------------------------------------------------------- tampering

/// An outsider that speaks the sync protocol and sends a header with one flipped bit. The node
/// must drop it (the signature no longer matches), refuse nothing else, and keep working.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tampered_operation_is_dropped_and_the_node_keeps_working() {
    init_logging();
    // ACL off here only so that an endpoint that is not a member can reach the session at all:
    // this test is about what happens to a bad operation, not about who may connect.
    let mut open = cfg();
    open.enforce_acl = false;
    let (owner, editor, _viewer, doc) = trio(open, cfg(), cfg()).await;

    let headers = owner.raw_op_headers(&doc).await.unwrap();
    assert!(headers.len() >= 6, "a document with a few operations: {}", headers.len());
    // A real operation's header with one bit flipped inside the entry.
    let mut tampered = headers.iter().max_by_key(|h| h.len()).unwrap().clone();
    let mid = tampered.len() / 2;
    tampered[mid] ^= 0x01;
    assert!(
        Header::<dcp_p2p::ext::DcpExt>::decode(&tampered).is_err(),
        "Header::decode must refuse a header whose bytes changed"
    );
    assert!(Header::<dcp_p2p::ext::DcpExt>::decode(&headers[0]).is_ok());

    let before = owner.op_counts(&doc).await.unwrap();
    let failures_before = owner.counters(&doc).await.unwrap().sync_failures;
    let _ = failures_before;

    // The evil endpoint: a fresh key, dials the owner, says hello, then sends the bad operation.
    let ep = iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
        .relay_mode(iroh::RelayMode::Disabled)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")
        .unwrap()
        .bind()
        .await
        .unwrap();
    let conn = ep.connect(owner.addr(), SYNC_ALPN).await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    write_hello(&mut send, &Hello { doc: doc.clone(), barrier: None }).await.unwrap();
    assert_eq!(read_status(&mut recv).await.unwrap(), STATUS_OK);
    let mut sink = FramedWrite::new(send, dcp_p2p::codec::Codec::<LogSyncMessage<String>>::new());
    let mut stream = FramedRead::new(recv, dcp_p2p::codec::Codec::<LogSyncMessage<String>>::new());
    use futures_util::{SinkExt, StreamExt};
    // The protocol: both sides send what they have, then what they will send, then operations.
    sink.send(LogSyncMessage::Have(Default::default())).await.unwrap();
    let _their_have = stream.next().await;
    sink.send(LogSyncMessage::PreSync { total_operations: 1, total_bytes: tampered.len() as u32 })
        .await
        .unwrap();
    sink.send(LogSyncMessage::Operation((tampered.clone(), None))).await.unwrap();
    sink.send(LogSyncMessage::Done).await.unwrap();
    // The owner's session ends with an error; drain whatever it still sent.
    while let Ok(Some(_)) = tokio::time::timeout(Duration::from_secs(3), stream.next()).await {}
    tokio::time::sleep(Duration::from_millis(500)).await;

    let after = owner.op_counts(&doc).await.unwrap();
    assert_eq!(before, after, "the tampered operation changed nothing: before {before:?} after {after:?}");
    let c = owner.counters(&doc).await.unwrap();
    eprintln!("MEASURE tampered_session: sync_failures {} -> {}", failures_before, c.sync_failures);
    assert!(c.sync_failures > failures_before, "the session that carried it must have failed");

    // The node keeps working: a normal publish still reaches the editor.
    let first = first_version(&owner, &doc).await;
    let v = owner.publish_version(&doc, vec![first], new_file(b"after the attack"), None).await.unwrap();
    wait_for("the editor still gets new versions", Duration::from_secs(30), || async {
        editor.versions(&doc).await.ok()?.iter().any(|x| x.id == v.id).then_some(())
    })
    .await;
}

// ------------------------------------------------------------------- revocation

fn acl_off() -> dcp_p2p::P2pConfig {
    let mut c = cfg();
    c.enforce_acl = false;
    c
}

/// The membership gate is off, so the removed member's node is served every operation. It still
/// cannot read what was published after its removal, and the blob it fetches is sealed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_revoked_member_receives_ciphertext_but_cannot_decrypt_later_entries() {
    init_logging();
    let (owner, editor, viewer, doc) = trio(acl_off(), acl_off(), acl_off()).await;
    let first = first_version(&owner, &doc).await;
    let pre = viewer.entry_count(&doc).await.unwrap();
    owner.revoke(&doc, &viewer.principal()).await.unwrap();
    let secret = b"published after the viewer was removed";
    let hidden = editor.publish_version(&doc, vec![first], new_file(secret), None).await.unwrap();

    // Serve the viewer everything, explicitly.
    let ops_before = viewer.op_counts(&doc).await.unwrap();
    let mut got = 0;
    for _ in 0..5 {
        let s = viewer.sync_now(&doc, &editor.principal()).await.unwrap();
        got += s.received_operations;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let ops_after = viewer.op_counts(&doc).await.unwrap();
    eprintln!("MEASURE revoked viewer: operations received {got}, applied/pending/rejected {ops_before:?} -> {ops_after:?}");
    assert!(got > 0, "the ciphertext did reach the removed member's node (the gate is off)");
    // The viewer processes its own removal and the later entries do not decrypt.
    assert_eq!(viewer.entry_count(&doc).await.unwrap(), pre, "no new entry was decrypted");
    assert!(viewer.fetch_file(&doc, &hidden).await.is_err());

    // The file: fetched by hash alone it is sealed ciphertext, not the plaintext.
    let blob = blob_of_locator(&hidden.body.file.locator).unwrap();
    let sealed = viewer.fetch_raw_blob(&editor.principal(), &blob).await.unwrap();
    assert_eq!(sealed.len(), secret.len() + 24 + 16, "nonce + ciphertext + tag");
    assert!(!sealed.windows(secret.len()).any(|w| w == secret), "the plaintext is not in the blob");
    // The blob hash is BLAKE3 of the sealed bytes (iroh-blobs); the version's sha256 is of the plaintext.
    assert_eq!(iroh_blobs::Hash::new(&sealed).to_hex(), blob);
    // The editor (still a member) reads it.
    let v = editor.versions(&doc).await.unwrap().into_iter().find(|v| v.id == hidden.id).unwrap();
    assert_eq!(editor.fetch_file(&doc, &v).await.unwrap(), secret);
}

/// With the gates on, the same removed member cannot even get the blob by hash.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_blob_gate_refuses_a_removed_member_and_a_non_member() {
    init_logging();
    let (owner, editor, viewer, doc) = default_trio().await;
    let first = first_version(&owner, &doc).await;
    owner.revoke(&doc, &viewer.principal()).await.unwrap();
    let hidden = editor.publish_version(&doc, vec![first.clone()], new_file(b"gated"), None).await.unwrap();
    let blob = blob_of_locator(&hidden.body.file.locator).unwrap();

    let r = viewer.fetch_raw_blob(&editor.principal(), &blob).await;
    eprintln!("MEASURE removed member fetching by hash with the gate on: {r:?}");
    assert!(r.is_err(), "the gate must refuse a removed member");
    // A member can.
    let ok = owner.fetch_raw_blob(&editor.principal(), &blob).await;
    assert!(ok.is_ok(), "{ok:?}");
    // An outsider that was never a member.
    let stranger = node(cfg()).await;
    stranger.add_peer(editor.addr()).await.unwrap();
    let r = stranger.fetch_raw_blob(&editor.principal(), &blob).await;
    assert!(r.is_err(), "{r:?}");
    // The sync gate: a removed member's session is refused.
    let denied = viewer.sync_now(&doc, &editor.principal()).await;
    assert!(denied.is_err(), "{denied:?}");
}

/// Without the membership gate a removed editor can still write, but readers drop what it writes:
/// its entry claims a causal point from before the removal, and a removal that the claim does not
/// include invalidates it (p2panda-auth's "strong removal", applied to entries).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_removed_editor_cannot_publish_with_a_stale_view() {
    init_logging();
    let (owner, editor, viewer, doc) = trio(acl_off(), acl_off(), acl_off()).await;
    let first = first_version(&owner, &doc).await;
    let entries_before = owner.entry_count(&doc).await.unwrap();
    // The editor goes away just before it is removed, so it never learns of the removal.
    editor.set_online(false).await.unwrap();
    owner.revoke(&doc, &editor.principal()).await.unwrap();
    // Offline, the editor still believes it is an editor and publishes.
    let stale = editor.publish_version(&doc, vec![first.clone()], new_file(b"written with a stale view"), None).await.unwrap();
    editor.set_online(true).await.unwrap();
    // Everyone is served everyone's operations; the entry reaches the owner and the viewer.
    for n in [&owner, &viewer] {
        let _ = n.sync_now(&doc, &editor.principal()).await;
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    for n in [&owner, &viewer] {
        let listed = n.versions(&doc).await.map(|v| v.iter().any(|x| x.id == stale.id)).unwrap_or(false);
        assert!(!listed, "{} lists a version written by a removed editor", n.principal());
    }
    // The owner DID decrypt the entry (it still holds the old group key) and then refused it: the
    // refusal is the reader's authorisation rule, not a failure to decrypt.
    let entries_after = owner.entry_count(&doc).await.unwrap();
    let counters = owner.counters(&doc).await.unwrap();
    eprintln!("MEASURE removed editor's entry: owner entries {entries_before} -> {entries_after}, listed: no; owner counters {counters:?}");
    assert!(entries_after > entries_before, "the entry must have been decrypted and refused, not lost");
}

/// The honest limit of revocation without a server: the rotated group key reaches a member only
/// when it applies the removal. A member that has not seen the removal still encrypts to the old
/// key, still serves the removed member, and the removed member reads what it published.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn content_from_a_member_that_has_not_seen_the_removal_is_readable_by_the_removed_member() {
    init_logging();
    let (owner, editor, viewer, doc) = default_trio().await;
    let first = first_version(&owner, &doc).await;
    editor.set_online(false).await.unwrap();
    let t = Instant::now();
    owner.revoke(&doc, &viewer.principal()).await.unwrap();
    eprintln!("MEASURE revoke with the only other member unreachable returned after {} ms", t.elapsed().as_millis());
    // The editor does not know. It publishes to the old group key.
    let secret = b"published by a member who had not seen the removal";
    let stale = editor.publish_version(&doc, vec![first], new_file(secret), None).await.unwrap();
    // The owner, the only one who knows, goes away before the editor returns.
    owner.set_online(false).await.unwrap();
    editor.set_online(true).await.unwrap();
    let seen = wait_soft(Duration::from_secs(30), || async {
        viewer.versions(&doc).await.ok()?.iter().any(|v| v.id == stale.id).then_some(())
    })
    .await;
    if seen.is_none() {
        eprintln!("DIAG viewer role {:?}", viewer.role(&doc).await);
        eprintln!("DIAG viewer members {:?}", viewer.members(&doc).await);
        eprintln!("DIAG viewer versions {:?}", viewer.versions(&doc).await.map(|v| v.len()));
        eprintln!("DIAG viewer ops (applied, pending, rejected) {:?}", viewer.op_counts(&doc).await);
        eprintln!("DIAG viewer entries {:?}", viewer.entry_count(&doc).await);
        eprintln!("DIAG viewer counters {:?}", viewer.counters(&doc).await);
        eprintln!("DIAG editor ops {:?} entries {:?} counters {:?}", editor.op_counts(&doc).await, editor.entry_count(&doc).await, editor.counters(&doc).await);
        eprintln!("DIAG editor members {:?}", editor.members(&doc).await);
        panic!("the removed viewer never listed the new version");
    }
    let v = viewer.versions(&doc).await.unwrap().into_iter().find(|v| v.id == stale.id).unwrap();
    assert_eq!(viewer.fetch_file(&doc, &v).await.unwrap(), secret);
    eprintln!("MEASURE removed member read a version published after its removal by a member that had not applied it: yes");
}

// ------------------------------------------------------------------- the contact card

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn granting_a_role_needs_the_grantees_contact_card_first() {
    init_logging();
    let owner = node(cfg()).await;
    let guest = node(cfg()).await;
    mesh(&[&owner, &guest]).await;
    let doc = owner
        .create_document(&dcp_conformance::standard_meta(), Some(new_file(b"x")))
        .await
        .unwrap();
    let r = owner.grant(&doc, &guest.principal(), Role::Editor).await;
    eprintln!("MEASURE grant without a card: {r:?}");
    assert!(r.is_err(), "adding a member needs its key bundle");
    invite(&owner, &guest, &doc).await;
    owner.grant(&doc, &guest.principal(), Role::Editor).await.unwrap();
    wait_for("the guest is an editor", Duration::from_secs(30), || async {
        (guest.role(&doc).await.ok()? == Role::Editor).then_some(())
    })
    .await;
    let r = owner.grant(&doc, &guest.principal(), Role::Commenter).await;
    assert!(matches!(r, Err(StoreError::Invalid(_))), "commenter is not expressible: {r:?}");
}

// ------------------------------------------------------------------- hashes

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_file_is_addressed_by_blake3_and_verified_by_sha256() {
    init_logging();
    let (owner, _editor, _viewer, doc) = default_trio().await;
    let v = owner.versions(&doc).await.unwrap().remove(0);
    let blob = blob_of_locator(&v.body.file.locator).expect("a blake3 locator");
    let sealed = owner.raw_blob(&blob).await.unwrap().expect("the blob is stored");
    assert_eq!(iroh_blobs::Hash::new(&sealed).to_hex(), blob, "the iroh-blobs hash is BLAKE3 of the sealed bytes");
    let plain = owner.fetch_file(&doc, &v).await.unwrap();
    dcp_core::integrity::verify(&v.body.file, &plain).expect("the spec's sha256 and size match the plaintext");
    assert_ne!(dcp_core::integrity::sha256_hex(&sealed), v.body.file.sha256, "sha256 is of the plaintext, not the sealed blob");
    let _ = Hash::digest(b"");
}

// ------------------------------------------------------------------- clock skew and leases

/// Lease stamps are first-seen times on each reader's own clock, so two devices whose clocks are
/// a minute apart still agree on who holds the lease and when it expires.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn leases_work_with_clocks_a_minute_apart() {
    init_logging();
    let factory = P2pFactory::skewed(60_000, -60_000);
    for j in ["lease_exclusion_and_readback", "lease_expiry_releases"] {
        let r = run_one(&factory, j).await;
        eprintln!("MEASURE skewed clocks (owner +60 s, editor -60 s): {j}: {:?}", r.outcome);
        assert_eq!(r.outcome, Outcome::Passed, "{j}");
    }
}

/// Negative control: stamping a lease with its author's clock, the same skew breaks expiry.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn author_stamped_leases_break_under_clock_skew() {
    init_logging();
    let mut factory = P2pFactory::skewed(0, 60_000).with_all(|c| c.author_stamped_leases = true);
    factory.label = "-author-stamped-leases";
    let r = run_one(&factory, "lease_expiry_releases").await;
    eprintln!("MEASURE author-stamped leases, editor +60 s: {:?}", r.outcome);
    assert!(matches!(r.outcome, Outcome::Failed(_)), "{:?}", r.outcome);
}

// ------------------------------------------------------------------- inline updates

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_inline_update_of_the_maximum_size_goes_through() {
    init_logging();
    let (owner, editor, _viewer, doc) = default_trio().await;
    let first = first_version(&owner, &doc).await;
    for size in [1_000usize, 20_000, dcp_core::updates::MAX_INLINE_BYTES as usize] {
        let payload = vec![b'u'; size];
        let u = dcp_core::Update {
            dcp: 1,
            algorithm: dcp_conformance::ALGORITHM.into(),
            doc_model: dcp_conformance::DOC_MODEL.into(),
            base_version: first.clone(),
            seq: 0,
            payload: dcp_core::updates::inline_payload(&payload).unwrap(),
        };
        let t = Instant::now();
        let sent = editor.publish_update(&doc, u).await.unwrap();
        let publish_ms = t.elapsed().as_millis();
        let t = Instant::now();
        wait_for("the owner has the update", Duration::from_secs(60), || async {
            owner.updates(&doc).await.ok()?.iter().any(|x| x.id == sent.id).then_some(())
        })
        .await;
        eprintln!("MEASURE inline update {size} bytes: publish {publish_ms} ms, at the owner {} ms later", t.elapsed().as_millis());
    }
}

// ------------------------------------------------------------------- leases across a partition

struct Sleep;

#[async_trait]
impl Pause for Sleep {
    async fn pause(&self, ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

/// With no coordinator a lease cannot be exclusive across a partition: both sides acquire, both
/// read back their own record twice, both believe they hold it. When the partition heals the
/// deterministic winner rule leaves exactly one holder and the other learns it at its next read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partitioned_sessions_both_acquire_the_lease_and_the_merge_picks_one() {
    init_logging();
    let (owner, editor, _viewer, doc) = default_trio().await;
    let first = first_version(&owner, &doc).await;
    let (co, ce) = (
        Claim::new(owner.principal(), "session-owner", "token-owner", &first, 60_000),
        Claim::new(editor.principal(), "session-editor", "token-editor", &first, 60_000),
    );
    owner.set_online(false).await.unwrap();
    editor.set_online(false).await.unwrap();
    let st = Settle::new(&Sleep, 500);
    let ro = lease::acquire(&*owner, &doc, &co, &st).await.unwrap();
    let re = lease::acquire(&*editor, &doc, &ce, &st).await.unwrap();
    assert!(matches!(ro, Acquire::Acquired(_)), "{ro:?}");
    assert!(matches!(re, Acquire::Acquired(_)), "both sides, partitioned, believe they hold the lease: {re:?}");
    assert!(lease::holds(&*owner, &doc, &co.holder).await.unwrap());
    assert!(lease::holds(&*editor, &doc, &ce.holder).await.unwrap());

    let t0 = Instant::now();
    owner.set_online(true).await.unwrap();
    editor.set_online(true).await.unwrap();
    wait_for("exactly one holder after the merge", Duration::from_secs(30), || async {
        let a = lease::holds(&*owner, &doc, &co.holder).await.ok()?;
        let b = lease::holds(&*editor, &doc, &ce.holder).await.ok()?;
        (a != b).then_some(())
    })
    .await;
    eprintln!("MEASURE partitioned lease: both acquired while apart; exactly one holder {} ms after the partition healed", t0.elapsed().as_millis());
    let lo = owner.lease(&doc).await.unwrap().unwrap();
    let le = editor.lease(&doc).await.unwrap().unwrap();
    assert_eq!(lo.id, le.id, "both nodes name the same winning record");
}
