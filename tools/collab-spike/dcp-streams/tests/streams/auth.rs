//! What a deployment can put in front of a Durable Streams server. The
//! reference servers have no access control of their own; this runs the Go
//! server behind Caddy's `basic_auth` (scripts/streams-up.sh caddy-auth):
//! `reader` may only GET and HEAD, `writer` may do anything.
//!
//! Needs `DCP_STREAMS_AUTH_URL`; fails without it unless `DCP_SKIP_STREAMS_AUTH=1`.

use std::sync::Arc;

use base64::Engine;
use dcp_conformance::{new_file, standard_meta, BASE_FILE};
use dcp_core::{DocumentStore, Role, StoreError};
use dcp_streams::http::{HttpError, JSON};
use dcp_streams::wire::Kind;
use dcp_streams::{Http, Identity, StreamsConfig, StreamsStore};

fn basic(user: &str, pass: &str) -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"))
    )
}

fn store(base: &str, auth: Option<&str>, device: &str) -> Arc<StreamsStore> {
    let mut http = Http::new(base);
    if let Some(a) = auth {
        http = http.with_header("Authorization", a);
    }
    Arc::new(StreamsStore::new(
        http,
        Identity::generate(),
        StreamsConfig::new(device),
    ))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_gates_by_credential_refuses_a_viewers_forgery_but_cannot_follow_revocation()
{
    let base = match std::env::var("DCP_STREAMS_AUTH_URL") {
        Ok(u) if !u.is_empty() => u,
        _ if std::env::var("DCP_SKIP_STREAMS_AUTH").as_deref() == Ok("1") => {
            eprintln!("SKIPPED auth (DCP_SKIP_STREAMS_AUTH=1): no server with access control");
            return;
        }
        _ => panic!("DCP_STREAMS_AUTH_URL is not set: run scripts/streams-up.sh caddy-auth, or set DCP_SKIP_STREAMS_AUTH=1 to skip explicitly"),
    };
    let writer = basic("writer", "writer-secret");
    let reader = basic("reader", "reader-secret");

    // No credential at all: nothing works, and the binding says Forbidden, not "empty".
    let anon = store(&base, None, "ANON");
    let r = anon
        .create_document(&standard_meta(), Some(new_file(BASE_FILE)))
        .await;
    assert!(
        matches!(
            r,
            Err(StoreError::Forbidden(_)) | Err(StoreError::Transport(_))
        ),
        "{r:?}"
    );
    assert!(matches!(
        Http::new(&base).head("dcp/x").await,
        Err(HttpError::Auth(401))
    ));

    let owner = store(&base, Some(&writer), "OWNER");
    let editor = store(&base, Some(&writer), "EDITOR");
    let viewer = store(&base, Some(&reader), "VIEWER");
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

    // The viewer reads with its reader credential...
    assert_eq!(viewer.versions(&doc).await.unwrap().len(), 1);
    // ...and a forgery it sends straight to the server is refused by the SERVER (401),
    // before any reader's rules are needed.
    let st = viewer.http().clock.estimate().unwrap_or(0);
    let forged = serde_json::json!({"st": st, "body": {
        "dcp": 1, "parents": [first],
        "file": {"mime": "application/x", "size": 1, "sha256": "0".repeat(64),
                 "locator": {"type": "durable-stream", "href": "https://x.invalid/dcp/d/f/z"}},
        "created_by": {"principal": viewer.principal(), "device": "V"}}});
    assert_eq!(
        viewer
            .send_unchecked(&doc, Kind::Ver, &forged)
            .await
            .unwrap(),
        None,
        "the server's gate refused it"
    );
    assert_eq!(owner.versions(&doc).await.unwrap().len(), 1);
    // The editor writes.
    let v2 = editor
        .publish_version(&doc, vec![first.clone()], new_file(b"v2"), None)
        .await
        .unwrap();
    assert_eq!(viewer.versions(&doc).await.unwrap().len(), 2);

    // Revocation is not something this gate can follow: the removed editor
    // keeps its `writer` credential, so the server still lets it append to
    // the NEW epoch's streams. What stops it there is the encryption and the
    // readers' drop rules, not the server.
    owner.revoke(&doc, &editor.principal()).await.unwrap();
    let r = editor
        .publish_version(&doc, vec![v2.id.clone()], new_file(b"x"), None)
        .await;
    assert!(
        matches!(r, Err(StoreError::Forbidden(_))),
        "the removed editor's own client refuses: {r:?}"
    );
    let n = {
        owner.sync(&doc).await.unwrap();
        owner.epochs(&doc).await.last().unwrap().0
    };
    let log = owner.stream_path(&doc, n, "log").await.unwrap();
    let junk = editor
        .http()
        .append(
            &log,
            JSON,
            br#"{"v":1,"k":"ver","e":3,"a":"x","n":"","c":"","s":""}"#.to_vec(),
            None,
            false,
        )
        .await;
    assert!(
        junk.is_ok(),
        "a shared writer credential cannot tell the removed editor from the owner: {junk:?}"
    );
    assert_eq!(
        owner.versions(&doc).await.unwrap().len(),
        2,
        "readers drop it"
    );
    // And the same credential, with the old key it still holds, can forge a
    // correctly signed record for the new epoch? No: it has no key for it.
    assert!(editor.held_keys(&doc).await.iter().all(|(e, _)| *e < n));
}
