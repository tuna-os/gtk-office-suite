//! Open Cloud Mesh against Nextcloud (RFC-0004 spike E, part 2): what the
//! discovery document says, what a share creation notification carries, what a
//! receiving server holding only the share's secret can do over WebDAV, and what
//! happens to it when the sender changes or removes the share.
//!
//! The receiver here is `dcp_webdav::ocm::OcmReceiver`, a few dozen lines with no
//! file store: every request Nextcloud sends it is recorded verbatim. The tests
//! that need two Nextcloud instances share between them with no helper in the
//! middle.

mod common;

use std::time::Duration;

use common::*;
use dcp_conformance::{new_file, standard_meta, BASE_FILE};
use dcp_core::{DocumentStore, Role};
use dcp_webdav::ocm::{accept_pending_remote_shares, OcmReceiver};
use dcp_webdav::xml::{self, DAV};
use dcp_webdav::Dav;
use serde_json::{json, Value};

const LOCKINFO: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:lockinfo xmlns:d="DAV:"><d:lockscope><d:exclusive/></d:lockscope><d:locktype><d:write/></d:locktype><d:owner>dcp-spike</d:owner></d:lockinfo>"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ocm_discovery_documents() {
    if require("ocm_discovery_documents", &[url_a(), url_b()]).await.is_none() {
        return;
    }
    let a = dav(&url_a(), "alice");
    let mut docs = Vec::new();
    for path in ["/.well-known/ocm", "/ocm-provider", "/ocm-provider/", "/index.php/ocm-provider/", "/.well-known/ocm.json", "/ocm"] {
        let r = a.request("GET", path, &[("Accept", "application/json")], None).await.unwrap();
        docs.push(json!({ "path": path, "status": r.status, "length": r.body.len() }));
    }
    measure("ocm_discovery_paths", json!(docs));
    let r = a.request("GET", "/.well-known/ocm", &[("Accept", "application/json")], None).await.unwrap();
    assert_eq!(r.status, 200);
    let doc: Value = serde_json::from_slice(&r.body).unwrap();
    let mut shown = doc.clone();
    shown["publicKey"]["publicKeyPem"] = json!("<elided>");
    measure("ocm_discovery_document_nc_a", shown);
    assert_eq!(doc["enabled"], true);
    assert!(doc["resourceTypes"][0]["protocols"]["webdav"].is_string(), "webdav is offered");
    // Web-app access mode (the OCM spec's `webapp` protocol) is not offered, and
    // neither are the capabilities its code flow needs.
    let protocols = doc["resourceTypes"][0]["protocols"].as_object().unwrap();
    let caps: Vec<&str> = doc["capabilities"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    measure("ocm_webapp_mode", json!({ "protocols": protocols.keys().collect::<Vec<_>>(), "capabilities": caps, "apiVersion": doc["apiVersion"], "version": doc["version"] }));
    assert!(!protocols.contains_key("webapp") && !protocols.contains_key("webapp-receive"));
    // What the server offers besides webdav varies with the release: 34 offers
    // `notifications` and `shares`; 35 adds `exchange-token` and `http-sig`
    // with a token endpoint and a JWKS. Neither offers `webapp`.
    measure(
        "ocm_code_flow_advertised",
        json!({ "exchange_token": caps.contains(&"exchange-token"), "http_sig": caps.contains(&"http-sig"), "tokenEndPoint": doc.get("tokenEndPoint"), "jwksUri": doc.get("jwksUri") }),
    );
}

/// What crosses the wire, and what a secret-holding receiver can do.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_receiver_with_only_the_share_secret() {
    if require("a_receiver_with_only_the_share_secret", &[url_a()]).await.is_none() {
        return;
    }
    let base = url_a();
    let clock = clock(&base).await;
    let fault = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let alice = store(&base, "alice", &clock, &fault);
    let recv = OcmReceiver::start("dcp-host").await.unwrap();
    println!("receiver listening on {} advertised as {}", recv.addr, recv.host());

    let doc = alice.create_document(&standard_meta(), Some(new_file(BASE_FILE))).await.unwrap();
    let dave = format!("dave@{}", recv.host());
    alice.grant(&doc, &dave, Role::Editor).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // 1. Discovery, then one share creation notification per folder.
    let records = recv.records();
    let discovery: Vec<_> = records.iter().filter(|r| r.method == "GET").collect();
    assert!(!discovery.is_empty(), "the sender looked the receiver up first");
    measure(
        "ocm_sender_discovery_requests",
        json!(discovery.iter().map(|r| json!({ "path": r.path, "user_agent": r.header("user-agent") })).collect::<Vec<_>>()),
    );
    let shares = recv.shares();
    assert_eq!(shares.len(), 2, "one notification per shared folder (.v and .l): {records:#?}");
    let mut secrets = std::collections::BTreeMap::new();
    let mut shown = Vec::new();
    for s in &shares {
        let sig = s.recorded.header("signature").unwrap_or("");
        shown.push(json!({
            "path": s.recorded.path,
            "content_type": s.recorded.header("content-type"),
            "digest": s.recorded.header("digest"),
            "date": s.recorded.header("date"),
            "signature_params": sig.split("signature=").next(),
            "body": { // the secret is a throw-away test token
                "shareWith": s.share_with(), "shareType": s.body["shareType"], "name": s.name(), "resourceType": s.body["resourceType"],
                "providerId": s.provider_id(), "owner": s.owner(), "sender": s.sender(), "sharedBy": s.body["sharedBy"],
                "protocol": s.body["protocol"],
                "other_keys": s.body.as_object().unwrap().keys().collect::<Vec<_>>(),
            }
        }));
        assert_eq!(s.protocol_name(), Some("webdav"));
        assert_eq!(s.share_with(), Some(format!("dave@http://{}", recv.host()).as_str()));
        assert!(sig.contains("keyId=") && sig.contains("rsa-sha512"), "the notification is signed: {sig}");
        secrets.insert(s.name().unwrap().to_owned(), s.shared_secret().unwrap().to_owned());
    }
    measure("ocm_share_creation_notifications", json!(shown));
    let secret_v = secrets.get(&format!("{doc}.v")).expect("a share of the version folder").clone();
    let secret_l = secrets.get(&format!("{doc}.l")).expect("a share of the lease folder").clone();
    assert_ne!(secret_v, secret_l);

    // 2. What the secret opens. Basic (`secret:`) and Bearer, on the version folder (perm 5).
    let tv = Dav::share_basic(&base, &secret_v);
    let tv_bearer = Dav::share_bearer(&base, &secret_v);
    let r = tv.request("PROPFIND", "/public.php/webdav/", &[("Depth", "1")], None).await.unwrap();
    let listing: Vec<String> = if r.status == 207 { xml::parse_multistatus(&r.body).unwrap().iter().map(|e| e.name()).collect() } else { vec![] };
    let bearer = tv_bearer.request("PROPFIND", "/public.php/webdav/", &[("Depth", "1")], None).await.unwrap();
    measure("token_propfind", json!({ "basic_status": r.status, "bearer_status": bearer.status, "listing": listing }));
    assert_eq!(r.status, 207, "{}", r.text());
    assert!(listing.iter().any(|n| n == "meta.json"));
    let meta = tv.get("meta.json").await.unwrap();
    assert_eq!(meta.status, 200);
    let perm_prop = tv
        .propfind("/public.php/webdav/", 0, &["ocm:share-permissions", "oc:permissions", "d:getetag"])
        .await
        .unwrap()
        .1;
    measure(
        "token_permission_properties_v",
        json!({ "ocm_share_permissions": perm_prop[0].get("http://open-cloud-mesh.org/ns", "share-permissions"), "oc_permissions": perm_prop[0].get("http://owncloud.org/ns", "permissions") }),
    );

    let create = tv.put_new("from-receiver.json", b"{\"hello\":1}".to_vec()).await.unwrap();
    let create_again = tv.put_new("from-receiver.json", b"{\"hello\":2}".to_vec()).await.unwrap();
    let overwrite = tv.request("PUT", "/public.php/webdav/meta.json", &[], Some(b"{}".to_vec())).await.unwrap();
    let delete = tv.request("DELETE", "/public.php/webdav/from-receiver.json", &[], None).await.unwrap();
    let lock = tv
        .request("LOCK", "/public.php/webdav/meta.json", &[("Content-Type", "application/xml"), ("Timeout", "Second-30")], Some(LOCKINFO.into()))
        .await
        .unwrap();
    measure(
        "token_writes_on_the_create_only_share",
        json!({ "create": create.status, "create_again": create_again.status, "overwrite": overwrite.status, "delete": delete.status, "lock": lock.status, "bearer_get_meta": tv_bearer.get("meta.json").await.unwrap().status }),
    );
    assert_eq!(create.status, 201);
    assert_eq!(create_again.status, 412);
    assert_eq!(overwrite.status, 403, "create-only is enforced for a federated receiver too");
    assert_eq!(delete.status, 403);

    // The lease folder (perm 15): a receiver can write lease records.
    let tl = Dav::share_basic(&base, &secret_l);
    let rec = tl.put_new("00000000.json", b"{}".to_vec()).await.unwrap();
    let over = tl.request("PUT", "/public.php/webdav/00000000.json", &[], Some(b"{\"x\":1}".to_vec())).await.unwrap();
    let lock = tl
        .request("LOCK", "/public.php/webdav/00000000.json", &[("Content-Type", "application/xml"), ("Timeout", "Second-30")], Some(LOCKINFO.into()))
        .await
        .unwrap();
    measure("token_writes_on_the_lease_share", json!({ "create": rec.status, "overwrite": over.status, "lock": lock.status }));
    assert_eq!(rec.status, 201);

    // 3. The sender changes the role, then removes the receiver.
    let before = recv.records().len();
    alice.grant(&doc, &dave, Role::Viewer).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let after_downgrade = recv.records()[before..].iter().map(|r| format!("{} {}", r.method, r.path)).collect::<Vec<_>>();
    let put_after = tv.put_new("after-downgrade.json", b"{}".to_vec()).await.unwrap();
    let get_after = tv.get("meta.json").await.unwrap();
    measure(
        "after_the_sender_downgrades_the_share",
        json!({ "requests_to_the_receiver": after_downgrade, "put_status": put_after.status, "get_status": get_after.status }),
    );
    assert_eq!(put_after.status, 403, "the sender enforces the new permission");
    assert!(after_downgrade.is_empty(), "no OCM notification announced the permission change: {after_downgrade:?}");

    let before = recv.records().len();
    alice.revoke(&doc, &dave).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let notes: Vec<Value> = recv.records()[before..]
        .iter()
        .filter(|r| r.path.ends_with("/ocm/notifications"))
        .filter_map(|r| r.json())
        .collect();
    let get_gone = tv.get("meta.json").await.unwrap();
    measure("after_the_sender_removes_the_receiver", json!({ "notifications": notes, "get_status": get_gone.status }));
    assert!(!notes.is_empty(), "the sender notified the receiver");
    assert!(notes.iter().all(|n| n["notificationType"] == "SHARE_UNSHARED"), "{notes:?}");
    assert!(matches!(get_gone.status, 401 | 404), "the secret stopped working: {}", get_gone.status);
}

/// Locks across a federation: a lock taken through the receiving server.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn locks_do_not_cross_a_federation() {
    if require("locks_do_not_cross_a_federation", &[url_a(), url_b()]).await.is_none() {
        return;
    }
    let clock_a = clock(&url_a()).await;
    let fault = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let alice = store(&url_a(), "alice", &clock_a, &fault);
    let bob_b = store(&url_b(), "bob", &clock(&url_b()).await, &fault);
    let doc = alice.create_document(&standard_meta(), Some(new_file(BASE_FILE))).await.unwrap();
    alice.grant(&doc, &bob_b.principal(), Role::Editor).await.unwrap();
    assert_eq!(accept_pending_remote_shares(bob_b.dav(), &doc).await.unwrap(), 2);

    // A file both sides can reach: the lease folder's first record.
    let rel = format!("{doc}.l/probe.json");
    let a = dav(&url_a(), "alice");
    let b = bob_b.dav().clone();
    assert_eq!(b.put_new(&rel, b"{}".to_vec()).await.unwrap().status, 201, "bob writes through his own server to alice's");

    // bob (via nc-b) takes a LOCK; alice writes on nc-a.
    let lock = b
        .request("LOCK", &b.files_path(&rel), &[("Content-Type", "application/xml"), ("Timeout", "Second-60")], Some(LOCKINFO.into()))
        .await
        .unwrap();
    let token = lock.header("lock-token").unwrap_or("").to_owned();
    let alice_put = a.request("PUT", &a.files_path(&rel), &[], Some(b"{\"alice\":1}".to_vec())).await.unwrap();
    let a_props = a.propfind(&a.files_path(&rel), 0, &["d:lockdiscovery", "nc:lock"]).await.unwrap().1;
    measure(
        "lock_taken_through_the_receiving_server",
        json!({ "lock_status": lock.status, "alice_put_on_the_sender": alice_put.status, "lock_visible_on_the_sender": a_props[0].get(DAV, "lockdiscovery") }),
    );
    assert_eq!(lock.status, 200);
    assert!(matches!(alice_put.status, 204 | 201), "the sender's own write was not blocked by a lock the receiver holds: {}", alice_put.status);
    if !token.is_empty() {
        let _ = b.request("UNLOCK", &b.files_path(&rel), &[("Lock-Token", token.as_str())], None).await;
    }

    // alice takes a LOCK on the sender; bob writes through nc-b.
    let lock = a
        .request("LOCK", &a.files_path(&rel), &[("Content-Type", "application/xml"), ("Timeout", "Second-60")], Some(LOCKINFO.into()))
        .await
        .unwrap();
    let bob_put = b.request("PUT", &b.files_path(&rel), &[], Some(b"{\"bob\":1}".to_vec())).await.unwrap();
    measure("lock_taken_on_the_sender", json!({ "lock_status": lock.status, "receiver_put_status": bob_put.status }));
    assert_eq!(lock.status, 200);
    assert!(matches!(bob_put.status, 204 | 201), "a write that arrives through the federation ignores the sender's Sabre lock: {}", bob_put.status);
    let tok = lock.header("lock-token").unwrap_or("").to_owned();
    let _ = a.request("UNLOCK", &a.files_path(&rel), &[("Lock-Token", tok.as_str())], None).await;

    // The files_lock "user lock" (X-User-Lock) instead of a Sabre lock.
    let ul = a
        .request("LOCK", &a.files_path(&rel), &[("X-User-Lock", "1"), ("Content-Type", "application/xml")], Some(LOCKINFO.into()))
        .await
        .unwrap();
    let bob_put2 = b.request("PUT", &b.files_path(&rel), &[], Some(b"{\"bob\":2}".to_vec())).await.unwrap();
    measure("user_lock_on_the_sender", json!({ "lock_status": ul.status, "receiver_put_status": bob_put2.status }));
    let _ = a.request("UNLOCK", &a.files_path(&rel), &[("X-User-Lock", "1")], None).await;

}

/// What the receiving server tells a receiver about its role.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn what_a_federated_mount_says_about_the_role() {
    if require("what_a_federated_mount_says_about_the_role", &[url_a(), url_b()]).await.is_none() {
        return;
    }
    let clock_a = clock(&url_a()).await;
    let fault = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let alice = store(&url_a(), "alice", &clock_a, &fault);
    let bob_b = store(&url_b(), "bob", &clock(&url_b()).await, &fault);
    let doc = alice.create_document(&standard_meta(), Some(new_file(BASE_FILE))).await.unwrap();
    let who = bob_b.principal();
    let mut rows = Vec::new();
    for (i, role) in [Role::Viewer, Role::Editor, Role::Owner, Role::Viewer].into_iter().enumerate() {
        alice.grant(&doc, &who, role).await.unwrap();
        if i == 0 {
            assert_eq!(accept_pending_remote_shares(bob_b.dav(), &doc).await.unwrap(), 2);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        let mut row = json!({ "granted": format!("{role:?}") });
        for dir in ["v", "l"] {
            let (_, e) = bob_b
                .dav()
                .propfind(
                    &bob_b.dav().files_path(&format!("{doc}.{dir}/")),
                    0,
                    &["oc:permissions", "oc:owner-id", "nc:mount-type", "nc:is-mount-root", "oc:share-types", "ocm:share-permissions"],
                )
                .await
                .unwrap();
            row[dir] = json!({
                "oc_permissions": e[0].get("http://owncloud.org/ns", "permissions"),
                "owner_id": e[0].get("http://owncloud.org/ns", "owner-id"),
                "mount_type": e[0].get("http://nextcloud.org/ns", "mount-type"),
                "ocm_share_permissions": e[0].get("http://open-cloud-mesh.org/ns", "share-permissions"),
            });
        }
        let remote = bob_b.dav().ocs("GET", "apps/files_sharing/api/v1/remote_shares", &[]).await.unwrap();
        row["remote_shares_on_the_receiver"] = json!(remote
            .as_array()
            .map(|a| a.iter().filter(|s| s["name"].as_str().is_some_and(|n| n.contains(&doc))).map(|s| json!({ "name": s["name"], "permissions": s["permissions"], "accepted": s["accepted"] })).collect::<Vec<_>>()));
        let seen = bob_b.role(&doc).await.unwrap();
        row["role_as_the_store_reads_it"] = json!(format!("{seen:?}"));
        rows.push(row);
        // Over OCM only read and write survive: an owner-level share reads as an editor.
        let expect = match role {
            Role::Viewer => Role::Viewer,
            _ => Role::Editor,
        };
        assert_eq!(seen, expect, "granted {role:?}");
    }
    measure("federated_mount_permissions_by_granted_role", json!(rows));
}

/// A receiver that says it can verify HTTP message signatures and honour the code
/// flow: does the sender change what it sends?
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_receiver_that_advertises_the_newer_capabilities() {
    if require("a_receiver_that_advertises_the_newer_capabilities", &[url_a()]).await.is_none() {
        return;
    }
    let base = url_a();
    let clock = clock(&base).await;
    let fault = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let alice = store(&base, "alice", &clock, &fault);
    let recv = OcmReceiver::start_advertising("dcp-host", &["notifications", "exchange-token", "http-sig"]).await.unwrap();
    let doc = alice.create_document(&standard_meta(), Some(new_file(BASE_FILE))).await.unwrap();
    alice.grant(&doc, &format!("dave@{}", recv.host()), Role::Viewer).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let shares = recv.shares();
    assert!(!shares.is_empty(), "{:#?}", recv.records());
    let s = &shares[0];
    measure(
        "share_notification_to_a_receiver_that_advertises_http_sig_and_exchange_token",
        json!({
            "signature_header": s.recorded.header("signature").map(|v| v.split("signature=").next().unwrap_or("").to_owned()),
            "signature_input_header": s.recorded.header("signature-input"),
            "content_digest": s.recorded.header("content-digest").is_some(),
            "protocol": s.body["protocol"],
        }),
    );
}
