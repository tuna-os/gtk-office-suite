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

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use dcp_core::*;
use dcp_wopi::{HostConfig, LoggedRequest, WopiHost};
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use support::*;
use tokio_tungstenite::tungstenite::Message;

const TYPED: &str = "DcpWopiSpike42";

fn skip() -> bool {
    if std::env::var("DCP_SKIP_COLLABORA").as_deref() == Ok("1") {
        eprintln!("SKIPPED collabora_e2e: DCP_SKIP_COLLABORA=1 (no real Collabora exercised)");
        return true;
    }
    false
}

fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

struct Env {
    cool_url: String,
    host_port: u16,
    host_url: String,
}

fn env() -> Env {
    let host_port: u16 = std::env::var("DCP_WOPI_HOST_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(18080);
    Env {
        cool_url: std::env::var("DCP_COLLABORA_URL").unwrap_or_else(|_| "http://127.0.0.1:9980".into()),
        host_port,
        host_url: std::env::var("DCP_WOPI_HOST_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{host_port}")),
    }
}

/// Fail loudly if Collabora is not there. Never skips.
async fn require_collabora(e: &Env) {
    let url = format!("{}/hosting/discovery", e.cool_url);
    let mut last = String::new();
    for _ in 0..30 {
        match http().get(&url).send().await {
            Ok(r) if r.status().is_success() => {
                let body = r.text().await.unwrap_or_default();
                assert!(body.contains("wopi-discovery"), "{url} answered but is not WOPI discovery: {}", &body[..body.len().min(200)]);
                eprintln!("collabora discovery ok at {url} ({} bytes)", body.len());
                return;
            }
            Ok(r) => last = format!("HTTP {}", r.status()),
            Err(err) => last = err.to_string(),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    panic!(
        "Collabora Online is not reachable at {url} ({last}). Start it with tools/collab-spike/scripts/collabora-up.sh, \
         or set DCP_SKIP_COLLABORA=1 to skip this test explicitly."
    );
}

type Messages = Arc<Mutex<Vec<(Instant, String)>>>;

struct Session {
    tx: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        Message,
    >,
    msgs: Messages,
}

impl Session {
    async fn connect(e: &Env, wopi_src_with_token: &str) -> Session {
        let encoded = enc(wopi_src_with_token);
        let ws_base = e.cool_url.replacen("http", "ws", 1);
        let url = format!("{ws_base}/cool/{encoded}/ws");
        eprintln!("COOL websocket: {}/cool/<encoded WOPISrc>/ws", ws_base);
        let (ws, resp) = tokio_tungstenite::connect_async(url.as_str())
            .await
            .unwrap_or_else(|err| panic!("COOL websocket connect failed: {err}"));
        eprintln!("COOL websocket upgraded: {}", resp.status());
        let (mut tx, mut rx) = ws.split();
        let msgs: Messages = Arc::new(Mutex::new(Vec::new()));
        let sink = msgs.clone();
        tokio::spawn(async move {
            while let Some(Ok(m)) = rx.next().await {
                let first_line = match m {
                    Message::Text(t) => t.as_str().lines().next().unwrap_or("").chars().take(300).collect::<String>(),
                    Message::Binary(b) => {
                        let head = &b[..b.len().min(300)];
                        let end = head.iter().position(|c| *c == b'\n').unwrap_or(head.len());
                        String::from_utf8_lossy(&head[..end]).into_owned()
                    }
                    Message::Close(f) => format!("<websocket close {f:?}>"),
                    _ => continue,
                };
                sink.lock().push((Instant::now(), first_line));
            }
        });
        tx.send(Message::Text(format!("load url={encoded}").into())).await.unwrap();
        Session { tx, msgs }
    }

    async fn send(&mut self, line: &str) {
        eprintln!("-> {line}");
        self.tx.send(Message::Text(line.to_string().into())).await.unwrap();
    }

    fn seen(&self, pred: impl Fn(&str) -> bool) -> bool {
        self.msgs.lock().iter().any(|(_, m)| pred(m))
    }

    async fn wait_for(&self, what: &str, timeout: Duration, pred: impl Fn(&str) -> bool) {
        let end = Instant::now() + timeout;
        while Instant::now() < end {
            if self.seen(&pred) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        self.dump();
        panic!("timed out after {timeout:?} waiting for {what}");
    }

    fn dump(&self) {
        eprintln!("---- COOL messages received (first lines, tiles omitted) ----");
        for (_, m) in self.msgs.lock().iter() {
            if !m.starts_with("tile:") && !m.starts_with("delta:") && !m.starts_with("invalidatetiles") {
                eprintln!("<- {m}");
            }
        }
    }
}

async fn start_host(e: &Env) -> (WopiHost, dcp_wopi::RunningHost) {
    let host = WopiHost::new(HostConfig { log_stderr: true, ..HostConfig::default() });
    let server = host
        .serve(format!("0.0.0.0:{}", e.host_port).parse().unwrap())
        .await
        .unwrap_or_else(|err| panic!("cannot bind the WOPI host on port {}: {err}", e.host_port));
    (host, server)
}

fn write_request_log(log: &[LoggedRequest]) {
    let mut out = String::from("seq method path op status resp_lock | request headers\n");
    for r in log {
        out.push_str(&format!(
            "#{} {} {} op={} -> {} resp_lock={:?} | {:?} {}\n",
            r.seq,
            r.method,
            r.path,
            r.operation.as_deref().unwrap_or("-"),
            r.status,
            r.response_lock,
            r.headers,
            r.response_body_preview,
        ));
    }
    eprintln!("==== WOPI requests Collabora sent to the host ====\n{out}");
    if let Ok(p) = std::env::var("DCP_WOPI_LOG_FILE") {
        let _ = std::fs::write(p, out);
    }
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

async fn type_text(s: &mut Session, text: &str) {
    // `textinput id=0 text=<uri-encoded>` inserts at the caret (protocol.txt).
    s.send(&format!("textinput id=0 text={}", enc(text))).await;
    let modified = |m: &str| m.contains(".uno:ModifiedStatus=true");
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        if s.seen(modified) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    // Fall back to key events, one per character.
    eprintln!("no ModifiedStatus after textinput; typing with key events instead");
    for ch in text.chars() {
        let c = ch as u32;
        s.send(&format!("key type=input char={c} key={c}")).await;
        s.send(&format!("key type=up char={c} key={c}")).await;
    }
    s.wait_for("ModifiedStatus=true after typing", Duration::from_secs(30), modified).await;
}

async fn open_in_collabora(
    e: &Env,
    host: &WopiHost,
    rig_doc: &str,
    owner: Arc<Handle>,
    backend: &Arc<Backend>,
    who: &str,
) -> Session {
    let token = host.register(rig_doc.to_string(), backend.handle(who), who);
    let _ = owner;
    let src = format!("{}?access_token={token}", WopiHost::wopi_src(&e.host_url, rig_doc));
    let s = Session::connect(e, &src).await;
    s.wait_for("the document to load (status:)", Duration::from_secs(180), |m| m.starts_with("status:")).await;
    eprintln!("document loaded; permission messages: {:?}", s.msgs.lock().iter().filter(|(_, m)| m.starts_with("perm:")).map(|(_, m)| m.clone()).collect::<Vec<_>>());
    s
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn collabora_edits_and_saves_a_new_dcp_version() {
    if skip() {
        return;
    }
    let e = env();
    require_collabora(&e).await;

    let backend = Backend::new();
    let owner = backend.handle("@alice:test");
    let base = make_odt("base");
    let doc = owner
        .create_document(&meta("Spike D"), Some(NewFile { mime: ODT_MIME.into(), bytes: base.clone() }))
        .await
        .unwrap();
    let v1 = owner.versions(&doc).await.unwrap()[0].clone();
    let (host, server) = start_host(&e).await;

    let mut s = open_in_collabora(&e, &host, &doc, owner.clone(), &backend, "@alice:test").await;
    assert!(s.seen(|m| m.starts_with("perm: edit") || m.contains("\"perm\":\"edit\"") || m.starts_with("perm:")), "no perm message; Collabora may have opened read-only");

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

    // Close and see whether Collabora unlocks (observed and printed; the
    // results doc reports it, the assertion is only that the log is complete).
    s.send("closedocument").await;
    drop(s);
    let end = Instant::now() + Duration::from_secs(60);
    let mut unlocked = false;
    while Instant::now() < end {
        if host.request_log().iter().any(|r| r.operation.as_deref() == Some("UNLOCK")) {
            unlocked = true;
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let final_log = host.request_log();
    write_request_log(&final_log);
    eprintln!("Collabora sent UNLOCK after closedocument within 60s: {unlocked}");
    eprintln!("lease after close: {:?}", owner.lease(&doc).await.unwrap().map(|l| l.body));
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn collabora_save_after_a_native_publish_is_refused_as_a_conflict() {
    if skip() {
        return;
    }
    let e = env();
    require_collabora(&e).await;

    let backend = Backend::new();
    let owner = backend.handle("@alice:test");
    backend.handle("@alice:test");
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
    server.shutdown().await;
}
