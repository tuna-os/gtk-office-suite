//! What Nextcloud's WebDAV, versions, locking and sharing really do, measured
//! one claim per test (RFC-0004 spike E, parts 1 and 3). Each test asserts the
//! behaviour the results document states and prints `MEASURE {...}` lines (and
//! appends them to `$DCP_MEASURE_OUT`) with the numbers it quotes.
//!
//! The spec's section 8.3 hypothesis is tested here directly: "a version is a
//! file state at an ETag; publishing is a conditional PUT; a 412 means another
//! version was published first".

mod common;

use std::time::{Duration, Instant};

use common::*;
use dcp_webdav::xml::{DAV, NC, OC};
use dcp_webdav::Dav;
use serde_json::json;

const LOCKINFO: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:lockinfo xmlns:d="DAV:"><d:lockscope><d:exclusive/></d:lockscope><d:locktype><d:write/></d:locktype><d:owner>dcp-spike</d:owner></d:lockinfo>"#;

async fn put(d: &Dav, name: &str, body: &[u8], headers: &[(&str, &str)]) -> dcp_webdav::Resp {
    d.request("PUT", &d.files_path(name), headers, Some(body.to_vec())).await.unwrap()
}

async fn etag_of(d: &Dav, name: &str) -> String {
    etag_and_modified(d, name).await.0
}

/// `(getetag, getlastmodified)` of a file: the ETag and the whole-second mtime.
async fn etag_and_modified(d: &Dav, name: &str) -> (String, String) {
    let (_, e) = d.propfind(&d.files_path(name), 0, &["d:getetag", "d:getlastmodified"]).await.unwrap();
    (e[0].get(DAV, "getetag").unwrap().to_owned(), e[0].get(DAV, "getlastmodified").unwrap().to_owned())
}

async fn share(d: &Dav, path: &str, with: &str, perms: u32) -> String {
    let perms = perms.to_string();
    let data = d
        .ocs(
            "POST",
            "apps/files_sharing/api/v1/shares",
            &[("path", path), ("shareType", "0"), ("shareWith", with), ("permissions", perms.as_str())],
        )
        .await
        .unwrap();
    data["id"].as_str().unwrap().to_owned()
}

async fn set_perms(d: &Dav, id: &str, perms: u32) {
    let perms = perms.to_string();
    d.ocs(
        "PUT",
        &format!("apps/files_sharing/api/v1/shares/{id}"),
        &[("permissions", perms.as_str())],
    )
    .await
    .unwrap();
}

// ------------------------------------------------------------------ ETags

/// Spec 8.3: "publishing a version is a conditional PUT (If-Match)". On Nextcloud
/// a create with `If-None-Match: *` is atomic, and an overwrite with `If-Match`
/// is not a reliable compare-and-set.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conditional_put_semantics() {
    if require("conditional_put_semantics", &[url_a()]).await.is_none() {
        return;
    }
    let a = dav(&url_a(), "alice");
    let name = unique("cond");

    // Sequential, one writer: the textbook behaviour.
    let r = put(&a, &name, b"one", &[("If-None-Match", "*")]).await;
    assert_eq!(r.status, 201);
    let e1 = r.etag().unwrap();
    assert_eq!(put(&a, &name, b"again", &[("If-None-Match", "*")]).await.status, 412, "create over an existing name");
    assert_eq!(put(&a, &name, b"x", &[("If-Match", "\"deadbeef\"")]).await.status, 412, "wrong ETag");

    // Is an overwrite always a new ETag? Twenty files each, replaced at once by
    // content of a different size and by content of the same size; then a second
    // writer that still holds the old ETag.
    let rounds = 20;
    let mut total_unchanged = 0;
    for (label, replacement) in [("different_size", &b"a replacement that is longer"[..]), ("same_size", &b"bbb"[..])] {
        let (mut unchanged, mut stale_accepted, mut stale_refused) = (0, 0, 0);
        let mut answer_matches_propfind = 0;
        // (etag moved, mtime second moved) -> count
        let mut by_cause = std::collections::BTreeMap::<&str, u32>::new();
        for i in 0..rounds {
            let n = format!("{name}-{label}-{i}");
            let e0 = put(&a, &n, b"aaa", &[("If-None-Match", "*")]).await.etag().unwrap();
            let (_, lm0) = etag_and_modified(&a, &n).await;
            let r = put(&a, &n, replacement, &[("If-Match", e0.as_str())]).await; // writer 1
            assert_eq!(r.status, 204);
            let (now, lm1) = etag_and_modified(&a, &n).await;
            if r.etag().as_deref() == Some(now.as_str()) {
                answer_matches_propfind += 1;
            }
            let key = match (now != e0, lm1 != lm0) {
                (false, false) => "etag_same_second_same",
                (false, true) => "etag_same_second_moved",
                (true, false) => "etag_moved_second_same",
                (true, true) => "etag_moved_second_moved",
            };
            *by_cause.entry(key).or_default() += 1;
            if now == e0 {
                unchanged += 1;
            }
            // Writer 2 read e0 before writer 1 wrote.
            let r2 = put(&a, &n, b"ccc", &[("If-Match", e0.as_str())]).await;
            match r2.status {
                204 => stale_accepted += 1,
                412 => stale_refused += 1,
                other => panic!("unexpected {other}"),
            }
        }
        measure(
            &format!("if_match_after_an_overwrite_{label}"),
            json!({ "rounds": rounds, "etag_unchanged": unchanged, "stale_writer_accepted": stale_accepted, "stale_writer_refused_412": stale_refused, "put_answer_etag_equals_propfind": answer_matches_propfind, "etag_vs_mtime_second": by_cause }),
        );
        // Whenever the ETag moved the stale writer is refused, and whenever it
        // did not the stale writer is accepted: If-Match is exactly as good as
        // the ETag, no better.
        assert_eq!(unchanged, stale_accepted, "{label}");
        assert_eq!(rounds - unchanged, stale_refused, "{label}");
        // The ETag moved exactly when the whole-second mtime moved.
        assert_eq!(by_cause.get("etag_same_second_moved").copied().unwrap_or(0), 0, "{label}: {by_cause:?}");
        assert_eq!(by_cause.get("etag_moved_second_same").copied().unwrap_or(0), 0, "{label}: {by_cause:?}");
        total_unchanged += unchanged;
    }
    let _ = e1;
    assert!(total_unchanged >= 1, "expected an overwrite that left the ETag as it was at least once in {} replacements", 2 * rounds);

    // Two writers at once with the same If-Match.
    let (mut both, mut one, mut none, mut statuses) = (0, 0, 0, std::collections::BTreeMap::<String, u32>::new());
    for i in 0..rounds {
        let n = format!("{name}-race-m-{i}");
        let e0 = put(&a, &n, b"base", &[("If-None-Match", "*")]).await.etag().unwrap();
        let hdr = [("If-Match", e0.as_str())];
        let (w1, w2) = tokio::join!(
            put(&a, &n, b"writer-one", &hdr),
            put(&a, &n, b"writer-2", &hdr)
        );
        *statuses.entry(format!("{}+{}", w1.status.min(w2.status), w1.status.max(w2.status))).or_default() += 1;
        match (w1.ok(), w2.ok()) {
            (true, true) => both += 1,
            (false, false) => none += 1,
            _ => one += 1,
        }
    }
    measure("if_match_concurrent", json!({ "rounds": rounds, "both_succeeded": both, "exactly_one": one, "neither": none, "status_pairs": statuses }));

    // Two creators at once.
    let (mut both, mut one, mut none, mut statuses) = (0, 0, 0, std::collections::BTreeMap::<String, u32>::new());
    for i in 0..rounds {
        let n = format!("{name}-race-c-{i}");
        let hdr = [("If-None-Match", "*")];
        let (w1, w2) = tokio::join!(
            put(&a, &n, b"creator-one", &hdr),
            put(&a, &n, b"creator-2", &hdr)
        );
        *statuses.entry(format!("{}+{}", w1.status.min(w2.status), w1.status.max(w2.status))).or_default() += 1;
        match (w1.ok(), w2.ok()) {
            (true, true) => both += 1,
            (false, false) => none += 1,
            _ => one += 1,
        }
        if w1.ok() != w2.ok() {
            let loser = if w1.ok() { &w2 } else { &w1 };
            assert!(matches!(loser.status, 412 | 423), "the loser got {}", loser.status);
        }
    }
    measure("if_none_match_concurrent", json!({ "rounds": rounds, "both_succeeded": both, "exactly_one": one, "neither": none, "status_pairs": statuses }));
    // Safety is the claim: never two winners. Liveness is not: in some rounds
    // Nextcloud's file lock refuses both creators (423 and 423).
    assert_eq!(both, 0, "two creators of the same name both succeeded");
    assert!(one >= rounds / 2, "most rounds should have a winner: {one} of {rounds}");
}

// ------------------------------------------------------------------ locks

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webdav_lock_semantics() {
    if require("webdav_lock_semantics", &[url_a()]).await.is_none() {
        return;
    }
    let a = dav(&url_a(), "alice");
    let b = dav(&url_a(), "bob");
    let name = unique("lock");
    put(&a, &name, b"lock me", &[]).await;
    share(&a, &format!("/{name}"), "bob", 3).await;

    // Does the server speak locking at all?
    let opt = a.request("OPTIONS", &a.files_path(""), &[], None).await.unwrap();
    let dav_header = opt.header("dav").unwrap_or("").to_owned();
    measure("options_dav_header", json!({ "dav": dav_header, "allow": opt.header("allow") }));
    assert!(opt.header("allow").unwrap().contains("LOCK"), "LOCK is advertised");

    let etag0 = etag_of(&a, &name).await;
    let lock_h = [("Content-Type", "application/xml"), ("Timeout", "Second-60")];
    let r = a.request("LOCK", &a.files_path(&name), &lock_h, Some(LOCKINFO.into())).await.unwrap();
    assert_eq!(r.status, 200, "{}", r.text());
    let token = r.header("lock-token").expect("a Lock-Token header").to_owned();
    assert!(r.text().contains("<d:timeout>Second-60</d:timeout>"));
    let etag_locked = etag_of(&a, &name).await;
    measure("lock_changes_etag", json!({ "before": etag0, "while_locked": etag_locked }));
    assert_ne!(etag0, etag_locked, "taking a lock changes the ETag, which breaks an If-Match chain");

    // Other writers are kept out, and so is the holder without its token.
    let bn = &name;
    assert_eq!(put(&b, bn, b"bob", &[]).await.status, 423, "another user's PUT on a locked file");
    let r = b.request("LOCK", &b.files_path(bn), &lock_h, Some(LOCKINFO.into())).await.unwrap();
    assert_eq!(r.status, 423, "another user's LOCK");
    assert_eq!(put(&a, &name, b"alice no token", &[]).await.status, 423, "the holder without the token");
    let if_hdr = format!("({token})");
    assert_eq!(put(&a, &name, b"alice with token", &[("If", if_hdr.as_str())]).await.status, 204);
    let r = a
        .request("LOCK", &a.files_path(&name), &[("Timeout", "Second-120"), ("If", if_hdr.as_str())], None)
        .await
        .unwrap();
    assert_eq!(r.status, 200, "refresh");
    assert!(r.text().contains("Second-120"));

    // A lock is authorised by its token alone, not by who took it.
    let r = b
        .request("UNLOCK", &b.files_path(bn), &[("Lock-Token", token.as_str())], None)
        .await
        .unwrap();
    measure("unlock_by_another_user_with_the_token", json!({ "status": r.status }));
    assert_eq!(r.status, 204, "bob released alice's lock with her token");
    assert_eq!(put(&b, bn, b"bob after unlock", &[]).await.status, 204);

    // Expiry.
    let r = a
        .request("LOCK", &a.files_path(&name), &[("Content-Type", "application/xml"), ("Timeout", "Second-2")], Some(LOCKINFO.into()))
        .await
        .unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(put(&b, bn, b"bob while locked", &[]).await.status, 423);
    tokio::time::sleep(Duration::from_millis(3500)).await;
    assert_eq!(put(&b, bn, b"bob after expiry", &[]).await.status, 204, "a 2 second lock expired");

    // The lock owner string is whatever the client sent.
    let r = a
        .request("LOCK", &a.files_path(&name), &lock_h, Some(LOCKINFO.into()))
        .await
        .unwrap();
    assert!(r.text().contains("<d:owner>dcp-spike</d:owner>"), "{}", r.text());
    let tok2 = r.header("lock-token").unwrap().to_owned();
    let (_, e) = a
        .propfind(&a.files_path(&name), 0, &["nc:lock-owner", "nc:lock-owner-type", "nc:lock-timeout", "nc:lock-time", "nc:lock-token"])
        .await
        .unwrap();
    measure(
        "sabre_lock_properties",
        json!({
            "owner": e[0].get(NC, "lock-owner"), "owner_type": e[0].get(NC, "lock-owner-type"),
            "timeout": e[0].get(NC, "lock-timeout"), "time": e[0].get(NC, "lock-time"), "token": e[0].get(NC, "lock-token"),
        }),
    );
    a.request("UNLOCK", &a.files_path(&name), &[("Lock-Token", tok2.as_str())], None).await.unwrap();

    // The files_lock convention: X-User-Lock names the authenticated user.
    let r = a
        .request("LOCK", &a.files_path(&name), &[("X-User-Lock", "1"), ("Content-Type", "application/xml")], Some(LOCKINFO.into()))
        .await
        .unwrap();
    assert_eq!(r.status, 200, "{}", r.text());
    let body = r.text();
    measure("user_lock_response", json!({ "body": body }));
    assert!(body.contains("<nc:lock-owner>alice</nc:lock-owner>"), "the owner is the authenticated user");
    let bob_put = put(&b, bn, b"bob while user-locked", &[]).await.status;
    let bob_unlock = b
        .request("UNLOCK", &b.files_path(bn), &[("X-User-Lock", "1")], None)
        .await
        .unwrap()
        .status;
    measure("user_lock_enforcement", json!({ "other_user_put": bob_put, "other_user_unlock": bob_unlock }));
    assert_eq!(bob_put, 423, "a user lock keeps other users out");
    let r = a.request("UNLOCK", &a.files_path(&name), &[("X-User-Lock", "1")], None).await.unwrap();
    measure("user_unlock_by_owner", json!({ "status": r.status }));

    // LOCK on a name that does not exist creates it.
    let ghost = format!("{name}-ghost");
    let r = a
        .request("LOCK", &a.files_path(&ghost), &[("Content-Type", "application/xml"), ("Timeout", "Second-30")], Some(LOCKINFO.into()))
        .await
        .unwrap();
    let (_, e) = a.propfind(&a.files_path(&ghost), 0, &["d:getcontentlength"]).await.unwrap();
    measure("lock_null_resource", json!({ "lock_status": r.status, "length": e[0].get(DAV, "getcontentlength") }));
    assert_eq!(r.status, 201);
    assert_eq!(e[0].get(DAV, "getcontentlength"), Some("0"));
}

// --------------------------------------------------------------- versions

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn versions_endpoint_keeps_a_state_per_second() {
    if require("versions_endpoint_keeps_a_state_per_second", &[url_a()]).await.is_none() {
        return;
    }
    let a = dav(&url_a(), "alice");
    let name = unique("ver");
    let r = put(&a, &name, b"state-0", &[]).await;
    let fid = r.header("oc-fileid").unwrap().to_owned();
    let t0 = Instant::now();
    // six overwrites: three quickly, a pause, three more. Seven states in all;
    // the current one is not a version.
    for i in 1..=3 {
        put(&a, &name, format!("state-{i}").as_bytes(), &[]).await;
    }
    tokio::time::sleep(Duration::from_millis(1100)).await;
    for i in 4..=6 {
        put(&a, &name, format!("state-{i}").as_bytes(), &[]).await;
    }
    let span = t0.elapsed();
    let path = format!("/remote.php/dav/versions/alice/versions/{fid}");
    let (r, entries) = a
        .propfind(&path, 1, &["d:getlastmodified", "d:getcontentlength", "d:getetag", "nc:version-author", "nc:version-label"])
        .await
        .unwrap();
    let mut seen = Vec::new();
    for e in entries.iter().filter(|e| !e.is_collection()) {
        let body = a.request("GET", &format!("{path}/{}", e.name()), &[], None).await.unwrap();
        seen.push(json!({
            "id": e.name(), "modified": e.get(DAV, "getlastmodified"), "length": e.get(DAV, "getcontentlength"),
            "author": e.get(NC, "version-author"), "label": e.get(NC, "version-label"), "content": body.text(),
        }));
    }
    measure("versions_endpoint", json!({ "overwrites": 6, "span_ms": ms(span), "listed": seen.len(), "list_ms": ms(r.elapsed), "versions": seen }));
    assert!(!seen.is_empty(), "the versions endpoint lists something");
    assert!(seen.len() < 6, "some of the replaced states were not kept: {}", seen.len());
    for v in &seen {
        assert_eq!(v["author"], "alice", "version-author is the authenticated writer");
    }
    // Versions cannot be written through the endpoint.
    let r = a.request("PUT", &format!("{path}/1234"), &[], Some(b"forged".to_vec())).await.unwrap();
    assert_eq!(r.status, 403);
}

// ------------------------------------------------- where could `parents` live

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_properties_are_per_user_and_per_file() {
    if require("custom_properties_are_per_user_and_per_file", &[url_a()]).await.is_none() {
        return;
    }
    let a = dav(&url_a(), "alice");
    let b = dav(&url_a(), "bob");
    let name = unique("props");
    let fid = put(&a, &name, b"props 1", &[]).await.header("oc-fileid").unwrap().to_owned();
    let patch = |v: &str| {
        format!(r#"<?xml version="1.0"?><d:propertyupdate xmlns:d="DAV:" xmlns:x="urn:dcp"><d:set><d:prop><x:parents>{v}</x:parents></d:prop></d:set></d:propertyupdate>"#)
    };
    let get = r#"<?xml version="1.0"?><d:propfind xmlns:d="DAV:" xmlns:x="urn:dcp"><d:prop><x:parents/></d:prop></d:propfind>"#;
    let read = |d: Dav, n: String| async move {
        let r = d
            .request("PROPFIND", &d.files_path(&n), &[("Depth", "0"), ("Content-Type", "application/xml")], Some(get.into()))
            .await
            .unwrap();
        let e = dcp_webdav::xml::parse_multistatus(&r.body).unwrap();
        e[0].get("urn:dcp", "parents").map(str::to_owned)
    };
    let r = a.request("PROPPATCH", &a.files_path(&name), &[("Content-Type", "application/xml")], Some(patch("v1,v2").into())).await.unwrap();
    assert_eq!(r.status, 207);
    assert_eq!(read(a.clone(), name.clone()).await.as_deref(), Some("v1,v2"), "an arbitrary namespaced property is stored");

    // It belongs to the FILE, not to a state of it: it survives an overwrite.
    put(&a, &name, b"props 2 (a different state)", &[]).await;
    let after_overwrite = read(a.clone(), name.clone()).await;
    assert_eq!(after_overwrite.as_deref(), Some("v1,v2"), "the old parents now describe a new state");

    // The versions endpoint does not carry it.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let vpath = format!("/remote.php/dav/versions/alice/versions/{fid}");
    let r = a
        .request(
            "PROPFIND",
            &vpath,
            &[("Depth", "1"), ("Content-Type", "application/xml")],
            Some(get.into()),
        )
        .await
        .unwrap();
    let e = dcp_webdav::xml::parse_multistatus(&r.body).unwrap();
    let on_versions = e.iter().filter(|e| e.get("urn:dcp", "parents").is_some()).count();
    assert_eq!(on_versions, 0, "no version carries the property");

    // And it is per user: bob, who may edit the file, neither sees nor changes alice's.
    share(&a, &format!("/{name}"), "bob", 3).await;
    assert_eq!(read(b.clone(), name.clone()).await, None, "bob does not see alice's property");
    let r = b.request("PROPPATCH", &b.files_path(&name), &[("Content-Type", "application/xml")], Some(patch("by-bob").into())).await.unwrap();
    assert_eq!(r.status, 207, "bob's PROPPATCH is accepted");
    assert_eq!(read(a.clone(), name.clone()).await.as_deref(), Some("v1,v2"), "alice still reads her own value");
    assert_eq!(read(b.clone(), name.clone()).await.as_deref(), Some("by-bob"));
    measure("custom_property_scope", json!({ "survives_overwrite": true, "on_versions_endpoint": false, "visible_to_other_user": false }));
}

// --------------------------------------------------------------- uploads

fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in input.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cut_off_upload_is_not_stored_and_checksums_are_not_checked() {
    if require("a_cut_off_upload_is_not_stored_and_checksums_are_not_checked", &[url_a()]).await.is_none() {
        return;
    }
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let base = url_a();
    let host = base.trim_start_matches("http://").to_owned();
    let a = dav(&base, "alice");
    let name = unique("cut");
    let auth = base64(format!("alice:{}", password()).as_bytes());
    let mut s = tokio::net::TcpStream::connect((host.as_str(), 80)).await.unwrap();
    let head = format!(
        "PUT /remote.php/dav/files/alice/{name} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Basic {auth}\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n"
    );
    s.write_all(head.as_bytes()).await.unwrap();
    s.write_all(&[b'x'; 500]).await.unwrap();
    s.shutdown().await.unwrap();
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(15), s.read_to_end(&mut out)).await;
    let status = String::from_utf8_lossy(&out).lines().next().unwrap_or("(no response)").to_owned();
    let r = a.request("PROPFIND", &a.files_path(&name), &[("Depth", "0")], None).await.unwrap();
    measure("cut_off_upload", json!({ "response": status, "file_exists_afterwards": r.status == 207 }));
    assert_eq!(r.status, 404, "a body shorter than its Content-Length must not leave a file; response was {status}");

    // OC-Checksum is stored as claimed.
    let n2 = unique("cks");
    let bogus = "SHA256:0000000000000000000000000000000000000000000000000000000000000000";
    let r = put(&a, &n2, b"checksum me", &[("OC-Checksum", bogus)]).await;
    let (_, e) = a.propfind(&a.files_path(&n2), 0, &["oc:checksums"]).await.unwrap();
    measure("oc_checksum_header", json!({ "put_status": r.status, "stored": e[0].get(OC, "checksums") }));
    assert_eq!(r.status, 201, "a wrong OC-Checksum is not rejected");
    assert!(e[0].get(OC, "checksums").unwrap().contains("0000000000"), "the claimed checksum is what is stored");
}

// -------------------------------------------------------------- sharing

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn share_permissions_are_the_roles() {
    if require("share_permissions_are_the_roles", &[url_a()]).await.is_none() {
        return;
    }
    let a = dav(&url_a(), "alice");
    let b = dav(&url_a(), "bob");
    let folder = unique("DCP-perm");
    assert_eq!(a.mkcol(&folder).await.unwrap().status, 201);
    put(&a, &format!("{folder}/existing.json"), b"{}", &[]).await;
    let sid = share(&a, &format!("/{folder}"), "bob", 1).await;
    let letters = |d: Dav, f: String| async move {
        let (_, e) = d.propfind(&d.files_path(&format!("{f}/")), 0, &["oc:permissions", "oc:owner-id"]).await.unwrap();
        (e[0].get(OC, "permissions").unwrap().to_owned(), e[0].get(OC, "owner-id").unwrap().to_owned())
    };
    let mut table = Vec::new();

    // viewer: 1
    assert_eq!(put(&b, &format!("{folder}/new.json"), b"{}", &[]).await.status, 403, "viewer create");
    assert_eq!(put(&b, &format!("{folder}/existing.json"), b"{}", &[]).await.status, 403, "viewer overwrite");
    let (l, owner) = letters(b.clone(), folder.clone()).await;
    assert_eq!(owner, "alice");
    table.push(json!({ "permissions": 1, "letters": l }));
    assert!(!l.contains('C') && !l.contains('R'), "{l}");

    // create-only: 5 = read + create
    set_perms(&a, &sid, 5).await;
    let created = put(&b, &format!("{folder}/v1.json"), b"{\"n\":1}", &[("If-None-Match", "*")]).await;
    let again = put(&b, &format!("{folder}/v1.json"), b"{\"n\":\"again\"}", &[("If-None-Match", "*")]).await;
    let over = put(&b, &format!("{folder}/existing.json"), b"{\"n\":\"bob\"}", &[]).await;
    let del = b.delete(&format!("{folder}/v1.json")).await.unwrap();
    let dest = format!("{}{}", b.base, b.files_path(&format!("{folder}/moved.json")));
    let mv = b
        .request(
            "MOVE",
            &b.files_path(&format!("{folder}/existing.json")),
            &[("Destination", dest.as_str())],
            None,
        )
        .await
        .unwrap();
    let (l, _) = letters(b.clone(), folder.clone()).await;
    measure(
        "create_only_share",
        json!({ "create": created.status, "create_again": again.status, "overwrite": over.status, "delete": del.status, "move": mv.status, "letters": l }),
    );
    table.push(json!({ "permissions": 5, "letters": l }));
    assert_eq!(created.status, 201);
    assert_eq!(again.status, 412);
    assert_eq!(over.status, 403, "create-only: no overwrite");
    assert_eq!(del.status, 403, "create-only: no delete");
    assert_eq!(mv.status, 403, "create-only: no rename");
    assert!(l.contains('C') && !l.contains('R'), "{l}");

    // editor: 15
    set_perms(&a, &sid, 15).await;
    let over = put(&b, &format!("{folder}/existing.json"), b"{\"n\":\"bob\"}", &[]).await;
    assert_eq!(over.status, 204, "update rights overwrite history");
    let (l, _) = letters(b.clone(), folder.clone()).await;
    table.push(json!({ "permissions": 15, "letters": l }));
    let fpath = format!("/{folder}");
    let reshare = b
        .ocs("POST", "apps/files_sharing/api/v1/shares", &[("path", fpath.as_str()), ("shareType", "0"), ("shareWith", "carol"), ("permissions", "1")])
        .await;
    assert!(reshare.is_err(), "an editor without the share right cannot reshare: {reshare:?}");

    // owner-level: 31
    set_perms(&a, &sid, 31).await;
    let (l, _) = letters(b.clone(), folder.clone()).await;
    table.push(json!({ "permissions": 31, "letters": l }));
    assert!(l.contains('R'), "{l}");
    let fpath = format!("/{folder}");
    let reshare = b
        .ocs("POST", "apps/files_sharing/api/v1/shares", &[("path", fpath.as_str()), ("shareType", "0"), ("shareWith", "carol"), ("permissions", "1")])
        .await;
    assert!(reshare.is_ok(), "{reshare:?}");

    // Revocation.
    a.ocs("DELETE", &format!("apps/files_sharing/api/v1/shares/{sid}"), &[]).await.unwrap();
    let r = b.request("PROPFIND", &b.files_path(&format!("{folder}/")), &[("Depth", "0")], None).await.unwrap();
    assert_eq!(r.status, 404, "after unsharing the folder does not exist for bob");
    measure("permission_letters_on_a_mount_root", json!(table));
}

// ------------------------------------------------------------- authorship

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn who_wrote_a_file() {
    if require("who_wrote_a_file", &[url_a()]).await.is_none() {
        return;
    }
    let a = dav(&url_a(), "alice");
    let b = dav(&url_a(), "bob");
    let folder = unique("DCP-who");
    a.mkcol(&folder).await.unwrap();
    share(&a, &format!("/{folder}"), "bob", 15).await;
    let r = put(&b, &format!("{folder}/by-bob.json"), b"{\"by\":\"bob\"}", &[]).await;
    assert_eq!(r.status, 201);
    let fid = r.header("oc-fileid").unwrap().to_owned();
    let numeric: String = fid.trim_start_matches('0').chars().take_while(char::is_ascii_digit).collect();
    let (_, e) = a
        .propfind(&a.files_path(&format!("{folder}/by-bob.json")), 0, &["oc:owner-id", "nc:upload_time", "nc:creation_time"])
        .await
        .unwrap();
    measure(
        "dav_properties_after_a_recipients_write",
        json!({ "owner_id": e[0].get(OC, "owner-id"), "upload_time": e[0].get(NC, "upload_time"), "creation_time": e[0].get(NC, "creation_time") }),
    );
    assert_eq!(e[0].get(OC, "owner-id"), Some("alice"), "WebDAV names the folder's owner, not the uploader");

    // The Activity app does name the uploader. How long until it does?
    let t0 = Instant::now();
    let mut found = None;
    while t0.elapsed() < Duration::from_secs(20) {
        let q = format!("apps/activity/api/v2/activity/filter?object_type=files&object_id={numeric}&limit=10");
        if let Ok(data) = a.ocs("GET", &q, &[]).await {
            if let Some(x) = data.as_array().and_then(|a| a.iter().find(|x| x["type"] == "file_created")) {
                found = Some((x["user"].as_str().unwrap_or("").to_owned(), t0.elapsed()));
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let (user, after) = found.expect("the Activity API reported the file's creation within 20 s");
    measure("activity_names_the_uploader", json!({ "user": user, "visible_after_ms": ms(after) }));
    assert_eq!(user, "bob");
}

// ------------------------------------------------------- the server clock

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_clock_has_whole_second_resolution() {
    if require("the_clock_has_whole_second_resolution", &[url_a()]).await.is_none() {
        return;
    }
    let a = dav(&url_a(), "alice");
    let t0 = Instant::now();
    let clock = clock(&url_a()).await;
    let calibration = t0.elapsed();
    let mut offsets = Vec::new();
    // Three seconds: the server's second ticks over several times.
    for _ in 0..24 {
        let before = clock.now_ms();
        let r = a.request("HEAD", "/robots.txt", &[], None).await.unwrap();
        let date_ms = r.date_ms().unwrap();
        // Date is floored to the second; the estimate must sit inside that second
        // (plus the time of the round trip).
        offsets.push(before as i64 - date_ms as i64);
        tokio::time::sleep(Duration::from_millis(130)).await;
    }
    let (lo, hi) = (*offsets.iter().min().unwrap(), *offsets.iter().max().unwrap());
    measure("clock_estimate_minus_date_header_ms", json!({ "calibration_ms": ms(calibration), "samples": offsets.len(), "min": lo, "max": hi }));
    // The estimate must sit inside the second the Date header names, give or take
    // the calibration error and a round trip.
    assert!(lo >= -30 && hi <= 1060, "the estimate left the server's second: {offsets:?}");
}

// --------------------------------------------------------------- latency

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn latency_of_the_operations_a_binding_uses() {
    if require("latency_of_the_operations_a_binding_uses", &[url_a()]).await.is_none() {
        return;
    }
    use dcp_core::DocumentStore;
    use dcp_conformance::{new_file, standard_meta};
    let base = url_a();
    let clock = clock(&base).await;
    let fault = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let owner = store(&base, "alice", &clock, &fault);
    let editor = store(&base, "bob", &clock, &fault);
    let a = dav(&base, "alice");
    let n = 15;

    let samples = |label: &str, v: Vec<f64>| measure(&format!("latency_{label}"), stats(v));
    let mut put_small = Vec::new();
    let mut get_small = Vec::new();
    let mut propfind = Vec::new();
    let mut create_new = Vec::new();
    let name = unique("lat");
    for i in 0..n {
        let r = put(&a, &format!("{name}-{i}"), &[b'x'; 1024], &[("If-None-Match", "*")]).await;
        put_small.push(ms(r.elapsed));
        let r = a.get(&format!("{name}-{i}")).await.unwrap();
        get_small.push(ms(r.elapsed));
        let (r, _) = a.propfind(&a.files_path(""), 0, &["d:getetag"]).await.unwrap();
        propfind.push(ms(r.elapsed));
    }
    samples("webdav_put_1kib", put_small);
    samples("webdav_get_1kib", get_small);
    samples("webdav_propfind_depth0_authenticated_as_the_tests_do", propfind);
    // The same request with the account password instead of an app password.
    let slow = Dav::new(&base, "alice", &password());
    let mut with_password = Vec::new();
    for _ in 0..n {
        let (r, _) = slow.propfind(&slow.files_path(""), 0, &["d:getetag"]).await.unwrap();
        with_password.push(ms(r.elapsed));
    }
    samples("webdav_propfind_depth0_with_the_account_password", with_password);
    let mut unauthenticated = Vec::new();
    for _ in 0..n {
        let r = a.request("GET", "/status.php", &[], None).await.unwrap();
        unauthenticated.push(ms(r.elapsed));
    }
    samples("status_php_get", unauthenticated);
    for _ in 0..5 {
        let t = Instant::now();
        let doc = owner.create_document(&standard_meta(), Some(new_file(b"base"))).await.unwrap();
        create_new.push(ms(t.elapsed()));
        let _ = doc;
    }
    samples("create_document_with_first_version", create_new);

    let doc = owner.create_document(&standard_meta(), Some(new_file(b"base"))).await.unwrap();
    owner.grant(&doc, &editor.principal(), dcp_core::Role::Editor).await.unwrap();
    let mut grant = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let d2 = owner.create_document(&standard_meta(), Some(new_file(b"base"))).await.unwrap();
        let c = t.elapsed();
        let t = Instant::now();
        owner.grant(&d2, &editor.principal(), dcp_core::Role::Editor).await.unwrap();
        grant.push(ms(t.elapsed()));
        let _ = c;
    }
    samples("grant_editor_two_shares", grant);

    let mut publish = Vec::new();
    let mut parent = owner.versions(&doc).await.unwrap()[0].id.clone();
    let mut sizes = std::collections::BTreeMap::new();
    for i in 0..30 {
        let t = Instant::now();
        let v = editor.publish_version(&doc, vec![parent.clone()], new_file(format!("version {i}").as_bytes()), None).await.unwrap();
        publish.push(ms(t.elapsed()));
        parent = v.id;
        if matches!(i + 2, 2 | 10 | 30) {
            let t = Instant::now();
            let all = owner.versions(&doc).await.unwrap();
            sizes.insert(all.len(), ms(t.elapsed()));
        }
    }
    samples("publish_version_end_to_end_1_small_file", publish);
    measure("versions_listing_ms_by_version_count", json!(sizes));

    let big: Vec<u8> = (0..(5 * 1024 * 1024)).map(|i| (i % 251) as u8).collect();
    let t = Instant::now();
    let v = editor.publish_version(&doc, vec![parent.clone()], new_file(&big), None).await.unwrap();
    let publish_5mib = ms(t.elapsed());
    let t = Instant::now();
    let back = owner.fetch_file(&doc, &v).await.unwrap();
    measure("5_mib_file", json!({ "publish_ms": publish_5mib, "fetch_ms": ms(t.elapsed()), "bytes_ok": back == big }));

    let mut lease_read = Vec::new();
    let mut lease_write = Vec::new();
    for _ in 0..n {
        let t = Instant::now();
        let l = owner.lease(&doc).await.unwrap();
        lease_read.push(ms(t.elapsed()));
        let rec = dcp_core::lease::released();
        let t = Instant::now();
        owner.write_lease(&doc, rec).await.unwrap();
        lease_write.push(ms(t.elapsed()));
        let _ = l;
    }
    samples("lease_read_propfind_plus_get", lease_read);
    samples("lease_write_create_one_record", lease_write);
}

// --------------------------------------------------------- the lease log

/// Lease rule 3's sleep, in real time.
struct RealPause;

#[async_trait::async_trait]
impl dcp_core::lease::Pause for RealPause {
    async fn pause(&self, ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

/// The lease log under real concurrency: two handles that have read record `n` both try to
/// create record `n + 1`, then both apply lease rule 3 (read back, settle, read back).
///
/// Creating a name with `If-None-Match: *` is NOT an atomic compare-and-set on Nextcloud: the
/// precondition is checked by a lookup that is not under the file lock, so a creator that passed
/// it before the other's record was visible can still replace that record and be answered `204`
/// (RFC-0004 spike E results, "The lease"). Both writers are then told their write was accepted
/// and the log holds the later one. What the binding guarantees is therefore the two-read rule,
/// and that is what this asserts: after it, at most one session holds the lease, and when any
/// write was accepted exactly one does. How often the server accepted both writes, and the
/// whole exchange each time it did, are printed and measured, not asserted.
///
/// `DCP_LEASE_RACE_ROUNDS` (default 20) sets the rounds, `DCP_LEASE_RACE_EDITOR=alice` makes the
/// second writer a second client of the owner's account instead of a sharee (bob).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lease_log_create_race_leaves_one_holder_after_the_read_backs() {
    use dcp_conformance::{new_file, standard_meta};
    use dcp_core::lease::{self, released, Attempt, Claim, Settle, MATRIX_SETTLE_MS};
    use dcp_core::{DocumentStore, Role, StoreError};
    if require("lease_log_create_race_leaves_one_holder_after_the_read_backs", &[url_a()]).await.is_none() {
        return;
    }
    let base = url_a();
    let clock = clock(&base).await;
    let fault = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let owner = store(&base, "alice", &clock, &fault);
    let editor_user = std::env::var("DCP_LEASE_RACE_EDITOR").unwrap_or_else(|_| "bob".into());
    let editor = store(&base, &editor_user, &clock, &fault);
    let doc = owner.create_document(&standard_meta(), Some(new_file(b"base"))).await.unwrap();
    if editor_user != "alice" {
        owner.grant(&doc, &editor.principal(), Role::Editor).await.unwrap();
    }
    let first = owner.versions(&doc).await.unwrap()[0].id.clone();
    let settle = Settle::new(&RealPause, MATRIX_SETTLE_MS);

    let rounds: usize = std::env::var("DCP_LEASE_RACE_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    // Writes the server accepted (the store's `Ok`): none, one, both.
    let (mut none_accepted, mut one_accepted, mut both_accepted) = (0, 0, 0);
    // Sessions that held the lease after the read-backs, per round: 0 and 1 are expected.
    let mut holders = std::collections::BTreeMap::<usize, u32>::new();
    let mut losers = std::collections::BTreeMap::<String, u32>::new();
    // The HTTP statuses of the two PUTs, as the server answered them.
    let mut status_pairs = std::collections::BTreeMap::<String, u32>::new();
    let mut violations = Vec::<String>::new();
    let mut not_located = 0;
    for i in 0..rounds {
        // Both read the same latest record, as two sessions that looked at once.
        let seen_o = owner.lease(&doc).await.unwrap();
        let seen_e = editor.lease(&doc).await.unwrap();
        assert_eq!(seen_o.as_ref().map(|l| l.id.clone()), seen_e.as_ref().map(|l| l.id.clone()));
        let next = seen_o.as_ref().map_or(0, |l| l.id.parse::<u64>().unwrap() + 1);
        let co = Claim::new(owner.principal(), format!("o{i}"), "t", &first, 60_000);
        let ce = Claim::new(editor.principal(), format!("e{i}"), "t", &first, 60_000);
        let (lo, le) = (co.lease_over(seen_o.as_ref()), ce.lease_over(seen_e.as_ref()));
        dcp_webdav::dav::trace_begin();
        let (wo, we) = tokio::join!(owner.write_lease(&doc, lo.clone()), editor.write_lease(&doc, le.clone()));
        let trace = dcp_webdav::dav::trace_take();
        let (ao, ae) = (Attempt { lease: lo, result: wo }, Attempt { lease: le, result: we });
        let puts: Vec<&dcp_webdav::dav::Exchange> = trace.iter().filter(|e| e.method == "PUT").collect();
        let mut pair: Vec<String> = puts.iter().map(|e| e.status.map_or("none".to_owned(), |s| s.to_string())).collect();
        pair.sort();
        *status_pairs.entry(pair.join("+")).or_default() += 1;
        match (ao.result.is_ok(), ae.result.is_ok()) {
            (false, false) => none_accepted += 1,
            (true, true) => both_accepted += 1,
            _ => one_accepted += 1,
        }
        // A write that was not accepted is a `Conflict` (412, 423). Nextcloud also answers a
        // create `404`, "File with name ... could not be located", to one of two creators of
        // one name while the other's `201` stands (RFC-0004 spike E results, "The lease"); the
        // store maps a 404 on a write to `Forbidden`. It is counted and quoted, and the write
        // is as lost as a conflict. Anything else is a failure.
        for (who, a) in [("owner", &ao), ("editor", &ae)] {
            match &a.result {
                Ok(_) => {}
                Err(e @ StoreError::Conflict(_)) => *losers.entry(e.to_string().chars().take(60).collect()).or_default() += 1,
                Err(e @ StoreError::Forbidden(m)) if m.contains("HTTP 404") => {
                    not_located += 1;
                    *losers.entry(e.to_string().chars().take(60).collect()).or_default() += 1;
                }
                Err(e) => violations.push(format!("round {i}: the {who}'s write was neither accepted, a Conflict nor a 404: {e:?}")),
            }
        }
        // The server did something other than 201 + 412/423: quote the whole exchange, and then
        // what the log holds.
        if puts.len() != 2 || puts.iter().any(|e| !matches!(e.status, Some(201 | 412 | 423))) {
            show_exchange(i, &format!("record {next}: the owner's write ok={}, the editor's ok={}", ao.result.is_ok(), ae.result.is_ok()), &trace);
            show_record_state(&base, &doc, next, &editor_user).await;
        }

        // Lease rule 3 for both, as two sessions would run it.
        let (ro, re) = tokio::join!(
            lease::confirm(&owner, &doc, &co, &ao, &settle),
            lease::confirm(&editor, &doc, &ce, &ae, &settle)
        );
        let (ro, re) = (ro.unwrap(), re.unwrap());
        let held = usize::from(ro.is_acquired()) + usize::from(re.is_acquired());
        *holders.entry(held).or_default() += 1;
        let accepted = usize::from(ao.result.is_ok()) + usize::from(ae.result.is_ok());
        if held > 1 || (accepted > 0 && held != 1) {
            // Counted and quoted, asserted below: a stress run reports every such round.
            let line = format!("round {i}, record {next}: statuses {pair:?}, writes accepted {accepted}, sessions holding after the read-backs {held}: owner {ro:?}, editor {re:?}");
            println!("LEASE-VIOLATION {line}");
            violations.push(line);
        }
        // A release keeps the chain going from the holder's record.
        if i % 5 == 4 {
            owner.lease(&doc).await.unwrap();
            owner.write_lease(&doc, released()).await.unwrap();
        }
    }
    measure(
        "lease_log_create_race",
        json!({
            "writers": format!("alice+{editor_user}"), "rounds": rounds,
            "no_write_accepted": none_accepted, "one_write_accepted": one_accepted, "both_writes_accepted": both_accepted,
            "writes_answered_404": not_located,
            "sessions_holding_after_the_read_backs": holders, "http_status_pairs": status_pairs, "loser_errors": losers
        }),
    );
    assert!(violations.is_empty(), "lease rule 3 did not leave exactly one holder, or a write failed in an unexpected way: {violations:#?}");
    assert!(one_accepted + both_accepted >= 1, "no round had an accepted write");
}

/// Print every request of one race, in the order they were sent, with the headers both ways.
fn show_exchange(round: usize, why: &str, trace: &[dcp_webdav::dav::Exchange]) {
    let t0 = trace.iter().map(|e| e.sent_us).min().unwrap_or(0);
    let rel = |us: u128| (us as f64 - t0 as f64) / 1000.0;
    println!("RACE-EXCHANGE round {round}: {why}; times are ms after the first request was sent");
    for e in trace {
        println!("  [{:>9.3} .. {:>9.3}] {} {} {}", rel(e.sent_us), rel(e.done_us), e.user, e.method, e.path);
        for (k, v) in &e.request_headers {
            println!("      > {k}: {v}");
        }
        println!("      > (body: {} bytes)", e.request_bytes);
        match (e.status, &e.error) {
            (Some(s), _) => println!("      < HTTP {s}"),
            (None, err) => println!("      < no response: {err:?}"),
        }
        for (k, v) in &e.response_headers {
            println!("      < {k}: {v}");
        }
        if !e.response_body.is_empty() {
            println!("      < body: {}", e.response_body.replace('\n', " "));
        }
    }
}

/// What the server holds for the contested record afterwards, seen by both users: the directory
/// listing with file ids and ETags, and the record's bytes.
async fn show_record_state(base: &str, doc: &str, next: u64, editor_user: &str) {
    let mut users = vec!["alice", editor_user];
    users.dedup();
    for user in users {
        let d = dav(base, user);
        let dir = d.files_path(&format!("{doc}.l/"));
        match d.propfind(&dir, 1, &["d:getetag", "d:getlastmodified", "d:getcontentlength", "oc:fileid", "oc:owner-id"]).await {
            Ok((_, entries)) => {
                for e in entries.iter().filter(|e| e.name().ends_with(".json")) {
                    let n = e.name();
                    // Only the contested record and its neighbours.
                    if n.strip_suffix(".json").and_then(|s| s.parse::<u64>().ok()).is_some_and(|k| k + 2 >= next) {
                        println!(
                            "  STATE {user} sees {n}: fileid={:?} etag={:?} modified={:?} length={:?} owner={:?}",
                            e.get(OC, "fileid"), e.get(DAV, "getetag"), e.get(DAV, "getlastmodified"), e.get(DAV, "getcontentlength"), e.get(OC, "owner-id")
                        );
                    }
                }
            }
            Err(err) => println!("  STATE {user}: PROPFIND failed: {err}"),
        }
        match d.get(&format!("{doc}.l/{next:08}.json")).await {
            Ok(r) => println!("  STATE {user} GET {next:08}.json -> HTTP {}: {}", r.status, r.text().replace('\n', " ")),
            Err(err) => println!("  STATE {user}: GET failed: {err}"),
        }
    }
}

// ----------------------------------------------------- forged authorship

/// An editor can write a version object that says `created_by` is the owner; the
/// server accepts it and nothing in WebDAV says otherwise. The Activity app does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_editor_can_claim_to_be_the_owner() {
    use dcp_conformance::{new_file, standard_meta};
    use dcp_core::{Author, DocumentStore, Role};
    if require("an_editor_can_claim_to_be_the_owner", &[url_a()]).await.is_none() {
        return;
    }
    let base = url_a();
    let clock = clock(&base).await;
    let fault = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let owner = store(&base, "alice", &clock, &fault);
    let editor = store(&base, "bob", &clock, &fault);
    let doc = owner.create_document(&standard_meta(), Some(new_file(b"base"))).await.unwrap();
    owner.grant(&doc, &editor.principal(), Role::Editor).await.unwrap();
    let first = owner.versions(&doc).await.unwrap().remove(0);

    let mut forged = first.body.clone();
    forged.parents = vec![first.id.clone()];
    forged.created_by = Author { principal: owner.principal(), device: "alice-device".into() };
    forged.message = Some("written by bob, signed alice".into());
    let name = unique("forged");
    let accepted = editor.put_raw_version(&doc, &name, &forged).await.unwrap();
    assert_eq!(accepted.as_deref(), Some(name.as_str()), "the server accepted an editor's version that names the owner");

    let listed = owner.versions(&doc).await.unwrap();
    let v = listed.iter().find(|v| v.id == name).expect("the forged version is listed");
    assert_eq!(v.sender, owner.principal(), "the store reports the author the object claims");

    // What the server knows: who created the file.
    let a = dav(&base, "alice");
    let (_, e) = a
        .propfind(&a.files_path(&format!("{doc}.v/{name}.json")), 0, &["oc:fileid", "oc:owner-id"])
        .await
        .unwrap();
    let fid = e[0].get(OC, "fileid").unwrap().to_owned();
    let t0 = Instant::now();
    let mut creator = None;
    while t0.elapsed() < Duration::from_secs(20) {
        let q = format!("apps/activity/api/v2/activity/filter?object_type=files&object_id={fid}&limit=10");
        if let Ok(data) = a.ocs("GET", &q, &[]).await {
            if let Some(x) = data.as_array().and_then(|a| a.iter().find(|x| x["type"] == "file_created")) {
                creator = x["user"].as_str().map(str::to_owned);
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    measure(
        "forged_created_by",
        json!({ "claimed": v.sender, "webdav_owner_id": e[0].get(OC, "owner-id"), "activity_says_created_by": creator }),
    );
    assert_eq!(creator.as_deref(), Some("bob"), "the Activity API names the real writer");
}
