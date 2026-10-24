//! Spike D, the real thing: Collabora Online loads a document from `WopiHost`
//! by WOPISrc, a user types, the document is saved, and the backing store holds
//! a new DCP version with the right parent whose ODF `content.xml` contains the
//! typed text.
//!
//! Collabora is driven over its COOL WebSocket protocol (`wsd/protocol.txt` in
//! the Collabora Online source): connect to `/cool/<encoded WOPISrc>/ws`, send
//! `load url=<encoded WOPISrc>`, `textinput`/`key`, `save`.
//!
//! This test needs a running Collabora (`scripts/collabora-up.sh`). It FAILS
//! when Collabora is not reachable. `DCP_SKIP_COLLABORA=1` skips it and the
//! test says so on stderr; nothing else skips it.
//!
//! Environment:
//!   DCP_COLLABORA_URL    default http://127.0.0.1:9980
//!   DCP_WOPI_HOST_PORT   port the WopiHost binds, default 18080
//!   DCP_WOPI_HOST_URL    how Collabora reaches the WopiHost,
//!                        default http://127.0.0.1:<port>
//!   DCP_WOPI_LOG_FILE    where to write the table of requests the host saw
//!   DCP_COLLABORA_WAIT_SECS  how long to wait for Collabora's discovery
//!                        document before failing (default 60)

mod support;
#[path = "shared/collabora_driver.rs"]
mod driver;

use std::sync::Arc;
use std::time::{Duration, Instant};

use dcp_core::*;
use dcp_wopi::{HostConfig, WopiHost};
use driver::*;
use support::*;

async fn start_host(e: &Env) -> (WopiHost, dcp_wopi::RunningHost) {
    let host = WopiHost::new(HostConfig { log_stderr: true, ..HostConfig::default() });
    let server = host
        .serve(format!("0.0.0.0:{}", e.host_port).parse().unwrap())
        .await
        .unwrap_or_else(|err| panic!("cannot bind the WOPI host on port {}: {err}", e.host_port));
    (host, server)
}

async fn wait_for_versions(owner: &Handle, doc: &str, n: usize, timeout: Duration) -> Vec<Stamped<Version>> {
    let end = Instant::now() + timeout;
    loop {
        let v = owner.versions(&doc.to_string()).await.unwrap();
        if v.len() >= n || Instant::now() > end {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn open_in_collabora(
    e: &Env,
    host: &WopiHost,
    rig_doc: &str,
    _owner: Arc<Handle>,
    backend: &Arc<Backend>,
    who: &str,
) -> Session {
    let token = host.register(rig_doc.to_string(), backend.handle(who), who);
    let src = wopi_src_with_token(e, rig_doc, &token);
    open_session(e, &src).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn collabora_edits_and_saves_a_new_dcp_version() {
    if skip("collabora_e2e") {
        return;
    }
    let e = env();
    require_collabora(&e).await;

    let backend = Backend::with_prefix("edit");
    let owner = backend.handle("@alice:test");
    let base = make_odt("base");
    let doc = owner
        .create_document(&meta("Spike D"), Some(NewFile { mime: ODT_MIME.into(), bytes: base.clone() }))
        .await
        .unwrap();
    let v1 = owner.versions(&doc).await.unwrap()[0].clone();
    let (host, server) = start_host(&e).await;

    let mut s = open_in_collabora(&e, &host, &doc, owner.clone(), &backend, "@alice:test").await;
    if !s.seen(|m| m == "perm: edit") {
        s.dump();
        panic!("Collabora did not grant `perm: edit`; it opened the document read-only");
    }

    type_text(&mut s, TYPED).await;
    s.send("save dontTerminateEdit=0 dontSaveIfUnmodified=0").await;

    let versions = wait_for_versions(&owner, &doc, 2, Duration::from_secs(90)).await;
    let log = host.request_log();
    write_request_log(&log);
    if versions.len() < 2 {
        s.dump();
    }
    assert!(versions.len() >= 2, "Collabora did not publish a new version within 90s (host saw {} requests)", log.len());

    // The new version: one child of v1, authored by the token's principal.
    let v2 = versions.iter().find(|v| v.id != v1.id).unwrap();
    assert_eq!(v2.body.parents, vec![v1.id.clone()], "the save must be a child of the version Collabora opened");
    assert_eq!(v2.body.created_by.principal, "@alice:test");
    let bytes = owner.fetch_file(&doc, v2).await.unwrap();
    assert_ne!(bytes, base, "the saved file is the unchanged base");
    let content = content_xml(&bytes);
    assert!(content.contains(TYPED), "typed text {TYPED:?} not in saved content.xml:\n{content}");
    eprintln!("saved content.xml contains {TYPED:?}; version {} parent {:?}", v2.id, v2.body.parents);

    s.dump();

    // What Collabora actually sent.
    let ops: Vec<_> = log.iter().map(|r| (r.method.as_str(), r.operation.as_deref().unwrap_or(""), r.path.as_str())).collect();
    assert!(ops.iter().any(|(m, o, p)| *m == "GET" && o.is_empty() && !p.ends_with("/contents")), "no CheckFileInfo: {ops:?}");
    assert!(ops.iter().any(|(m, _, p)| *m == "GET" && p.ends_with("/contents")), "no GetFile: {ops:?}");
    let lock = log.iter().find(|r| r.operation.as_deref() == Some("LOCK")).expect("Collabora never sent LOCK");
    let lock_string = lock.header("x-wopi-lock").expect("LOCK without X-WOPI-Lock").to_string();
    assert_eq!(lock.status, 200);
    let put = log.iter().find(|r| r.operation.as_deref() == Some("PUT")).expect("no PutFile");
    assert_eq!(put.status, 200);
    assert_eq!(put.header("x-wopi-lock"), Some(lock_string.as_str()), "PutFile must carry the lock string from LOCK");
    assert!(put.header("x-cool-wopi-timestamp").is_some(), "PutFile carried no X-COOL-WOPI-Timestamp");

    // While the document is open the lease is held with Collabora's lock string.
    let lease = owner.lease(&doc).await.unwrap().expect("a lease record");
    eprintln!("lease while open: {:?}", lease.body);
    assert_eq!(lease.body.state, LeaseState::Held);
    assert_eq!(lease.body.token.as_deref(), Some(lock_string.as_str()));
    assert_eq!(lease.body.duration_ms, Some(1_800_000));
    assert!(lease.body.renewal.is_some(), "a held lease carries renewal");
    let opened = owner
        .versions(&doc)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.body.parents.is_empty())
        .expect("the first version");
    assert_eq!(lease.body.base_version.as_deref(), Some(opened.id.as_str()), "base_version is the version Collabora opened");
    assert!(lease.body.holder.is_some());
    dcp_core::schema::validate_typed(dcp_core::schema::Kind::Lease, &lease.body).expect("lease validates");

    // Close the tab: Collabora unlocks when its last editing session
    // disconnects. What it sends is recorded for the results doc.
    s.close().await;
    let end = Instant::now() + Duration::from_secs(90);
    let mut unlock = None;
    while Instant::now() < end && unlock.is_none() {
        unlock = host.request_log().into_iter().find(|r| r.operation.as_deref() == Some("UNLOCK"));
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    write_request_log(&host.request_log());
    let unlock = unlock.expect("Collabora did not send UNLOCK within 90s of the websocket closing");
    assert_eq!(unlock.status, 200);
    assert_eq!(unlock.header("x-wopi-lock"), Some(lock_string.as_str()));
    let lease = owner.lease(&doc).await.unwrap().expect("a lease record");
    eprintln!("lease after UNLOCK: {:?}", lease.body);
    assert_eq!(lease.body.state, LeaseState::Released);
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn collabora_save_after_a_native_publish_is_refused_as_a_conflict() {
    if skip("collabora_e2e") {
        return;
    }
    let e = env();
    require_collabora(&e).await;

    let backend = Backend::with_prefix("conflict");
    let owner = backend.handle("@alice:test");
    let doc = owner
        .create_document(&meta("Spike D conflict"), Some(NewFile { mime: ODT_MIME.into(), bytes: make_odt("base") }))
        .await
        .unwrap();
    let v1 = owner.versions(&doc).await.unwrap()[0].clone();
    let (host, server) = start_host(&e).await;

    let mut s = open_in_collabora(&e, &host, &doc, owner.clone(), &backend, "@alice:test").await;

    // Another DCP writer (not through WOPI) publishes a child of v1 while
    // Collabora has v1 open and holds the lease. The lease is advisory (spec
    // 4.3 rule 6), so the store accepts it.
    let native = owner
        .publish_version(&doc, vec![v1.id.clone()], NewFile { mime: ODT_MIME.into(), bytes: make_odt("native edit") }, None)
        .await
        .unwrap();

    type_text(&mut s, TYPED).await;
    s.send("save dontTerminateEdit=0 dontSaveIfUnmodified=0").await;

    // Collabora sends its stored LastModifiedTime; the head moved, so the host
    // answers 409 + COOLStatusCode 1010 and publishes nothing.
    let end = Instant::now() + Duration::from_secs(60);
    let mut put = None;
    while Instant::now() < end && put.is_none() {
        put = host.request_log().into_iter().find(|r| r.operation.as_deref() == Some("PUT"));
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    write_request_log(&host.request_log());
    let put = put.expect("Collabora never attempted PutFile");
    assert_eq!(put.status, 409, "expected the timestamp conflict: {put:?}");
    assert!(put.response_body_preview.contains("1010"), "{put:?}");
    assert!(put.header("x-cool-wopi-timestamp").is_some());
    let versions = owner.versions(&doc).await.unwrap();
    assert_eq!(versions.len(), 2, "only the native version may exist: {versions:?}");
    assert!(versions.iter().any(|v| v.id == native.id));
    // Collabora tells the user instead of overwriting.
    s.wait_for("a conflict message to the user", Duration::from_secs(30), |m| m.contains("documentconflict") || m.contains("Conflict")).await;
    s.dump();
    s.close().await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    write_request_log(&host.request_log());
    server.shutdown().await;
}

/// Collabora refreshes its lock by sending `LOCK` again with the same string,
/// not `REFRESH_LOCK` (`DocumentBroker::refreshLock` calls
/// `updateStorageLockStateAsync(LOCK)`). `collabora-up.sh` sets
/// `storage.wopi.locking.refresh` to 10 s so this is observable; the default is
/// 900 s.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn collabora_refreshes_the_lease_with_the_same_lock_string() {
    if skip("collabora_e2e") {
        return;
    }
    let e = env();
    require_collabora(&e).await;

    let backend = Backend::with_prefix("refresh");
    let owner = backend.handle("@alice:test");
    let doc = owner
        .create_document(&meta("Spike D refresh"), Some(NewFile { mime: ODT_MIME.into(), bytes: make_odt("base") }))
        .await
        .unwrap();
    let (host, server) = start_host(&e).await;
    let s = open_in_collabora(&e, &host, &doc, owner.clone(), &backend, "@alice:test").await;

    let lock_count = |host: &WopiHost| host.request_log().iter().filter(|r| r.operation.as_deref() == Some("LOCK")).count();
    let first = owner.lease(&doc).await.unwrap().expect("lease after load");
    let end = Instant::now() + Duration::from_secs(90);
    while Instant::now() < end && lock_count(&host) < 2 {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let log = host.request_log();
    write_request_log(&log);
    let locks: Vec<_> = log.iter().filter(|r| r.operation.as_deref() == Some("LOCK")).collect();
    assert!(locks.len() >= 2, "no lock refresh within 90s ({} LOCK requests)", locks.len());
    assert_eq!(locks[0].header("x-wopi-lock"), locks[1].header("x-wopi-lock"), "a refresh uses the same lock string");
    assert_eq!(locks[1].status, 200);
    assert!(
        !log.iter().any(|r| matches!(r.operation.as_deref(), Some("REFRESH_LOCK" | "GET_LOCK" | "UNLOCK_AND_RELOCK"))),
        "Collabora was expected to use only LOCK, PUT and UNLOCK: {log:?}"
    );
    // The refresh moved the lease's binding timestamp (spec 4.3 rule 4) and,
    // being a write over a held record, raised its renewal (lease rule 8).
    let second = owner.lease(&doc).await.unwrap().unwrap();
    assert_eq!(second.body.token, first.body.token);
    assert!(second.ts_ms > first.ts_ms, "lease was not re-stamped: {} -> {}", first.ts_ms, second.ts_ms);
    let (r1, r2) = (first.body.renewal.expect("renewal before"), second.body.renewal.expect("renewal after"));
    assert!(r2 > r1, "renewal did not increase: {r1} -> {r2}");
    assert_eq!(second.body.duration_ms, Some(1_800_000));
    dcp_core::schema::validate_typed(dcp_core::schema::Kind::Lease, &second.body).expect("lease validates");
    s.close().await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    server.shutdown().await;
}

/// A viewer's session is read-only: Collabora never locks and never saves.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn collabora_opens_a_viewer_read_only_and_never_locks_or_saves() {
    if skip("collabora_e2e") {
        return;
    }
    let e = env();
    require_collabora(&e).await;

    let backend = Backend::with_prefix("viewer");
    let owner = backend.handle("@alice:test");
    let doc = owner
        .create_document(&meta("Spike D viewer"), Some(NewFile { mime: ODT_MIME.into(), bytes: make_odt("base") }))
        .await
        .unwrap();
    owner.grant(&doc, "@carol:test", Role::Viewer).await.unwrap();
    let (host, server) = start_host(&e).await;
    let mut s = open_in_collabora(&e, &host, &doc, owner.clone(), &backend, "@carol:test").await;
    s.dump();
    assert!(s.seen(|m| m.starts_with("perm: readonly") || m.starts_with("perm: view")), "expected a read-only permission message");
    assert!(s.seen(|m| m.starts_with("filemode:") && m.contains("\"readOnly\": true")), "expected filemode readOnly");

    // Typing and saving change nothing.
    s.send(&format!("textinput id=0 text={}", enc(TYPED))).await;
    s.send("save dontTerminateEdit=0 dontSaveIfUnmodified=0").await;
    tokio::time::sleep(Duration::from_secs(15)).await;
    let log = host.request_log();
    write_request_log(&log);
    assert!(!log.iter().any(|r| r.operation.is_some()), "a read-only session sent WOPI operations: {log:?}");
    assert_eq!(owner.versions(&doc).await.unwrap().len(), 1);
    assert!(owner.lease(&doc).await.unwrap().is_none(), "a read-only session must not take a lease");
    s.close().await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    server.shutdown().await;
}
