//! The Collabora Online driver shared by `collabora_e2e.rs` (over the
//! in-memory store) and `dcp-interop`'s `collabora_over_matrix.rs` (over the
//! Matrix binding): environment, reachability check, and a COOL WebSocket
//! session (`wsd/protocol.txt` in the Collabora Online source).
//!
//! Environment:
//!   DCP_COLLABORA_URL    default http://127.0.0.1:9980
//!   DCP_WOPI_HOST_PORT   port the WopiHost binds, default 18080
//!   DCP_WOPI_HOST_URL    how Collabora reaches the WopiHost,
//!                        default http://127.0.0.1:<port>
//!   DCP_WOPI_LOG_FILE    where to write the table of requests the host saw
//!   DCP_COLLABORA_WAIT_SECS  how long to wait for Collabora's discovery
//!                        document before failing (default 60)
//!   DCP_SKIP_COLLABORA=1 skip (and say so)
#![allow(dead_code)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use dcp_wopi::{LoggedRequest, WopiHost};
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

pub const TYPED: &str = "DcpWopiSpike42";

pub fn skip(test: &str) -> bool {
    if std::env::var("DCP_SKIP_COLLABORA").as_deref() == Ok("1") {
        eprintln!("SKIPPED {test}: DCP_SKIP_COLLABORA=1 (no real Collabora exercised)");
        return true;
    }
    false
}

pub fn enc(s: &str) -> String {
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

pub struct Env {
    pub cool_url: String,
    pub host_port: u16,
    pub host_url: String,
}

pub fn env() -> Env {
    let host_port: u16 = std::env::var("DCP_WOPI_HOST_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(18080);
    Env {
        cool_url: std::env::var("DCP_COLLABORA_URL").unwrap_or_else(|_| "http://127.0.0.1:9980".into()),
        host_port,
        host_url: std::env::var("DCP_WOPI_HOST_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{host_port}")),
    }
}

/// Fail loudly if Collabora is not there. Never skips.
pub async fn require_collabora(e: &Env) {
    let url = format!("{}/hosting/discovery", e.cool_url);
    let mut last = String::new();
    let wait_secs: u64 = std::env::var("DCP_COLLABORA_WAIT_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
    for _ in 0..=(wait_secs / 2) {
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

pub type Messages = Arc<Mutex<Vec<(Instant, String)>>>;

pub struct Session {
    tx: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        Message,
    >,
    pub msgs: Messages,
}

impl Session {
    pub async fn connect(e: &Env, wopi_src_with_token: &str) -> Session {
        let encoded = enc(wopi_src_with_token);
        let ws_base = e.cool_url.replacen("http", "ws", 1);
        let url = format!("{ws_base}/cool/{encoded}/ws");
        eprintln!("COOL websocket: {}/cool/<encoded WOPISrc>/ws", ws_base);
        // Collabora (26.04) rejects a WebSocket upgrade that has no Origin
        // header ("Rejecting message with no Origin header"); the browser sends
        // its own origin, which is Collabora's web server URL.
        let mut request = url.as_str().into_client_request().expect("websocket request");
        request.headers_mut().insert("Origin", e.cool_url.parse().expect("origin header"));
        let (ws, resp) = tokio_tungstenite::connect_async(request)
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

    /// Close the websocket the way a browser tab closing does. Collabora
    /// unlocks the file when its last editing session disconnects.
    pub async fn close(mut self) {
        let _ = self.tx.send(Message::Close(None)).await;
        let _ = self.tx.close().await;
    }

    pub async fn send(&mut self, line: &str) {
        eprintln!("-> {line}");
        self.tx.send(Message::Text(line.to_string().into())).await.unwrap();
    }

    pub fn seen(&self, pred: impl Fn(&str) -> bool) -> bool {
        self.msgs.lock().iter().any(|(_, m)| pred(m))
    }

    pub async fn wait_for(&self, what: &str, timeout: Duration, pred: impl Fn(&str) -> bool) {
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

    pub fn dump(&self) {
        eprintln!("---- COOL messages received (first lines, tiles omitted) ----");
        for (_, m) in self.msgs.lock().iter() {
            if !m.starts_with("tile:") && !m.starts_with("delta:") && !m.starts_with("invalidatetiles") {
                eprintln!("<- {m}");
            }
        }
    }
}

pub fn write_request_log(log: &[LoggedRequest]) {
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

pub async fn type_text(s: &mut Session, text: &str) {
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


fn http() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().expect("reqwest client")
}

/// Connect to Collabora with a WOPISrc that carries the access token, wait for
/// the document to load, and say what permission it granted.
pub async fn open_session(e: &Env, wopi_src_with_token: &str) -> Session {
    let s = Session::connect(e, wopi_src_with_token).await;
    s.wait_for("the document to load (status:)", Duration::from_secs(120), |m| m.starts_with("status:")).await;
    eprintln!(
        "document loaded; permission messages: {:?}",
        s.msgs.lock().iter().filter(|(_, m)| m.starts_with("perm:")).map(|(_, m)| m.clone()).collect::<Vec<_>>()
    );
    s
}

/// `<WOPISrc>?access_token=<token>` as Collabora is launched with it.
pub fn wopi_src_with_token(e: &Env, doc: &str, token: &str) -> String {
    format!("{}?access_token={token}", WopiHost::wopi_src(&e.host_url, doc))
}
