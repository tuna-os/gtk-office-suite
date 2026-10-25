//! Server restart and crash. These tests start their own reference server
//! (so they need `node` and the installed `scripts/streams-server`) and are
//! the evidence for "behaviour on server restart" in the results document.

use std::path::PathBuf;
use std::time::Instant;

use dcp_conformance::{new_file, standard_meta, ALGORITHM, BASE_FILE, DOC_MODEL};
use dcp_core::updates::StreamWriter;
use dcp_core::{DocumentStore, Lease, LeaseState, Role, StoreError};
use dcp_streams::http::{HttpError, JSON};

use crate::common::*;
use crate::require_streams;
use crate::server::*;

fn scratch(name: &str, port: u16) -> PathBuf {
    // One directory per server: two servers on one data directory corrupt it.
    let d = std::env::temp_dir().join(format!("dcp-streams-{name}-{}-{port}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[derive(Clone, Copy)]
enum Stop {
    Sigterm,
    Sigkill,
}

async fn file_backed_restart(stop: Stop) {
    let port = free_port();
    let dir = scratch("restart", port);
    let mut s1 = ServerProc::start(port, Some(dir.clone())).await;
    let base = s1.base();
    println!(
        "RESTART({}) first start ready in {:?}",
        match stop {
            Stop::Sigterm => "SIGTERM",
            Stop::Sigkill => "SIGKILL",
        },
        s1.started_in
    );

    let owner = store_at(&base, "OWNER");
    let editor = store_at(&base, "EDITOR");
    let doc = owner
        .create_document(&standard_meta(), Some(new_file(BASE_FILE)))
        .await
        .unwrap();
    owner
        .grant(&doc, &editor.principal(), Role::Editor)
        .await
        .unwrap();
    let first = owner.versions(&doc).await.unwrap()[0].id.clone();

    // 20 acknowledged updates, 3 versions and a lease; the owner has read them all.
    let mut w = StreamWriter::new(ALGORITHM, DOC_MODEL);
    for i in 0..20 {
        editor
            .publish_update(&doc, w.next(&first, format!("u{i}").as_bytes()).unwrap())
            .await
            .unwrap();
    }
    let mut parent = first.clone();
    for i in 0..3 {
        parent = editor
            .publish_version(
                &doc,
                vec![parent],
                new_file(format!("v{i}").as_bytes()),
                None,
            )
            .await
            .unwrap()
            .id;
    }
    let claim = Lease {
        dcp: 1,
        state: LeaseState::Held,
        holder: Some(dcp_core::Holder {
            principal: editor.principal(),
            session: "s1".into(),
        }),
        token: Some("t".into()),
        base_version: Some(parent.clone()),
        renewal: Some(0),
        duration_ms: Some(3_600_000),
    };
    let held = editor.write_lease(&doc, claim.clone()).await.unwrap();
    assert_eq!(owner.updates(&doc).await.unwrap().len(), 20);
    assert_eq!(owner.versions(&doc).await.unwrap().len(), 4);
    let reads_before = owner.http().snapshot();

    // Stop the server immediately after the last acknowledgement.
    match stop {
        Stop::Sigterm => s1.stop().await,
        Stop::Sigkill => s1.kill9().await,
    }
    // While it is down, the client reports a transport error, not an empty document.
    let down = owner.updates(&doc).await;
    assert!(matches!(down, Err(StoreError::Transport(_))), "{down:?}");
    let t0 = Instant::now();
    let mut s2 = ServerProc::start(port, Some(dir.clone())).await;
    println!(
        "RESTART second start ready in {:?} (process start to READY), {:?} wall incl. spawn",
        s2.started_in,
        t0.elapsed()
    );

    // Every acknowledged record survived, and the client resumes from the offset it kept.
    assert_eq!(owner.updates(&doc).await.unwrap().len(), 20);
    assert_eq!(owner.versions(&doc).await.unwrap().len(), 4);
    let resumed = owner.http().snapshot();
    println!(
        "RESTART resume: {} requests, {} bytes received to catch up after the restart (nothing new to read)",
        resumed.requests - reads_before.requests,
        resumed.bytes_received - reads_before.bytes_received
    );
    assert!(
        resumed.bytes_received - reads_before.bytes_received < 2_000,
        "the client re-read history instead of resuming from its offset"
    );

    // The writer carries on; the reader sees exactly the new records.
    for i in 20..25 {
        editor
            .publish_update(&doc, w.next(&first, format!("u{i}").as_bytes()).unwrap())
            .await
            .unwrap();
    }
    assert_eq!(owner.updates(&doc).await.unwrap().len(), 25);

    // Stream-Seq state survived: a duplicate sequence number is still refused,
    // and the lease continues from where it was.
    let lease_path = editor.stream_path(&doc, 1, "lease").await.unwrap();
    let dup = crate::common::http(&base)
        .append(
            &lease_path,
            JSON,
            b"{}".to_vec(),
            Some(&format!("{:016x}", 1)),
            false,
        )
        .await;
    assert!(
        matches!(dup, Err(HttpError::Conflict { closed: false })),
        "Stream-Seq state was lost: {dup:?}"
    );
    let mut renewal = claim.clone();
    renewal.renewal = Some(1);
    let renewed = editor.write_lease(&doc, renewal).await.unwrap();
    assert!(renewed.ts_ms >= held.ts_ms);
    assert_eq!(
        owner.lease(&doc).await.unwrap().unwrap().body.renewal,
        Some(1)
    );

    // A client with an empty cache reads all of it.
    let fresh = store_with(&base, "FRESH", owner.identity().clone());
    assert_eq!(fresh.updates(&doc).await.unwrap().len(), 25);
    assert_eq!(fresh.versions(&doc).await.unwrap().len(), 4);

    s2.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_backed_server_keeps_every_acknowledged_record_across_sigterm() {
    let _ = require_streams!();
    file_backed_restart(Stop::Sigterm).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_backed_server_keeps_every_acknowledged_record_across_sigkill() {
    let _ = require_streams!();
    file_backed_restart(Stop::Sigkill).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_in_memory_server_forgets_everything_and_the_client_says_so() {
    let _ = require_streams!();
    let port = free_port();
    let mut s1 = ServerProc::start(port, None).await;
    let base = s1.base();
    let owner = store_at(&base, "OWNER");
    let doc = owner
        .create_document(&standard_meta(), Some(new_file(BASE_FILE)))
        .await
        .unwrap();
    assert_eq!(owner.versions(&doc).await.unwrap().len(), 1);
    s1.stop().await;
    let mut s2 = ServerProc::start(port, None).await;
    // The document is gone. A client must not recreate it silently: it
    // reports NotFound for a document it knows it had.
    let r = owner.versions(&doc).await;
    assert!(matches!(r, Err(StoreError::NotFound(_))), "{r:?}");
    let r = owner
        .publish_version(&doc, vec![], new_file(b"x"), None)
        .await;
    assert!(matches!(r, Err(StoreError::NotFound(_))), "{r:?}");
    s2.stop().await;
}
