//! The smallest Open Cloud Mesh helper that lets a test see federation:
//!
//! - [`OcmReceiver`] is a **receiving server** with no file store at all. It
//!   serves a discovery document, accepts `POST /ocm/shares` and
//!   `POST /ocm/notifications`, and keeps every request exactly as it arrived
//!   (headers, signature, body). A Nextcloud sender shares to it; what it
//!   recorded is what crosses the wire.
//! - [`IncomingShare`] reads the share creation notification.
//! - [`accept_pending_remote_shares`] drives a Nextcloud **receiving** server's
//!   own OCS API to accept what arrived (two Nextcloud instances, no helper in
//!   the middle).
//!
//! There is no sender here: Nextcloud was the only sending server in the spike.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response, StatusCode};
use axum::routing::any;
use axum::Router;
use dcp_core::StoreError;
use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::dav::Dav;

/// One request the receiver saw.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    /// Header names lower-cased, values as sent.
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub at_ms: u64,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn json(&self) -> Option<Value> {
        serde_json::from_str(&self.body).ok()
    }
}

struct Inner {
    records: Mutex<Vec<Recorded>>,
    /// `host:port` as the senders must reach it.
    advertised: String,
    capabilities: Vec<String>,
}

pub struct OcmReceiver {
    inner: Arc<Inner>,
    pub addr: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for OcmReceiver {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl OcmReceiver {
    /// Listen on every interface, on a free port. `advertised_host` is the name the
    /// Nextcloud containers resolve to the runner (`dcp-host`).
    pub async fn start(advertised_host: &str) -> std::io::Result<OcmReceiver> {
        Self::start_advertising(advertised_host, &["notifications"]).await
    }

    /// As [`Self::start`], advertising these OCM `capabilities` in discovery.
    pub async fn start_advertising(advertised_host: &str, capabilities: &[&str]) -> std::io::Result<OcmReceiver> {
        let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await?;
        let addr = listener.local_addr()?;
        let inner = Arc::new(Inner {
            records: Mutex::new(Vec::new()),
            advertised: format!("{advertised_host}:{}", addr.port()),
            capabilities: capabilities.iter().map(|c| (*c).to_owned()).collect(),
        });
        let app = Router::new()
            .fallback(any(handle))
            .with_state(inner.clone());
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(OcmReceiver { inner, addr, task })
    }

    /// The receiver's OCM address host: `dcp-host:41234`.
    pub fn host(&self) -> String {
        self.inner.advertised.clone()
    }

    pub fn records(&self) -> Vec<Recorded> {
        self.inner.records.lock().clone()
    }

    /// Requests whose path ends with `suffix`, oldest first.
    pub fn records_for(&self, suffix: &str) -> Vec<Recorded> {
        self.records()
            .into_iter()
            .filter(|r| r.path.trim_end_matches('/').ends_with(suffix))
            .collect()
    }

    /// Share creation notifications received, parsed.
    pub fn shares(&self) -> Vec<IncomingShare> {
        self.records_for("/ocm/shares")
            .into_iter()
            .filter(|r| r.method == "POST")
            .filter_map(|r| r.json().map(|body| IncomingShare { body, recorded: r }))
            .collect()
    }

    /// The discovery document the receiver serves.
    pub fn discovery(&self) -> Value {
        discovery(&self.inner.advertised, &self.inner.capabilities)
    }
}

fn discovery(host: &str, capabilities: &[String]) -> Value {
    let mut d = json!({
        "enabled": true,
        "apiVersion": "1.1.0",
        "endPoint": format!("http://{host}/ocm"),
        "provider": "dcp-spike-receiver",
        "resourceTypes": [{
            "name": "file",
            "shareTypes": ["user"],
            "protocols": { "webdav": "/public.php/webdav/" }
        }],
        "capabilities": capabilities
    });
    if capabilities.iter().any(|c| c == "http-sig") {
        d["jwksUri"] = json!(format!("http://{host}/ocm/jwks"));
    }
    d
}

fn reply(status: StatusCode, body: Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("a static response builds")
}

async fn handle(State(s): State<Arc<Inner>>, req: Request<Body>) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap_or_default();
    let rec = Recorded {
        method: parts.method.to_string(),
        path: parts
            .uri
            .path_and_query()
            .map(|p| p.as_str().to_owned())
            .unwrap_or_default(),
        headers: parts
            .headers
            .iter()
            .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("<binary>").to_owned()))
            .collect(),
        body: String::from_utf8_lossy(&bytes).into_owned(),
        at_ms: now_ms(),
    };
    let path = parts.uri.path().trim_end_matches('/').to_owned();
    s.records.lock().push(rec);
    match (parts.method.as_str(), path.as_str()) {
        ("GET", "/.well-known/ocm") | ("GET", "/ocm-provider") => reply(StatusCode::OK, discovery(&s.advertised, &s.capabilities)),
        ("POST", p) if p.ends_with("/ocm/shares") => {
            reply(StatusCode::CREATED, json!({ "recipientDisplayName": "dcp-spike-receiver" }))
        }
        ("POST", p) if p.ends_with("/ocm/notifications") => reply(StatusCode::CREATED, json!({})),
        _ => reply(StatusCode::NOT_FOUND, json!({})),
    }
}

/// A share creation notification (OCM "Share Creation Notification"), as
/// Nextcloud 34.0.5 sent it.
#[derive(Debug, Clone)]
pub struct IncomingShare {
    pub body: Value,
    pub recorded: Recorded,
}

impl IncomingShare {
    fn s(&self, ptr: &str) -> Option<&str> {
        self.body.pointer(ptr).and_then(Value::as_str)
    }

    pub fn share_with(&self) -> Option<&str> {
        self.s("/shareWith")
    }
    pub fn name(&self) -> Option<&str> {
        self.s("/name")
    }
    pub fn provider_id(&self) -> Option<&str> {
        self.s("/providerId")
    }
    pub fn owner(&self) -> Option<&str> {
        self.s("/owner")
    }
    pub fn sender(&self) -> Option<&str> {
        self.s("/sender")
    }
    pub fn protocol_name(&self) -> Option<&str> {
        self.s("/protocol/name")
    }
    /// The secret a receiver presents to the sender's WebDAV endpoint.
    pub fn shared_secret(&self) -> Option<&str> {
        self.s("/protocol/options/sharedSecret")
            .or_else(|| self.s("/protocol/webdav/sharedSecret"))
    }
    /// The sender's base URL: `owner` is an address `alice@http://nc-a`.
    pub fn sender_base(&self) -> Option<String> {
        let owner = self.owner()?;
        let host = owner.rsplit_once('@')?.1;
        Some(if host.starts_with("http") { host.to_owned() } else { format!("http://{host}") })
    }
}

/// The pending remote shares of `dav`'s user on a Nextcloud receiving server:
/// `(id, name, owner)`.
pub async fn pending_remote_shares(dav: &Dav) -> Result<Vec<(String, String, String)>, StoreError> {
    let data = dav
        .ocs("GET", "apps/files_sharing/api/v1/remote_shares/pending", &[])
        .await?;
    Ok(data
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|s| {
            (
                match &s["id"] {
                    Value::String(x) => x.clone(),
                    other => other.to_string(),
                },
                s["name"].as_str().unwrap_or("").to_owned(),
                s["owner"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect())
}

/// Accept every pending remote share whose name starts with `name_prefix`.
/// Returns how many it accepted.
pub async fn accept_pending_remote_shares(dav: &Dav, name_prefix: &str) -> Result<usize, StoreError> {
    let mut n = 0;
    for (id, name, _) in pending_remote_shares(dav).await? {
        if name.trim_start_matches('/').starts_with(name_prefix) {
            dav.ocs("POST", &format!("apps/files_sharing/api/v1/remote_shares/pending/{id}"), &[])
                .await?;
            n += 1;
        }
    }
    Ok(n)
}
