//! Real Collabora Online in front of `WopiHost<MatrixStore>`: Collabora loads a
//! document from the WOPI host by WOPISrc, the host serves it from a Matrix
//! room on a real Synapse, and everything Collabora saves lands in that room
//! as a DCP version a native Matrix user reads.
//!
//! Needs BOTH services: a Synapse (`DCP_SYNAPSE_URL`, relaxed profile) and a
//! Collabora (`scripts/collabora-up.sh`, `DCP_COLLABORA_URL`). Each fails when
//! its service is missing; `DCP_SKIP_SYNAPSE=1` / `DCP_SKIP_COLLABORA=1` skip
//! and say so. Run with `--test-threads=1`: every test binds the same WOPI
//! host port, the one `collabora-up.sh` aliased.
//!
//! The driver is dcp-wopi's (`tests/shared/collabora_driver.rs`), the one that
//! ran against an in-memory store; only the store behind the host changed.

#[path = "../../dcp-wopi/tests/shared/collabora_driver.rs"]
mod driver;
#[path = "../../dcp-wopi/tests/shared/odf.rs"]
mod odf;

use std::time::{Duration, Instant};

use dcp_conformance::assert_same_view;
use dcp_core::schema::{self, Kind};
use dcp_core::{LeaseState, Stamped, Version};
use dcp_interop::rig::synapse;
use dcp_interop::{Rig, RigOptions, Who};
use driver::*;

fn base_odt() -> Vec<u8> {
    odf::make_odt("base")
}

/// `None` when a service is explicitly skipped.
async fn setup(test: &str) -> Option<(Rig, Env)> {
    if skip(test) {
        return None;
    }
    let syn = synapse()?;
    let e = env();
    require_collabora(&e).await;
    let rig = Rig::new(&syn, RigOptions::honest().for_collabora(e.host_port, base_odt)).await.expect("rig");
    Some((rig, e))
}

async fn open(rig: &Rig, e: &Env, who: Who) -> Session {
    let src = wopi_src_with_token(e, &rig.doc, rig.token(who));
    open_session(e, &src).await
}

async fn versions_at_least(rig: &Rig, n: usize, timeout: Duration) -> Vec<Stamped<Version>> {
    let end = Instant::now() + timeout;
    loop {
        let v = rig.native(Who::Owner).versions(&rig.doc).await.expect("versions");
        if v.len() >= n || Instant::now() > end {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn lock_ops(rig: &Rig) -> Vec<dcp_wopi::LoggedRequest> {
    rig.host.request_log().into_iter().filter(|r| r.operation.as_deref() == Some("LOCK")).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn collabora_over_matrix_edits_and_saves_a_version_a_matrix_user_reads() {
    let Some((rig, e)) = setup("collabora_over_matrix").await else { return };
    let owner = rig.native(Who::Owner);
    let mut s = open(&rig, &e, Who::Editor).await;
    if !s.seen(|m| m == "perm: edit") {
        s.dump();
        panic!("Collabora did not grant `perm: edit`; it opened the document read-only");
    }
    type_text(&mut s, TYPED).await;
    s.send("save dontTerminateEdit=0 dontSaveIfUnmodified=0").await;

    let versions = versions_at_least(&rig, 2, Duration::from_secs(90)).await;
    let log = rig.host.request_log();
    write_request_log(&log);
    if versions.len() < 2 {
        s.dump();
    }
    assert!(versions.len() >= 2, "Collabora did not publish a version into the Matrix room within 90 s ({} requests)", log.len());
    let v2 = versions.iter().find(|v| v.id != rig.first).unwrap();
    assert_eq!(v2.body.parents, vec![rig.first.clone()], "the save must be a child of the version Collabora opened");
    assert_eq!(v2.body.created_by.principal, rig.principal(Who::Editor));
    assert_eq!(v2.sender, rig.principal(Who::Editor), "the Matrix event's sender is the token's principal");
    assert!(v2.id.starts_with('$'), "a Matrix event id: {}", v2.id);
    let bytes = owner.fetch_file(&rig.doc, v2).await.unwrap();
    let content = odf::content_xml(&bytes);
    assert!(content.contains(TYPED), "typed text {TYPED:?} not in the saved content.xml:\n{content}");

    // A different Matrix principal reads the same thing, natively and through
    // the host.
    rig.wait_head(Who::Viewer, &v2.id).await.expect("the Matrix reader sees Collabora's save as the only head");
    assert_same_view(&**rig.native(Who::Viewer), &**owner, &rig.doc).await.unwrap();
    assert_same_view(&**rig.native(Who::Viewer), &rig.sim(Who::Viewer), &rig.doc).await.unwrap();

    // While open, the Matrix lease is Collabora's lock, per the rules of 4.3 and 8.2.
    let lock_string = lock_ops(&rig)[0].header("x-wopi-lock").expect("LOCK without X-WOPI-Lock").to_string();
    let lease = owner.lease(&rig.doc).await.unwrap().expect("a lease record in the Matrix room");
    eprintln!("lease while open: {:?}", lease.body);
    assert_eq!(lease.body.state, LeaseState::Held);
    assert_eq!(lease.body.token.as_deref(), Some(lock_string.as_str()));
    assert_eq!(lease.body.duration_ms, Some(1_800_000));
    assert!(lease.body.renewal.is_some(), "a held lease carries renewal (rule 8)");
    assert_eq!(lease.body.holder.as_ref().map(|h| h.principal.clone()), Some(rig.principal(Who::Editor)));
    schema::validate_typed(Kind::Lease, &lease.body).expect("the lease Collabora's host wrote validates");

    // Closing the tab: Collabora sends UNLOCK when its last session goes.
    s.close().await;
    let end = Instant::now() + Duration::from_secs(90);
    let mut unlock = None;
    while Instant::now() < end && unlock.is_none() {
        unlock = rig.host.request_log().into_iter().find(|r| r.operation.as_deref() == Some("UNLOCK"));
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    write_request_log(&rig.host.request_log());
    let unlock = unlock.expect("Collabora did not send UNLOCK within 90 s of the websocket closing");
    assert_eq!(unlock.status, 200);
    let lease = owner.lease(&rig.doc).await.unwrap().expect("a lease record");
    assert_eq!(lease.body.state, LeaseState::Released, "UNLOCK must release the lease in the Matrix room");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn collabora_over_matrix_save_after_a_native_matrix_publish_is_a_conflict() {
    let Some((rig, e)) = setup("collabora_over_matrix").await else { return };
    let owner = rig.native(Who::Owner);
    let mut s = open(&rig, &e, Who::Editor).await;

    // A native Matrix user publishes a child of the version Collabora has open
    // while Collabora holds the lease (advisory: lease rule 6).
    let native = owner
        .publish_version(
            &rig.doc,
            vec![rig.first.clone()],
            dcp_core::NewFile { mime: dcp_conformance::MIME.into(), bytes: odf::make_odt("native edit") },
            None,
        )
        .await
        .unwrap();
    // The host must have seen it, or Collabora's save is judged against a stale head.
    rig.until("the host to see the native version", || async {
        let info = rig.raw(Who::Editor).check_file_info().await?.json()?;
        if info["Version"] == native.id.as_str() {
            return Ok(Ok(()));
        }
        Ok(Err(format!("Version {}", info["Version"])))
    })
    .await
    .unwrap();

    type_text(&mut s, TYPED).await;
    s.send("save dontTerminateEdit=0 dontSaveIfUnmodified=0").await;
    let end = Instant::now() + Duration::from_secs(60);
    let mut put = None;
    while Instant::now() < end && put.is_none() {
        put = rig.host.request_log().into_iter().find(|r| r.operation.as_deref() == Some("PUT"));
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    write_request_log(&rig.host.request_log());
    let put = put.expect("Collabora never attempted PutFile");
    assert_eq!(put.status, 409, "expected the timestamp conflict: {put:?}");
    assert!(put.response_body_preview.contains("1010"), "{put:?}");
    assert!(put.response_lock.is_none(), "a changed-document 409 carries no X-WOPI-Lock: {put:?}");
    let ids = rig.version_ids(&**owner).await.unwrap();
    let mut want = vec![rig.first.clone(), native.id.clone()];
    want.sort();
    assert_eq!(ids, want, "only the native version may exist besides the first");
    s.wait_for("a conflict message to the user", Duration::from_secs(30), |m| m.contains("documentconflict") || m.contains("Conflict")).await;
    s.close().await;
    tokio::time::sleep(Duration::from_secs(5)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn collabora_over_matrix_opens_a_viewer_read_only_and_never_locks_or_saves() {
    let Some((rig, e)) = setup("collabora_over_matrix").await else { return };
    let mut s = open(&rig, &e, Who::Viewer).await;
    s.dump();
    assert!(s.seen(|m| m.starts_with("perm: readonly") || m.starts_with("perm: view")), "expected a read-only permission message");
    s.send(&format!("textinput id=0 text={}", enc(TYPED))).await;
    s.send("save dontTerminateEdit=0 dontSaveIfUnmodified=0").await;
    tokio::time::sleep(Duration::from_secs(15)).await;
    let log = rig.host.request_log();
    write_request_log(&log);
    assert!(!log.iter().any(|r| r.operation.is_some()), "a read-only session sent WOPI operations: {log:?}");
    assert_eq!(rig.native(Who::Owner).versions(&rig.doc).await.unwrap().len(), 1);
    assert!(rig.native(Who::Owner).lease(&rig.doc).await.unwrap().is_none(), "a read-only session must not take a lease");
    s.close().await;
    tokio::time::sleep(Duration::from_secs(3)).await;
}

/// Collabora refreshes its lock with `LOCK` and the same string
/// (`collabora-up.sh` sets the period to 10 s). On Matrix that would be a state
/// event identical to the host's own current one, which Synapse silently drops;
/// the host writes `renewal` + 1, so the refresh is a new event with a later
/// timestamp (lease rule 8). This is the real-Collabora, real-Synapse form of
/// `wopi_lock_refresh_is_a_new_record_on_matrix`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn collabora_over_matrix_refresh_is_a_new_lease_record_with_renewal_plus_one() {
    let Some((rig, e)) = setup("collabora_over_matrix").await else { return };
    let owner = rig.native(Who::Owner);
    let s = open(&rig, &e, Who::Editor).await;

    let first = owner.lease(&rig.doc).await.unwrap().expect("lease after load");
    assert_eq!(first.body.renewal, Some(0), "the first LOCK over no record: renewal 0");
    let end = Instant::now() + Duration::from_secs(90);
    while Instant::now() < end && lock_ops(&rig).len() < 2 {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let locks = lock_ops(&rig);
    write_request_log(&rig.host.request_log());
    assert!(locks.len() >= 2, "no lock refresh within 90 s ({} LOCK requests)", locks.len());
    assert_eq!(locks[0].header("x-wopi-lock"), locks[1].header("x-wopi-lock"), "a refresh uses the same lock string");
    assert_eq!(locks[1].status, 200);

    let second = owner.lease(&rig.doc).await.unwrap().unwrap();
    assert_eq!(second.body.token, first.body.token);
    assert_eq!(second.body.renewal, Some(1), "the refresh is a write over a held record: renewal 0 + 1");
    assert_ne!(second.id, first.id, "Synapse must have stored a NEW event for the refresh");
    assert!(second.ts_ms > first.ts_ms, "the lease was not re-stamped: {} -> {}", first.ts_ms, second.ts_ms);
    s.close().await;
    tokio::time::sleep(Duration::from_secs(5)).await;
}
