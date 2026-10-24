//! `WopiClientStore` (a DocumentStore that speaks only WOPI HTTP) against a
//! `WopiHost` over the in-test backing store.

mod support;

use dcp_core::*;
use dcp_wopi::{WopiClientStore, CAPABILITIES};
use support::*;

fn sim(rig: &Rig, who: &str) -> WopiClientStore {
    let s = WopiClientStore::new(rig.server.base_url(), who, format!("wopi-sim-{who}"));
    s.attach(rig.doc.clone(), rig.token(who));
    s
}

fn new_file(text: &str) -> NewFile {
    NewFile { mime: ODT_MIME.into(), bytes: make_odt(text) }
}

#[tokio::test]
async fn publish_through_wopi_makes_a_child_of_the_head() {
    let rig = Rig::new().await;
    let s = sim(&rig, "@alice:test");

    let before = s.versions(&rig.doc).await.unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].id, rig.v1);
    assert_eq!(before[0].body.parents, Vec::<String>::new());
    assert_eq!(before[0].sender, "@alice:test");
    assert_eq!(s.fetch_file(&rig.doc, &before[0]).await.unwrap(), make_odt("base"));

    let v2 = s
        .publish_version(&rig.doc, vec![rig.v1.clone()], new_file("v2"), Some("dropped".into()))
        .await
        .unwrap();
    assert_eq!(v2.body.parents, vec![rig.v1.clone()]);
    assert_eq!(v2.body.created_by.principal, "@alice:test");

    // The backing store holds the same version, with the same bytes.
    let backing = rig.owner.versions(&rig.doc).await.unwrap();
    assert_eq!(backing.len(), 2);
    let b2 = backing.iter().find(|v| v.id == v2.id).unwrap();
    assert_eq!(b2.body, v2.body);
    assert_eq!(rig.owner.fetch_file(&rig.doc, b2).await.unwrap(), make_odt("v2"));

    // The simulator unlocked: no active lease remains.
    let lease = s.lease(&rig.doc).await.unwrap().unwrap();
    assert_eq!(lease.body.state, LeaseState::Released);

    // The whole graph is listed (host extension) and every version is readable.
    let all = s.versions(&rig.doc).await.unwrap();
    assert_eq!(all.len(), 2);
    let first = all.iter().find(|v| v.id == rig.v1).unwrap();
    assert_eq!(s.fetch_file(&rig.doc, first).await.unwrap(), make_odt("base"));
    assert_eq!(s.fetch_file(&rig.doc, &v2).await.unwrap(), make_odt("v2"));
    let v3 = s.publish_version(&rig.doc, vec![v2.id.clone()], new_file("v3"), None).await.unwrap();
    assert_eq!(v3.body.parents, vec![v2.id.clone()]);
}

#[tokio::test]
async fn the_first_version_of_an_empty_document() {
    let backend = Backend::new();
    let owner = backend.handle("@alice:test");
    let doc = owner.create_document(&meta("Fresh"), None).await.unwrap();
    let host = dcp_wopi::WopiHost::default();
    let server = host.serve("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let s = WopiClientStore::new(server.base_url(), "@alice:test", "sim");
    s.attach(doc.clone(), host.register(doc.clone(), owner.clone(), "@alice:test"));

    assert!(s.versions(&doc).await.unwrap().is_empty());
    let v1 = s.publish_version(&doc, vec![], new_file("one"), None).await.unwrap();
    assert!(v1.body.parents.is_empty());
    assert_eq!(owner.versions(&doc).await.unwrap().len(), 1);
    // parents [] on a document that has versions is not expressible.
    let err = s.publish_version(&doc, vec![], new_file("two"), None).await.unwrap_err();
    assert!(matches!(err, StoreError::Invalid(_)), "{err}");
}

#[tokio::test]
async fn a_held_lock_blocks_the_simulator_and_nothing_is_published() {
    let rig = Rig::new().await;
    rig.owner.grant(&rig.doc, "@bob:test", Role::Editor).await.unwrap();
    let alice = sim(&rig, "@alice:test");
    let bob = sim(&rig, "@bob:test");

    // Alice's editor locks the file (a lease via WOPI, spec 4.3).
    alice
        .write_lease(
            &rig.doc,
            Lease {
                dcp: 1,
                state: LeaseState::Held,
                holder: None,
                token: Some("alice-lock".into()),
                base_version: None,
                renewal: Some(0),
                duration_ms: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(bob.lease(&rig.doc).await.unwrap().unwrap().body.token.as_deref(), Some("alice-lock"));

    // Bob cannot take it, and his save is refused: spec test 4.
    let err = bob
        .write_lease(
            &rig.doc,
            Lease { dcp: 1, state: LeaseState::Held, holder: None, token: Some("bob-lock".into()), base_version: None, renewal: Some(0), duration_ms: None },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, StoreError::Conflict(ref m) if m.contains("alice-lock")), "{err}");
    let err = bob.publish_version(&rig.doc, vec![rig.v1.clone()], new_file("bob"), None).await.unwrap_err();
    assert!(matches!(err, StoreError::Conflict(_)), "{err}");
    assert_eq!(rig.owner.versions(&rig.doc).await.unwrap().len(), 1);

    // Release by anyone who knows the string (WOPI locks are not user-owned),
    // via the Released lease.
    bob.write_lease(&rig.doc, Lease { dcp: 1, state: LeaseState::Released, holder: None, token: None, base_version: None, renewal: None, duration_ms: None })
        .await
        .unwrap();
    bob.publish_version(&rig.doc, vec![rig.v1.clone()], new_file("bob"), None).await.unwrap();
}

#[tokio::test]
async fn lease_expiry_by_the_binding_clock() {
    let rig = Rig::new().await;
    let alice = sim(&rig, "@alice:test");
    alice
        .write_lease(&rig.doc, Lease { dcp: 1, state: LeaseState::Held, holder: None, token: Some("t".into()), base_version: None, renewal: Some(0), duration_ms: None })
        .await
        .unwrap();
    assert_eq!(alice.lease(&rig.doc).await.unwrap().unwrap().body.state, LeaseState::Held);
    let t0 = alice.now_ms(&rig.doc).await.unwrap();
    rig.backend.advance(31 * 60 * 1000);
    assert!(alice.now_ms(&rig.doc).await.unwrap() >= t0 + 31 * 60 * 1000);
    assert_eq!(alice.lease(&rig.doc).await.unwrap().unwrap().body.state, LeaseState::Released);
}

#[tokio::test]
async fn roles_come_from_check_file_info_and_a_viewer_cannot_publish() {
    let rig = Rig::new().await;
    rig.owner.grant(&rig.doc, "@carol:test", Role::Viewer).await.unwrap();
    rig.owner.grant(&rig.doc, "@dave:test", Role::Commenter).await.unwrap();
    rig.owner.grant(&rig.doc, "@bob:test", Role::Editor).await.unwrap();
    assert_eq!(sim(&rig, "@alice:test").role(&rig.doc).await.unwrap(), Role::Owner);
    assert_eq!(sim(&rig, "@bob:test").role(&rig.doc).await.unwrap(), Role::Editor);
    assert_eq!(sim(&rig, "@dave:test").role(&rig.doc).await.unwrap(), Role::Commenter);
    let carol = sim(&rig, "@carol:test");
    assert_eq!(carol.role(&rig.doc).await.unwrap(), Role::Viewer);

    let v = carol.versions(&rig.doc).await.unwrap();
    assert_eq!(carol.fetch_file(&rig.doc, &v[0]).await.unwrap(), make_odt("base"));
    let err = carol.publish_version(&rig.doc, vec![rig.v1.clone()], new_file("nope"), None).await.unwrap_err();
    assert!(matches!(err, StoreError::Forbidden(_)), "{err}");
    assert_eq!(rig.owner.versions(&rig.doc).await.unwrap().len(), 1);

    let m = sim(&rig, "@alice:test").meta(&rig.doc).await.unwrap();
    assert_eq!(m.title, "Budget");
    assert_eq!(m.mime, ODT_MIME);
}

#[tokio::test]
async fn what_wopi_cannot_express_is_refused_loudly() {
    let rig = Rig::new().await;
    let s = sim(&rig, "@alice:test");
    let invalid = |e: StoreError| matches!(e, StoreError::Invalid(ref m) if m.contains("unsupported over WOPI"));

    assert!(invalid(s.create_document(&meta("x"), None).await.unwrap_err()));
    assert!(invalid(s.grant(&rig.doc, "@z:test", Role::Viewer).await.unwrap_err()));
    assert!(invalid(s.revoke(&rig.doc, "@z:test").await.unwrap_err()));
    assert!(invalid(s.updates(&rig.doc).await.unwrap_err()));
    // Merge: two parents.
    let err = s
        .publish_version(&rig.doc, vec![rig.v1.clone(), "other".into()], new_file("m"), None)
        .await
        .unwrap_err();
    assert!(invalid(err));
    // A parent that is not a retained version.
    let err = s.publish_version(&rig.doc, vec!["nonesuch".into()], new_file("o"), None).await.unwrap_err();
    assert!(matches!(err, StoreError::NotFound(_)), "{err}");
    assert_eq!(rig.owner.versions(&rig.doc).await.unwrap().len(), 1);
    // A first version of a document that has one.
    let err = s.publish_version(&rig.doc, vec![], new_file("o"), None).await.unwrap_err();
    assert!(invalid(err));

    // The capability flags say the same thing.
    let c = s.capabilities();
    assert_eq!(c, CAPABILITIES);
    assert!(!c.create_document && !c.grant_revoke && !c.merge_parents && !c.lease_fields && !c.updates);
    assert!(!c.version_messages);
}

#[tokio::test]
async fn saving_a_child_of_an_older_version_forks() {
    let rig = Rig::new().await;
    rig.owner.grant(&rig.doc, "@bob:test", Role::Editor).await.unwrap();
    let alice = sim(&rig, "@alice:test");

    // Alice's editor has v1 open ...
    // ... Bob publishes natively (DCP, not WOPI) ...
    let bob = rig.backend.handle("@bob:test");
    let vb = bob
        .publish_version(&rig.doc, vec![rig.v1.clone()], new_file("bob"), None)
        .await
        .unwrap();
    // ... and Alice saves what she opened: Collabora's forced save. A second
    // head appears; nothing is overwritten (spec version rules 4 and 5).
    let va = alice.publish_version(&rig.doc, vec![rig.v1.clone()], new_file("alice"), None).await.unwrap();
    assert_eq!(va.body.parents, vec![rig.v1.clone()]);
    let all = rig.owner.versions(&rig.doc).await.unwrap();
    assert_eq!(all.len(), 3);
    assert!(all.iter().any(|v| v.id == vb.id));
    let heads: Vec<_> = all
        .iter()
        .filter(|v| !all.iter().any(|o| o.body.parents.contains(&v.id)))
        .map(|v| v.id.clone())
        .collect();
    assert_eq!(heads.len(), 2, "{heads:?}");
}

#[tokio::test]
async fn a_truncated_upload_is_never_published() {
    let rig = Rig::new().await;
    let s = sim(&rig, "@alice:test");
    let whole = make_odt("truncate me please, long enough");
    let cut = NewFile { mime: ODT_MIME.into(), bytes: whole[..whole.len() / 2].to_vec() };
    let err = s.publish_version(&rig.doc, vec![rig.v1.clone()], cut, None).await.unwrap_err();
    assert!(matches!(err, StoreError::Invalid(_)), "{err}");
    assert_eq!(rig.owner.versions(&rig.doc).await.unwrap().len(), 1);
    // The failed attempt did not leave the lock behind.
    assert_eq!(s.lease(&rig.doc).await.unwrap().unwrap().body.state, LeaseState::Released);
}

#[tokio::test]
async fn a_corrupt_head_is_not_served_by_the_host() {
    let rig = Rig::new().await;
    let s = sim(&rig, "@alice:test");
    let v = s.versions(&rig.doc).await.unwrap().remove(0);
    rig.backend.corrupt(&rig.doc, &rig.v1);
    let err = s.fetch_file(&rig.doc, &v).await.unwrap_err();
    assert!(matches!(err, StoreError::Transport(ref m) if m.contains("integrity")), "{err}");
}
