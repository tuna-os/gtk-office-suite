//! WOPI wire semantics of `WopiHost`, checked with raw HTTP against the
//! behaviour in Microsoft's public WOPI REST docs and what Collabora's
//! `WopiStorage.cpp` expects of a host.

mod support;

use dcp_core::*;
use reqwest::StatusCode;
use serde_json::Value;
use support::*;

async fn post_op(
    rig: &Rig,
    token: &str,
    op: &str,
    headers: &[(&str, &str)],
) -> reqwest::Response {
    let mut r = http()
        .post(rig.url(false))
        .query(&[("access_token", token)])
        .header("X-WOPI-Override", op)
        .header("content-length", "0");
    for (k, v) in headers {
        r = r.header(*k, *v);
    }
    r.send().await.unwrap()
}

async fn lock_of(r: &reqwest::Response) -> Option<String> {
    r.headers().get("x-wopi-lock").map(|v| v.to_str().unwrap().to_string())
}

async fn put(
    rig: &Rig,
    token: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> reqwest::Response {
    let mut r = http()
        .post(rig.url(true))
        .query(&[("access_token", token)])
        .header("X-WOPI-Override", "PUT")
        .body(body);
    for (k, v) in headers {
        r = r.header(*k, *v);
    }
    r.send().await.unwrap()
}

async fn get_file(rig: &Rig, token: &str) -> reqwest::Response {
    http().get(rig.url(true)).query(&[("access_token", token)]).send().await.unwrap()
}

async fn cfi(rig: &Rig, token: &str) -> Value {
    let r = http().get(rig.url(false)).query(&[("access_token", token)]).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    r.json::<Value>().await.unwrap()
}

async fn version_count(rig: &Rig) -> usize {
    rig.owner.versions(&rig.doc).await.unwrap().len()
}

async fn grant(rig: &Rig, who: &str, role: Role) {
    rig.owner.grant(&rig.doc, who, role).await.unwrap();
}

#[tokio::test]
async fn check_file_info_reports_the_head_and_the_role() {
    let rig = Rig::new().await;
    grant(&rig, "@bob:test", Role::Editor).await;
    grant(&rig, "@carol:test", Role::Viewer).await;
    grant(&rig, "@dave:test", Role::Commenter).await;

    let owner = cfi(&rig, &rig.token("@alice:test")).await;
    assert_eq!(owner["BaseFileName"], "Budget.odt");
    assert_eq!(owner["OwnerId"], "@alice:test");
    assert_eq!(owner["UserId"], "@alice:test");
    assert_eq!(owner["Version"], rig.v1);
    assert_eq!(owner["Size"], make_odt("base").len());
    assert_eq!(owner["UserCanWrite"], true);
    assert_eq!(owner["SupportsLocks"], true);
    assert_eq!(owner["SupportsUpdate"], true);
    assert_eq!(owner["UserCanNotWriteRelative"], true);
    assert_eq!(owner["DcpRole"], "owner");
    let lm = owner["LastModifiedTime"].as_str().unwrap();
    assert!(dcp_wopi::timefmt::parse_iso8601_ms(lm).unwrap() > 1_700_000_000_000, "{lm}");

    assert_eq!(cfi(&rig, &rig.token("@bob:test")).await["UserCanWrite"], true);
    let viewer = cfi(&rig, &rig.token("@carol:test")).await;
    assert_eq!(viewer["UserCanWrite"], false);
    assert_eq!(viewer["UserCanOnlyComment"], false);
    let commenter = cfi(&rig, &rig.token("@dave:test")).await;
    assert_eq!(commenter["UserCanWrite"], false);
    assert_eq!(commenter["UserCanOnlyComment"], true);
}

#[tokio::test]
async fn tokens_are_checked() {
    let rig = Rig::new().await;
    let r = http().get(rig.url(false)).query(&[("access_token", "nope")]).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r = http().get(rig.url(false)).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

    // A token for one file does not open another.
    let other = rig
        .owner
        .create_document(&meta("Other"), Some(NewFile { mime: ODT_MIME.into(), bytes: make_odt("x") }))
        .await
        .unwrap();
    let tok = rig.host.register(other, rig.backend.handle("@alice:test"), "@alice:test");
    let r = http().get(rig.url(false)).query(&[("access_token", tok.as_str())]).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

    // A revoked token stops working.
    let t = rig.token("@alice:test");
    rig.host.revoke_token(&t);
    let r = http().get(rig.url(false)).query(&[("access_token", t.as_str())]).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn get_file_serves_the_head_and_refuses_a_corrupt_one() {
    let rig = Rig::new().await;
    let t = rig.token("@alice:test");
    let r = get_file(&rig, &t).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.headers()["x-wopi-itemversion"], rig.v1.as_str());
    assert_eq!(r.bytes().await.unwrap().to_vec(), make_odt("base"));

    // Spec version rule 2: the host re-hashes and does not serve a mismatch.
    rig.backend.corrupt(&rig.doc, &rig.v1);
    let r = get_file(&rig, &t).await;
    assert_eq!(r.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = r.text().await.unwrap();
    assert!(body.contains("integrity"), "{body}");
}

#[tokio::test]
async fn lock_family_returns_the_current_lock_on_mismatch() {
    let rig = Rig::new().await;
    grant(&rig, "@bob:test", Role::Editor).await;
    let a = rig.token("@alice:test");
    let b = rig.token("@bob:test");

    // GET_LOCK on an unlocked file: 200 and an empty X-WOPI-Lock (present).
    let r = post_op(&rig, &a, "GET_LOCK", &[]).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(lock_of(&r).await.as_deref(), Some(""));

    // UNLOCK, REFRESH_LOCK, UNLOCK_AND_RELOCK on an unlocked file: 409, empty lock.
    for (op, hs) in [
        ("UNLOCK", vec![("X-WOPI-Lock", "L1")]),
        ("REFRESH_LOCK", vec![("X-WOPI-Lock", "L1")]),
        ("LOCK", vec![("X-WOPI-Lock", "L2"), ("X-WOPI-OldLock", "L1")]),
    ] {
        let r = post_op(&rig, &a, op, &hs).await;
        assert_eq!(r.status(), StatusCode::CONFLICT, "{op}");
        assert_eq!(lock_of(&r).await.as_deref(), Some(""), "{op}");
    }

    // LOCK works and is recorded as a DCP lease (spec 4.3).
    let r = post_op(&rig, &a, "LOCK", &[("X-WOPI-Lock", "L1")]).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert!(r.headers().get("x-wopi-lock").is_none(), "not on 200");
    let lease = rig.owner.lease(&rig.doc).await.unwrap().unwrap();
    assert_eq!(lease.body.state, LeaseState::Held);
    assert_eq!(lease.body.token.as_deref(), Some("L1"));
    assert_eq!(lease.body.holder.as_ref().unwrap().principal, "@alice:test");
    assert_eq!(lease.body.base_version.as_deref(), Some(rig.v1.as_str()));
    assert_eq!(lease.body.duration_ms, Some(30 * 60 * 1000));

    // Another lock string: 409 carrying the current lock; the lease is untouched.
    let r = post_op(&rig, &b, "LOCK", &[("X-WOPI-Lock", "L-other")]).await;
    assert_eq!(r.status(), StatusCode::CONFLICT);
    assert_eq!(lock_of(&r).await.as_deref(), Some("L1"));
    assert_eq!(rig.owner.lease(&rig.doc).await.unwrap().unwrap().body.token.as_deref(), Some("L1"));

    // GET_LOCK returns it, for any token.
    let r = post_op(&rig, &b, "GET_LOCK", &[]).await;
    assert_eq!(lock_of(&r).await.as_deref(), Some("L1"));

    // Same string: a refresh (200), and the binding timestamp moves.
    let before = rig.owner.lease(&rig.doc).await.unwrap().unwrap().ts_ms;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let r = post_op(&rig, &b, "LOCK", &[("X-WOPI-Lock", "L1")]).await;
    assert_eq!(r.status(), StatusCode::OK);
    let after = rig.owner.lease(&rig.doc).await.unwrap().unwrap();
    assert!(after.ts_ms > before);
    // Locks are not user-owned: bob refreshing keeps alice as the holder.
    assert_eq!(after.body.holder.unwrap().principal, "@alice:test");

    // REFRESH_LOCK / UNLOCK with the wrong string: 409 + current lock.
    for op in ["REFRESH_LOCK", "UNLOCK"] {
        let r = post_op(&rig, &b, op, &[("X-WOPI-Lock", "wrong")]).await;
        assert_eq!(r.status(), StatusCode::CONFLICT, "{op}");
        assert_eq!(lock_of(&r).await.as_deref(), Some("L1"), "{op}");
    }
    assert_eq!(post_op(&rig, &b, "REFRESH_LOCK", &[("X-WOPI-Lock", "L1")]).await.status(), StatusCode::OK);

    // UNLOCK_AND_RELOCK: wrong old lock 409; right old lock swaps the string.
    let r = post_op(&rig, &a, "LOCK", &[("X-WOPI-Lock", "L2"), ("X-WOPI-OldLock", "nope")]).await;
    assert_eq!(r.status(), StatusCode::CONFLICT);
    assert_eq!(lock_of(&r).await.as_deref(), Some("L1"));
    let r = post_op(&rig, &a, "LOCK", &[("X-WOPI-Lock", "L2"), ("X-WOPI-OldLock", "L1")]).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(rig.owner.lease(&rig.doc).await.unwrap().unwrap().body.token.as_deref(), Some("L2"));

    // UNLOCK with the right string releases it.
    assert_eq!(post_op(&rig, &a, "UNLOCK", &[("X-WOPI-Lock", "L2")]).await.status(), StatusCode::OK);
    assert_eq!(rig.owner.lease(&rig.doc).await.unwrap().unwrap().body.state, LeaseState::Released);
    let r = post_op(&rig, &a, "GET_LOCK", &[]).await;
    assert_eq!(lock_of(&r).await.as_deref(), Some(""));
}

#[tokio::test]
async fn lock_input_is_validated_and_roles_gate_it() {
    let rig = Rig::new().await;
    grant(&rig, "@carol:test", Role::Viewer).await;
    let a = rig.token("@alice:test");
    let c = rig.token("@carol:test");

    // MS Lock: 400 when X-WOPI-Lock is missing or empty.
    assert_eq!(post_op(&rig, &a, "LOCK", &[]).await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(post_op(&rig, &a, "LOCK", &[("X-WOPI-Lock", "")]).await.status(), StatusCode::BAD_REQUEST);
    // Spec 4.3: token at most 256 characters.
    let long = "x".repeat(dcp_wopi::MAX_LOCK_LEN + 1);
    assert_eq!(post_op(&rig, &a, "LOCK", &[("X-WOPI-Lock", &long)]).await.status(), StatusCode::BAD_REQUEST);
    let max = "x".repeat(dcp_wopi::MAX_LOCK_LEN);
    assert_eq!(post_op(&rig, &a, "LOCK", &[("X-WOPI-Lock", &max)]).await.status(), StatusCode::OK);
    assert_eq!(post_op(&rig, &a, "UNLOCK", &[("X-WOPI-Lock", &max)]).await.status(), StatusCode::OK);

    // A viewer may read the lock but not take it.
    assert_eq!(post_op(&rig, &c, "GET_LOCK", &[]).await.status(), StatusCode::OK);
    assert_eq!(post_op(&rig, &c, "LOCK", &[("X-WOPI-Lock", "v")]).await.status(), StatusCode::FORBIDDEN);
    assert!(rig.owner.lease(&rig.doc).await.unwrap().is_none_or(|l| l.body.state == LeaseState::Released));

    // Operations the host does not do.
    for op in ["PUT_RELATIVE", "RENAME_FILE", "DELETE", "PUT_USER_INFO"] {
        assert_eq!(post_op(&rig, &a, op, &[]).await.status(), StatusCode::NOT_IMPLEMENTED, "{op}");
    }
}

#[tokio::test]
async fn an_expired_lease_is_an_unlocked_file() {
    let rig = Rig::new().await;
    let a = rig.token("@alice:test");
    assert_eq!(post_op(&rig, &a, "LOCK", &[("X-WOPI-Lock", "L1")]).await.status(), StatusCode::OK);
    let r = post_op(&rig, &a, "GET_LOCK", &[]).await;
    assert_eq!(lock_of(&r).await.as_deref(), Some("L1"));

    // 30 minutes on the binding's clock (spec 4.3 rule 2): the lock is gone and
    // somebody else can take it. Expiry is by the binding's clock, not ours.
    rig.backend.advance(30 * 60 * 1000 + 1);
    let r = post_op(&rig, &a, "GET_LOCK", &[]).await;
    assert_eq!(lock_of(&r).await.as_deref(), Some(""));
    let r = post_op(&rig, &a, "UNLOCK", &[("X-WOPI-Lock", "L1")]).await;
    assert_eq!(r.status(), StatusCode::CONFLICT);
    assert_eq!(lock_of(&r).await.as_deref(), Some(""));
    assert_eq!(post_op(&rig, &a, "LOCK", &[("X-WOPI-Lock", "L9")]).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn concurrent_locks_have_one_winner() {
    let rig = Rig::new().await;
    grant(&rig, "@bob:test", Role::Editor).await;
    let toks = [rig.token("@alice:test"), rig.token("@bob:test")];
    let mut set = tokio::task::JoinSet::new();
    for i in 0..16usize {
        let url = rig.url(false);
        let tok = toks[i % 2].clone();
        set.spawn(async move {
            http()
                .post(url)
                .query(&[("access_token", tok.as_str())])
                .header("X-WOPI-Override", "LOCK")
                .header("X-WOPI-Lock", format!("lock-{i}"))
                .header("content-length", "0")
                .send()
                .await
                .unwrap()
                .status()
        });
    }
    let mut ok = 0;
    while let Some(s) = set.join_next().await {
        if s.unwrap() == StatusCode::OK {
            ok += 1;
        }
    }
    assert_eq!(ok, 1, "exactly one lock string wins");
}

#[tokio::test]
async fn put_file_with_the_lock_publishes_a_child_of_what_the_session_read() {
    let rig = Rig::new().await;
    let a = rig.token("@alice:test");
    // Collabora's order: CheckFileInfo, GetFile, Lock, PutFile, Unlock.
    let info = cfi(&rig, &a).await;
    assert_eq!(get_file(&rig, &a).await.status(), StatusCode::OK);
    assert_eq!(post_op(&rig, &a, "LOCK", &[("X-WOPI-Lock", "L1")]).await.status(), StatusCode::OK);

    let edited = make_odt("base and typed");
    let r = put(
        &rig,
        &a,
        &[
            ("X-WOPI-Lock", "L1"),
            ("X-COOL-WOPI-Timestamp", info["LastModifiedTime"].as_str().unwrap()),
            ("X-COOL-WOPI-IsAutosave", "false"),
        ],
        edited.clone(),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let new_id = r.headers()["x-wopi-itemversion"].to_str().unwrap().to_string();
    assert!(r.headers().get("x-wopi-lock").is_none());
    let body: Value = r.json().await.unwrap();
    assert!(body["LastModifiedTime"].is_string());

    let versions = rig.owner.versions(&rig.doc).await.unwrap();
    assert_eq!(versions.len(), 2);
    let v2 = versions.iter().find(|v| v.id == new_id).unwrap();
    assert_eq!(v2.body.parents, vec![rig.v1.clone()]);
    assert_eq!(v2.body.created_by.principal, "@alice:test");
    assert_eq!(rig.owner.fetch_file(&rig.doc, v2).await.unwrap(), edited);

    // A second save by the same session chains on the first.
    let info2 = cfi(&rig, &a).await;
    assert_eq!(info2["Version"], new_id.as_str());
    let r = put(
        &rig,
        &a,
        &[
            ("X-WOPI-Lock", "L1"),
            ("X-COOL-WOPI-Timestamp", body["LastModifiedTime"].as_str().unwrap()),
        ],
        make_odt("again"),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let v3 = r.headers()["x-wopi-itemversion"].to_str().unwrap().to_string();
    let versions = rig.owner.versions(&rig.doc).await.unwrap();
    assert_eq!(versions.iter().find(|v| v.id == v3).unwrap().body.parents, vec![new_id]);
}

#[tokio::test]
async fn put_file_with_a_wrong_lock_publishes_nothing() {
    let rig = Rig::new().await;
    let a = rig.token("@alice:test");
    get_file(&rig, &a).await;
    post_op(&rig, &a, "LOCK", &[("X-WOPI-Lock", "L1")]).await;

    for hs in [vec![("X-WOPI-Lock", "WRONG")], vec![]] {
        let r = put(&rig, &a, &hs, make_odt("evil")).await;
        assert_eq!(r.status(), StatusCode::CONFLICT);
        assert_eq!(lock_of(&r).await.as_deref(), Some("L1"), "409 must carry the current lock");
    }
    assert_eq!(version_count(&rig).await, 1);

    // Unlocked file + a lock string: 409 with the empty lock (MS PutFile).
    post_op(&rig, &a, "UNLOCK", &[("X-WOPI-Lock", "L1")]).await;
    let r = put(&rig, &a, &[("X-WOPI-Lock", "L1")], make_odt("evil")).await;
    assert_eq!(r.status(), StatusCode::CONFLICT);
    assert_eq!(lock_of(&r).await.as_deref(), Some(""));
    assert_eq!(version_count(&rig).await, 1);
}

#[tokio::test]
async fn put_file_without_a_lock_needs_an_unchanged_head() {
    let rig = Rig::new().await;
    grant(&rig, "@bob:test", Role::Editor).await;
    let a = rig.token("@alice:test");

    // Never read the file: no base, so no save.
    let r = put(&rig, &a, &[], make_odt("blind")).await;
    assert_eq!(r.status(), StatusCode::CONFLICT);
    assert_eq!(lock_of(&r).await.as_deref(), Some(""));
    assert_eq!(version_count(&rig).await, 1);

    // Read, head unchanged: accepted.
    get_file(&rig, &a).await;
    let r = put(&rig, &a, &[], make_odt("ok")).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(version_count(&rig).await, 2);

    // Someone else publishes natively; alice's session is now stale: 409,
    // and nothing is published.
    let bob = rig.backend.handle("@bob:test");
    let head = rig.owner.versions(&rig.doc).await.unwrap().into_iter().max_by_key(|v| v.ts_ms).unwrap();
    bob.publish_version(
        &rig.doc,
        vec![head.id],
        NewFile { mime: ODT_MIME.into(), bytes: make_odt("bob") },
        None,
    )
    .await
    .unwrap();
    let r = put(&rig, &a, &[], make_odt("stale")).await;
    assert_eq!(r.status(), StatusCode::CONFLICT);
    assert_eq!(version_count(&rig).await, 3, "only bob's version was added");
}

#[tokio::test]
async fn collabora_timestamp_conflict_is_reported_as_1010() {
    let rig = Rig::new().await;
    grant(&rig, "@bob:test", Role::Editor).await;
    let a = rig.token("@alice:test");
    let info = cfi(&rig, &a).await;
    let stale_ts = info["LastModifiedTime"].as_str().unwrap().to_string();
    get_file(&rig, &a).await;
    post_op(&rig, &a, "LOCK", &[("X-WOPI-Lock", "L1")]).await;

    // Bob publishes natively while alice's editor holds the lease. A lease is
    // advisory (spec 4.3 rule 6), so this is accepted at the store.
    let bob = rig.backend.handle("@bob:test");
    bob.publish_version(
        &rig.doc,
        vec![rig.v1.clone()],
        NewFile { mime: ODT_MIME.into(), bytes: make_odt("bob") },
        None,
    )
    .await
    .unwrap();

    // Lock matches but the timestamp is stale: 409 + COOLStatusCode 1010,
    // *without* X-WOPI-Lock (Collabora reads a 409 with that header as a lock
    // mismatch, not as "document changed").
    let r = put(&rig, &a, &[("X-WOPI-Lock", "L1"), ("X-COOL-WOPI-Timestamp", &stale_ts)], make_odt("mine")).await;
    assert_eq!(r.status(), StatusCode::CONFLICT);
    assert!(r.headers().get("x-wopi-lock").is_none());
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["COOLStatusCode"], 1010);
    assert_eq!(version_count(&rig).await, 2, "nothing of alice's was published");

    // The legacy header name is honoured too.
    let r = put(&rig, &a, &[("X-WOPI-Lock", "L1"), ("X-LOOL-WOPI-Timestamp", &stale_ts)], make_odt("mine")).await;
    assert_eq!(r.status(), StatusCode::CONFLICT);

    // "savetostorage force=1" sends no timestamp: the save is accepted and,
    // because its parent is v1, the document now has two heads (spec rule 5).
    let r = put(&rig, &a, &[("X-WOPI-Lock", "L1")], make_odt("forced")).await;
    assert_eq!(r.status(), StatusCode::OK);
    let versions = rig.owner.versions(&rig.doc).await.unwrap();
    assert_eq!(versions.len(), 3);
    let forced = versions.iter().find(|v| v.id == r.headers()["x-wopi-itemversion"].to_str().unwrap()).unwrap();
    assert_eq!(forced.body.parents, vec![rig.v1.clone()]);

    // The conflict is visible, not hidden: read-only, and the heads are listed.
    let info = cfi(&rig, &rig.token("@bob:test")).await;
    assert_eq!(info["UserCanWrite"], false);
    assert_eq!(info["DcpHeads"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn put_file_refuses_a_package_that_does_not_open() {
    let rig = Rig::new().await;
    let a = rig.token("@alice:test");
    get_file(&rig, &a).await;
    post_op(&rig, &a, "LOCK", &[("X-WOPI-Lock", "L1")]).await;

    let whole = make_odt("big enough to truncate");
    for bad in [whole[..whole.len() / 2].to_vec(), b"not a zip at all".to_vec(), vec![]] {
        let r = put(&rig, &a, &[("X-WOPI-Lock", "L1")], bad).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }
    assert_eq!(version_count(&rig).await, 1, "a truncated upload is never published");
    let r = put(&rig, &a, &[("X-WOPI-Lock", "L1")], whole).await;
    assert_eq!(r.status(), StatusCode::OK);
}

#[tokio::test]
async fn roles_are_enforced_on_put_file() {
    let rig = Rig::new().await;
    grant(&rig, "@carol:test", Role::Viewer).await;
    let c = rig.token("@carol:test");
    get_file(&rig, &c).await;
    let r = put(&rig, &c, &[], make_odt("viewer write")).await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert_eq!(version_count(&rig).await, 1);

    // The host's own check is not the only one: a token whose store handle is a
    // viewer is refused by the store as well. Promote on the host side only by
    // handing out a viewer's store handle under an owner label.
    let t = rig.host.register(rig.doc.clone(), rig.backend.handle("@carol:test"), "@alice:test");
    let r = put(&rig, &t, &[], make_odt("spoof")).await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert_eq!(version_count(&rig).await, 1);
}

#[tokio::test]
async fn a_new_document_accepts_its_first_version_without_a_lock() {
    let backend = Backend::new();
    let owner = backend.handle("@alice:test");
    let doc = owner.create_document(&meta("Fresh"), None).await.unwrap();
    let host = dcp_wopi::WopiHost::default();
    let server = host.serve("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let tok = host.register(doc.clone(), owner.clone(), "@alice:test");
    let base = format!("{}/wopi/files/{doc}", server.base_url());

    let info: Value = http().get(&base).query(&[("access_token", tok.as_str())]).send().await.unwrap().json().await.unwrap();
    assert_eq!(info["Size"], 0);
    assert_eq!(info["Version"], "");
    let r = http().post(format!("{base}/contents")).query(&[("access_token", tok.as_str())])
        .header("X-WOPI-Override", "PUT").body(make_odt("first")).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let v = owner.versions(&doc).await.unwrap();
    assert_eq!(v.len(), 1);
    assert!(v[0].body.parents.is_empty(), "first version has parents: []");
}
