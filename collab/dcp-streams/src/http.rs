//! The Durable Streams HTTP protocol, as far as this binding uses it:
//! create (`PUT`), append (`POST`), close, catch-up and long-poll reads
//! (`GET`), `HEAD`, `DELETE`. Offsets are opaque strings handed back by the
//! server and handed back to it; nothing here interprets one.
//!
//! JSON mode (`Content-Type: application/json`) is used for the record
//! streams, so the server keeps message boundaries. File streams are
//! `application/octet-stream`, created closed in one request.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::{header, Method, StatusCode};
use serde_json::Value;

use crate::clock::ServerClock;

pub const JSON: &str = "application/json";
pub const OCTETS: &str = "application/octet-stream";
/// The offset that means "from the start".
pub const START: &str = "-1";

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("not found")]
    NotFound,
    /// `409`: the stream is closed, the sequence regressed, the content type
    /// differs, or the stream exists with another configuration.
    #[error("conflict (stream closed: {closed})")]
    Conflict { closed: bool },
    #[error("gone (410)")]
    Gone,
    #[error("payload too large (413)")]
    TooLarge,
    #[error("refused by the server's access control ({0})")]
    Auth(u16),
    #[error("http {0}: {1}")]
    Status(u16, String),
    #[error("transport: {0}")]
    Transport(String),
    #[error("the server sent something that is not the protocol: {0}")]
    Protocol(String),
}

/// What one read returned.
#[derive(Debug, Clone)]
pub struct ReadResp {
    /// JSON mode: the messages. Octet mode: empty (see `body`).
    pub items: Vec<Value>,
    pub body: Vec<u8>,
    /// `Stream-Next-Offset`: where to read next.
    pub next: String,
    /// `Stream-Up-To-Date`.
    pub up_to_date: bool,
    /// `Stream-Closed`.
    pub closed: bool,
}

#[derive(Debug, Clone)]
pub struct Head {
    pub next: String,
    pub closed: bool,
}

#[derive(Debug, Clone)]
pub struct Appended {
    pub next: String,
    pub closed: bool,
}

#[derive(Default, Debug)]
pub struct Stats {
    pub requests: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    /// Responses that arrived as HTTP/1.1.
    pub responses_h1: AtomicU64,
    /// Responses that arrived as HTTP/2.
    pub responses_h2: AtomicU64,
    /// Attempts that failed below HTTP (connect, TLS, reset, timeout).
    pub transport_errors: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatsSnapshot {
    pub requests: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub responses_h1: u64,
    pub responses_h2: u64,
    pub transport_errors: u64,
}

/// Which HTTP version a client insists on. With `Some`, the client offers only that
/// version in ALPN (TLS) and refuses a response of any other version, so a
/// downgrade cannot go unnoticed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpVersion {
    Http1,
    Http2,
}

impl HttpVersion {
    fn matches(self, v: reqwest::Version) -> bool {
        match self {
            HttpVersion::Http1 => v == reqwest::Version::HTTP_11,
            HttpVersion::Http2 => v == reqwest::Version::HTTP_2,
        }
    }
}

/// How the client reaches the server: what it trusts, which HTTP version it requires.
///
/// There are **no built-in trust roots** (no `webpki-roots`, no platform store): a
/// `https://` server is trusted only if its chain ends at one of `ca_pem`. With no
/// CA every `https://` connection fails (fail closed). Redirects are never followed.
#[derive(Clone, Debug)]
pub struct Transport {
    /// PEM of the CA certificate(s) that may sign the server's certificate.
    pub ca_pem: Option<Vec<u8>>,
    /// `Some`: offer and accept only this version. `None`: ALPN offers `h2`, `http/1.1`.
    pub version: Option<HttpVersion>,
    /// Refuse any request to a base URL that is not `https://`.
    pub require_tls: bool,
    /// How long an idle pooled connection is kept.
    pub pool_idle: Duration,
    /// Connect `host` to this address whatever DNS says (the name is still what the
    /// certificate is checked against and what is sent as SNI).
    pub resolve: Vec<(String, std::net::SocketAddr)>,
    /// HTTP/2: send PING frames this often, also while idle, and drop the connection if one
    /// is not answered within 10 s. `None`: none are sent (reqwest's default).
    pub h2_keep_alive: Option<Duration>,
    /// HTTP/2: let the receive windows grow with the bandwidth-delay product. `false` is
    /// reqwest's default, a fixed 64 KiB per stream.
    pub h2_adaptive_window: bool,
}

impl Default for Transport {
    fn default() -> Self {
        Transport {
            ca_pem: None,
            version: None,
            require_tls: false,
            // The reference server closes idle keep-alive connections after 5 s;
            // drop ours first so that an append never goes down a dead socket.
            pool_idle: Duration::from_secs(2),
            resolve: Vec::new(),
            h2_keep_alive: None,
            h2_adaptive_window: false,
        }
    }
}

#[derive(Clone)]
pub struct Http {
    client: reqwest::Client,
    base: String,
    require_tls: bool,
    required: Option<HttpVersion>,
    headers: Vec<(String, String)>,
    pub clock: Arc<ServerClock>,
    pub stats: Arc<Stats>,
}

/// The error and every error under it: reqwest's own text is only "error sending
/// request", the certificate verdict is two sources down.
fn chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut cur = e.source();
    while let Some(c) = cur {
        s.push_str(": ");
        s.push_str(&c.to_string());
        cur = c.source();
    }
    s
}

fn transport(e: reqwest::Error) -> HttpError {
    HttpError::Transport(chain(&e))
}

impl Http {
    /// `base` is the URL under which stream paths are appended, for the
    /// reference server `http://127.0.0.1:4437/v1/stream/`. Plain `http://` is
    /// allowed (a local server); an `https://` base needs [`Http::with_transport`]
    /// to say which CA to trust, otherwise every request fails closed.
    pub fn new(base: &str) -> Self {
        Self::with_transport(base, Transport::default())
    }

    pub fn with_transport(base: &str, t: Transport) -> Self {
        let mut b = reqwest::Client::builder()
            .use_rustls_tls()
            .tls_built_in_root_certs(false)
            .https_only(base.starts_with("https://"))
            // A redirect is a server telling the client to go elsewhere, possibly to a
            // plain-text URL; a stream binding has no use for one.
            .redirect(reqwest::redirect::Policy::none())
            .pool_idle_timeout(t.pool_idle)
            .tcp_nodelay(true)
            .connect_timeout(Duration::from_secs(10));
        if let Some(pem) = &t.ca_pem {
            let cert = reqwest::Certificate::from_pem(pem).expect("the CA is not a PEM certificate");
            b = b.add_root_certificate(cert);
        }
        for (name, addr) in &t.resolve {
            b = b.resolve(name, *addr);
        }
        if let Some(every) = t.h2_keep_alive {
            b = b
                .http2_keep_alive_interval(every)
                .http2_keep_alive_timeout(Duration::from_secs(10))
                .http2_keep_alive_while_idle(true);
        }
        if t.h2_adaptive_window {
            b = b.http2_adaptive_window(true);
        }
        b = match t.version {
            Some(HttpVersion::Http1) => b.http1_only(),
            Some(HttpVersion::Http2) => b.http2_prior_knowledge(),
            None => b,
        };
        let client = b.build().expect("reqwest client");
        let mut base = base.to_owned();
        if !base.ends_with('/') {
            base.push('/');
        }
        Http {
            client,
            base,
            require_tls: t.require_tls,
            required: t.version,
            headers: Vec::new(),
            clock: Arc::new(ServerClock::default()),
            stats: Arc::new(Stats::default()),
        }
    }

    /// The underlying client, for requests the binding does not make (SSE, raw
    /// long polls in the transport tests). It shares the connection pool.
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Send this header on every request (an `Authorization` token for a
    /// server that has access control in front of it).
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    /// A handle with its own clock and counters, sharing the connection pool.
    pub fn fork(&self) -> Self {
        Http {
            clock: Arc::new(ServerClock::default()),
            stats: Arc::new(Stats::default()),
            ..self.clone()
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path.trim_start_matches('/'))
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            requests: self.stats.requests.load(Ordering::Relaxed),
            bytes_sent: self.stats.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.stats.bytes_received.load(Ordering::Relaxed),
            responses_h1: self.stats.responses_h1.load(Ordering::Relaxed),
            responses_h2: self.stats.responses_h2.load(Ordering::Relaxed),
            transport_errors: self.stats.transport_errors.load(Ordering::Relaxed),
        }
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        extra: &[(&str, String)],
        body: Option<Vec<u8>>,
        timeout: Option<Duration>,
    ) -> Result<reqwest::Response, HttpError> {
        if self.require_tls && !self.base.starts_with("https://") {
            return Err(HttpError::Protocol(format!(
                "this client requires TLS and the base URL is not https: {}",
                self.base
            )));
        }
        let mut last = HttpError::Transport("no attempt made".into());
        for attempt in 0..3 {
            let mut rb = self.client.request(method.clone(), self.url(path));
            if !query.is_empty() {
                rb = rb.query(query);
            }
            for (k, v) in &self.headers {
                rb = rb.header(k.as_str(), v.as_str());
            }
            for (k, v) in extra {
                rb = rb.header(*k, v.as_str());
            }
            if let Some(t) = timeout {
                rb = rb.timeout(t);
            }
            let sent = body.as_ref().map_or(0, |b| b.len()) as u64;
            if let Some(b) = &body {
                rb = rb.body(b.clone());
            }
            self.stats.requests.fetch_add(1, Ordering::Relaxed);
            self.stats.bytes_sent.fetch_add(sent, Ordering::Relaxed);
            match rb.send().await {
                Ok(resp) => {
                    match resp.version() {
                        reqwest::Version::HTTP_2 => {
                            self.stats.responses_h2.fetch_add(1, Ordering::Relaxed)
                        }
                        _ => self.stats.responses_h1.fetch_add(1, Ordering::Relaxed),
                    };
                    // Never accept a response of another HTTP version than the one
                    // under test: with `required` set the ALPN offer already
                    // excludes it, this is the second check.
                    if let Some(want) = self.required {
                        if !want.matches(resp.version()) {
                            return Err(HttpError::Protocol(format!(
                                "required {want:?}, the server answered {:?}",
                                resp.version()
                            )));
                        }
                    }
                    if let Some(d) = resp
                        .headers()
                        .get(header::DATE)
                        .and_then(|v| v.to_str().ok())
                    {
                        self.clock.observe_http_date(d, Instant::now());
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    self.stats.transport_errors.fetch_add(1, Ordering::Relaxed);
                    last = transport(e);
                    // Reads, creates and deletes are idempotent in the
                    // protocol. An append that is retried after a lost reply
                    // may be stored twice; every record is content-addressed
                    // and readers drop the second copy.
                    if attempt < 2 {
                        tokio::time::sleep(Duration::from_millis(50 * (attempt + 1))).await;
                        continue;
                    }
                }
            }
        }
        Err(last)
    }

    fn classify(status: StatusCode, resp_headers: &header::HeaderMap, text: String) -> HttpError {
        match status.as_u16() {
            404 => HttpError::NotFound,
            409 => HttpError::Conflict {
                closed: resp_headers
                    .get("stream-closed")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|v| v.eq_ignore_ascii_case("true")),
            },
            410 => HttpError::Gone,
            413 => HttpError::TooLarge,
            401 | 403 => HttpError::Auth(status.as_u16()),
            s => HttpError::Status(s, text),
        }
    }

    async fn fail(&self, resp: reqwest::Response) -> HttpError {
        let status = resp.status();
        let headers = resp.headers().clone();
        let text = resp.text().await.unwrap_or_default();
        Self::classify(status, &headers, text)
    }

    fn hdr(resp: &reqwest::Response, name: &str) -> Option<String> {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    }

    fn flag(resp: &reqwest::Response, name: &str) -> bool {
        Self::hdr(resp, name).is_some_and(|v| v.eq_ignore_ascii_case("true"))
    }

    /// `PUT`: create a stream, optionally with initial content, optionally
    /// closed in the same request. Returns `(created, next_offset)`;
    /// `created` is false when the stream already existed with this configuration.
    pub async fn create(
        &self,
        path: &str,
        content_type: &str,
        body: Option<Vec<u8>>,
        closed: bool,
    ) -> Result<(bool, String), HttpError> {
        let mut extra = vec![("content-type", content_type.to_owned())];
        if closed {
            extra.push(("stream-closed", "true".into()));
        }
        let resp = self
            .send(
                Method::PUT,
                path,
                &[],
                &extra,
                body,
                Some(Duration::from_secs(300)),
            )
            .await?;
        let status = resp.status();
        if status == StatusCode::CREATED || status == StatusCode::OK {
            let next = Self::hdr(&resp, "stream-next-offset").unwrap_or_default();
            return Ok((status == StatusCode::CREATED, next));
        }
        Err(self.fail(resp).await)
    }

    /// `POST`: append one message (JSON mode) or bytes. `seq` is the
    /// protocol's `Stream-Seq`: the server refuses a value that is not
    /// strictly greater (lexicographically) than the last accepted one.
    pub async fn append(
        &self,
        path: &str,
        content_type: &str,
        body: Vec<u8>,
        seq: Option<&str>,
        close: bool,
    ) -> Result<Appended, HttpError> {
        let mut extra = vec![("content-type", content_type.to_owned())];
        if let Some(s) = seq {
            extra.push(("stream-seq", s.to_owned()));
        }
        if close {
            extra.push(("stream-closed", "true".into()));
        }
        let resp = self
            .send(
                Method::POST,
                path,
                &[],
                &extra,
                Some(body),
                Some(Duration::from_secs(300)),
            )
            .await?;
        if resp.status().is_success() {
            return Ok(Appended {
                next: Self::hdr(&resp, "stream-next-offset").unwrap_or_default(),
                closed: Self::flag(&resp, "stream-closed"),
            });
        }
        Err(self.fail(resp).await)
    }

    /// `POST` with arbitrary extra headers, answering the status as well. For
    /// tests of protocol features the binding does not use (idempotent producers).
    pub async fn append_raw(
        &self,
        path: &str,
        content_type: &str,
        body: Vec<u8>,
        headers: &[(&str, String)],
    ) -> Result<(u16, Appended), HttpError> {
        let mut extra = vec![("content-type", content_type.to_owned())];
        extra.extend(headers.iter().map(|(k, v)| (*k, v.clone())));
        let resp = self
            .send(
                Method::POST,
                path,
                &[],
                &extra,
                Some(body),
                Some(Duration::from_secs(300)),
            )
            .await?;
        if resp.status().is_success() {
            return Ok((
                resp.status().as_u16(),
                Appended {
                    next: Self::hdr(&resp, "stream-next-offset").unwrap_or_default(),
                    closed: Self::flag(&resp, "stream-closed"),
                },
            ));
        }
        Err(self.fail(resp).await)
    }

    pub async fn head(&self, path: &str) -> Result<Head, HttpError> {
        let resp = self
            .send(
                Method::HEAD,
                path,
                &[],
                &[],
                None,
                Some(Duration::from_secs(30)),
            )
            .await?;
        if resp.status().is_success() {
            return Ok(Head {
                next: Self::hdr(&resp, "stream-next-offset").unwrap_or_default(),
                closed: Self::flag(&resp, "stream-closed"),
            });
        }
        Err(self.fail(resp).await)
    }

    /// Pin the phase of the server clock (see `clock.rs`): poll `HEAD` every
    /// 10 ms until the `Date` header ticks over to the next second (at most
    /// 1.5 s). Any status is fine; only the header matters. Idempotent.
    pub async fn calibrate(&self) -> Result<(), HttpError> {
        if self.clock.is_calibrated() {
            return Ok(());
        }
        let deadline = Instant::now() + Duration::from_millis(1500);
        let mut first: Option<u64> = None;
        loop {
            let resp = self
                .send(
                    Method::HEAD,
                    "dcp-clock",
                    &[],
                    &[],
                    None,
                    Some(Duration::from_secs(10)),
                )
                .await?;
            drop(resp);
            let seen = self.clock.last_date_ms();
            match (first, seen) {
                (None, s) => first = s,
                (Some(f), Some(s)) if s > f => break,
                _ => {}
            }
            if Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if first.is_none() {
            return Err(HttpError::Protocol(
                "the server sends no Date header".into(),
            ));
        }
        self.clock.set_calibrated();
        Ok(())
    }

    pub async fn delete(&self, path: &str) -> Result<(), HttpError> {
        let resp = self
            .send(
                Method::DELETE,
                path,
                &[],
                &[],
                None,
                Some(Duration::from_secs(30)),
            )
            .await?;
        if resp.status().is_success() {
            return Ok(());
        }
        Err(self.fail(resp).await)
    }

    /// One catch-up `GET` from `offset`. A JSON stream's body is an array.
    pub async fn read_chunk(
        &self,
        path: &str,
        offset: &str,
        json: bool,
    ) -> Result<ReadResp, HttpError> {
        let resp = self
            .send(
                Method::GET,
                path,
                &[("offset", offset)],
                &[],
                None,
                Some(Duration::from_secs(300)),
            )
            .await?;
        self.parse_read(resp, json).await
    }

    /// Long-poll: waits up to the server's timeout for data after `offset`.
    pub async fn long_poll(
        &self,
        path: &str,
        offset: &str,
        json: bool,
    ) -> Result<ReadResp, HttpError> {
        let resp = self
            .send(
                Method::GET,
                path,
                &[("offset", offset), ("live", "long-poll")],
                &[],
                None,
                Some(Duration::from_secs(120)),
            )
            .await?;
        self.parse_read(resp, json).await
    }

    async fn parse_read(&self, resp: reqwest::Response, json: bool) -> Result<ReadResp, HttpError> {
        let status = resp.status();
        if !(status == StatusCode::OK || status == StatusCode::NO_CONTENT) {
            return Err(self.fail(resp).await);
        }
        let next = Self::hdr(&resp, "stream-next-offset")
            .ok_or_else(|| HttpError::Protocol("no Stream-Next-Offset on a read".into()))?;
        let up_to_date = Self::flag(&resp, "stream-up-to-date");
        let closed = Self::flag(&resp, "stream-closed");
        let body = resp.bytes().await.map_err(transport)?.to_vec();
        self.stats
            .bytes_received
            .fetch_add(body.len() as u64, Ordering::Relaxed);
        let items = if json && !body.is_empty() {
            match serde_json::from_slice::<Value>(&body) {
                Ok(Value::Array(a)) => a,
                Ok(other) => vec![other],
                Err(e) => {
                    return Err(HttpError::Protocol(format!(
                        "a JSON stream returned bad JSON: {e}"
                    )))
                }
            }
        } else {
            Vec::new()
        };
        Ok(ReadResp {
            items,
            body: if json { Vec::new() } else { body },
            next,
            up_to_date,
            closed,
        })
    }

    /// Read a JSON stream from `offset` to its current tail. Returns every
    /// message and the offset to resume from, and whether the stream is closed.
    pub async fn read_all(
        &self,
        path: &str,
        offset: &str,
    ) -> Result<(Vec<Value>, String, bool), HttpError> {
        let mut items = Vec::new();
        let mut at = offset.to_owned();
        loop {
            let r = self.read_chunk(path, &at, true).await?;
            let progressed = r.next != at;
            items.extend(r.items);
            at = r.next;
            if r.up_to_date || r.closed || !progressed {
                return Ok((items, at, r.closed));
            }
        }
    }

    /// Read a whole octet stream (a file blob).
    pub async fn read_bytes(&self, path: &str) -> Result<Vec<u8>, HttpError> {
        let mut out = Vec::new();
        let mut at = START.to_owned();
        loop {
            let r = self.read_chunk(path, &at, false).await?;
            let progressed = r.next != at;
            out.extend_from_slice(&r.body);
            at = r.next;
            if r.up_to_date || r.closed || !progressed {
                return Ok(out);
            }
        }
    }
}
