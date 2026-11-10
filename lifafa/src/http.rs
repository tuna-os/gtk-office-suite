//! The one place Hylki talks HTTP through ureq (#2026).
//!
//! The rest of the app was written against ureq 2's builder: `ureq::post(url)
//! .set(..).timeout(..).send_json(..)`, an `Error::Status(code, response)` for
//! a refusal, an `Error::Transport` for everything else. ureq 3 reshaped all
//! of that (an `http::Request`, a status code error without its response, a
//! `Body` that must be read through a handle). This module keeps the shape the
//! callers already use over the new library, so a later ureq release changes
//! one file and not twenty.
//!
//! Behaviour kept from ureq 2:
//! - an HTTP status of 400 or more is an [`Error::Status`] carrying the
//!   response, so its body can still be read for the server's own wording;
//! - `timeout` bounds the whole call, `timeout_read` each read;
//! - a redirect limit of 0 hands back the 3xx response itself.

use std::io::Read;
use std::time::Duration;

/// The longest body read into memory by [`Response::into_string`],
/// [`Response::into_json`] and [`Response::into_bytes`].
const BODY_LIMIT: u64 = 32 * 1024 * 1024;

/// What went wrong below HTTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidUrl,
    UnknownScheme,
    Dns,
    ConnectionFailed,
    Io,
    Other,
}

/// A failure before there was an answer, or in reading it.
#[derive(Debug)]
pub struct Transport {
    kind: ErrorKind,
    message: String,
}

impl Transport {
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// A request that did not succeed.
#[derive(Debug)]
pub enum Error {
    /// The server answered 400 or more.
    Status(u16, Response),
    /// There was no usable answer.
    Transport(Transport),
}

impl Error {
    fn transport(kind: ErrorKind, message: impl Into<String>) -> Self {
        Error::Transport(Transport {
            kind,
            message: message.into(),
        })
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            Error::Status(..) => ErrorKind::Other,
            Error::Transport(t) => t.kind,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Status(code, resp) => write!(f, "{}: status code {code}", resp.url),
            Error::Transport(t) => t.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::transport(ErrorKind::Io, e.to_string())
    }
}

fn from_ureq(e: ureq::Error) -> Error {
    use ureq::Error as E;
    let kind = match &e {
        E::BadUri(_) | E::Http(_) => ErrorKind::InvalidUrl,
        E::HostNotFound => ErrorKind::Dns,
        E::ConnectionFailed => ErrorKind::ConnectionFailed,
        E::Io(_) => ErrorKind::Io,
        _ => ErrorKind::Other,
    };
    let kind = match (&e, kind) {
        (E::BadUri(m), _) if m.contains("scheme") => ErrorKind::UnknownScheme,
        (_, k) => k,
    };
    Error::transport(kind, e.to_string())
}

enum Payload {
    Bytes(Vec<u8>),
    Stream(ureq::Body),
}

/// An answer from a server, with its body still to be read.
pub struct Response {
    status: u16,
    url: String,
    headers: Vec<(String, String)>,
    body: Payload,
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

impl Response {
    /// A response made up in memory: what the tests hand the code that
    /// reads a refusal.
    #[cfg(test)]
    pub fn new(status: u16, _text: &str, body: &str) -> Result<Self, Error> {
        Ok(Response {
            status,
            url: String::new(),
            headers: Vec::new(),
            body: Payload::Bytes(body.as_bytes().to_vec()),
        })
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    /// The URL the answer came from, after redirects.
    pub fn get_url(&self) -> &str {
        &self.url
    }

    /// A header's value, by name in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The media type of the body, without parameters such as the charset;
    /// `text/plain` when the server named none.
    pub fn content_type(&self) -> &str {
        self.header("Content-Type")
            .and_then(|v| v.split(';').next())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("text/plain")
    }

    /// The reason phrase for the status.
    pub fn status_text(&self) -> &str {
        ureq::http::StatusCode::from_u16(self.status)
            .ok()
            .and_then(|s| s.canonical_reason())
            .unwrap_or("")
    }

    /// The body as a stream, for large downloads.
    pub fn into_reader(self) -> Box<dyn Read + Send + 'static> {
        match self.body {
            Payload::Bytes(b) => Box::new(std::io::Cursor::new(b)),
            Payload::Stream(body) => Box::new(body.into_reader()),
        }
    }

    pub fn into_bytes(self) -> std::io::Result<Vec<u8>> {
        let mut out = Vec::new();
        self.into_reader().take(BODY_LIMIT).read_to_end(&mut out)?;
        Ok(out)
    }

    /// The body as text, lossy over bad UTF-8 as a browser would.
    pub fn into_string(self) -> std::io::Result<String> {
        Ok(String::from_utf8_lossy(&self.into_bytes()?).into_owned())
    }

    pub fn into_json<T: serde::de::DeserializeOwned>(self) -> std::io::Result<T> {
        serde_json::from_slice(&self.into_bytes()?).map_err(std::io::Error::from)
    }
}

enum Body<'a> {
    None,
    Owned(Vec<u8>),
    Bytes(&'a [u8]),
    Stream(&'a mut dyn Read),
}

/// A request being put together.
pub struct Request {
    agent: Agent,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    query: Vec<(String, String)>,
    timeout: Option<Duration>,
}

impl Request {
    pub fn set(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// A query-string pair, percent-encoded when the URL is built.
    pub fn query(mut self, name: &str, value: &str) -> Self {
        self.query.push((name.to_string(), value.to_string()));
        self
    }

    /// A limit on the whole call, from connecting to the last byte.
    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = Some(d);
        self
    }

    pub fn call(self) -> Result<Response, Error> {
        self.dispatch(Body::None)
    }

    /// Sends what `reader` yields, without holding it in memory.
    pub fn send(self, mut reader: impl Read) -> Result<Response, Error> {
        self.dispatch(Body::Stream(&mut reader))
    }

    pub fn send_bytes(self, bytes: &[u8]) -> Result<Response, Error> {
        self.dispatch(Body::Bytes(bytes))
    }

    pub fn send_string(self, text: &str) -> Result<Response, Error> {
        self.dispatch(Body::Bytes(text.as_bytes()))
    }

    pub fn send_json(self, body: impl serde::Serialize) -> Result<Response, Error> {
        let bytes = serde_json::to_vec(&body).map_err(|e| Error::from(std::io::Error::from(e)))?;
        let req = if self.has("Content-Type") {
            self
        } else {
            self.set("Content-Type", "application/json")
        };
        req.dispatch(Body::Owned(bytes))
    }

    pub fn send_form(self, pairs: &[(&str, &str)]) -> Result<Response, Error> {
        let body = pairs
            .iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let req = if self.has("Content-Type") {
            self
        } else {
            self.set("Content-Type", "application/x-www-form-urlencoded")
        };
        req.dispatch(Body::Owned(body.into_bytes()))
    }

    fn has(&self, name: &str) -> bool {
        self.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
    }

    fn dispatch(self, body: Body<'_>) -> Result<Response, Error> {
        let mut url = self.url.clone();
        for (i, (k, v)) in self.query.iter().enumerate() {
            url.push(if i == 0 && !url.contains('?') { '?' } else { '&' });
            url.push_str(&format!("{}={}", encode(k), encode(v)));
        }
        let mut builder = ureq::http::Request::builder()
            .method(self.method.as_str())
            .uri(&url);
        for (k, v) in &self.headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        let agent = &self.agent.inner;
        let answer = match body {
            Body::None => {
                let req = builder
                    .body(())
                    .map_err(|e| Error::transport(ErrorKind::InvalidUrl, e.to_string()))?;
                run(agent, req, self.timeout)
            }
            Body::Owned(b) => {
                let req = builder
                    .body(b)
                    .map_err(|e| Error::transport(ErrorKind::InvalidUrl, e.to_string()))?;
                run(agent, req, self.timeout)
            }
            Body::Bytes(b) => {
                let req = builder
                    .body(b)
                    .map_err(|e| Error::transport(ErrorKind::InvalidUrl, e.to_string()))?;
                run(agent, req, self.timeout)
            }
            Body::Stream(r) => {
                let req = builder
                    .body(ureq::SendBody::from_reader(r))
                    .map_err(|e| Error::transport(ErrorKind::InvalidUrl, e.to_string()))?;
                run(agent, req, self.timeout)
            }
        }?;
        use ureq::ResponseExt;
        let reached = answer.get_uri().to_string();
        let (parts, body) = answer.into_parts();
        let status = parts.status.as_u16();
        let headers = parts
            .headers
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().to_string(), v.to_str().ok()?.to_string())))
            .collect();
        let url = if reached.is_empty() { url } else { reached };
        let response = Response {
            status,
            url,
            headers,
            body: Payload::Stream(body),
        };
        if status >= 400 {
            Err(Error::Status(status, response))
        } else {
            Ok(response)
        }
    }
}

fn run<B: ureq::AsSendBody>(
    agent: &ureq::Agent,
    req: ureq::http::Request<B>,
    timeout: Option<Duration>,
) -> Result<ureq::http::Response<ureq::Body>, Error> {
    match timeout {
        Some(d) => {
            let req = agent.configure_request(req).timeout_global(Some(d)).build();
            agent.run(req)
        }
        None => agent.run(req),
    }
    .map_err(from_ureq)
}

/// Percent-encoding for form and query values: everything but the
/// unreserved characters.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A client with its own timeouts and redirect policy.
#[derive(Clone)]
pub struct Agent {
    inner: ureq::Agent,
}

impl Agent {
    pub fn request(&self, method: &str, url: &str) -> Request {
        Request {
            agent: self.clone(),
            method: method.to_ascii_uppercase(),
            url: url.to_string(),
            headers: Vec::new(),
            query: Vec::new(),
            timeout: None,
        }
    }

    pub fn get(&self, url: &str) -> Request {
        self.request("GET", url)
    }

    pub fn post(&self, url: &str) -> Request {
        self.request("POST", url)
    }

    pub fn put(&self, url: &str) -> Request {
        self.request("PUT", url)
    }
}

/// Builds an [`Agent`].
pub struct AgentBuilder {
    config: ureq::config::ConfigBuilder<ureq::typestate::AgentScope>,
}

impl AgentBuilder {
    pub fn new() -> Self {
        AgentBuilder {
            config: ureq::Agent::config_builder()
                .http_status_as_error(false)
                .max_redirects_will_error(false),
        }
    }

    /// A limit on a whole call.
    pub fn timeout(mut self, d: Duration) -> Self {
        self.config = self.config.timeout_global(Some(d));
        self
    }

    pub fn timeout_connect(mut self, d: Duration) -> Self {
        self.config = self.config.timeout_connect(Some(d));
        self
    }

    /// A limit on each read of the body.
    pub fn timeout_read(mut self, d: Duration) -> Self {
        self.config = self.config.timeout_per_read(Some(d));
        self
    }

    /// How many redirects to follow; 0 hands the 3xx back.
    pub fn redirects(mut self, n: u32) -> Self {
        self.config = self.config.max_redirects(n);
        self
    }

    pub fn user_agent(mut self, ua: &str) -> Self {
        self.config = self.config.user_agent(ua);
        self
    }

    pub fn build(self) -> Agent {
        Agent {
            inner: ureq::Agent::new_with_config(self.config.build()),
        }
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

fn default_agent() -> &'static Agent {
    static AGENT: std::sync::OnceLock<Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| AgentBuilder::new().build())
}

pub fn request(method: &str, url: &str) -> Request {
    default_agent().request(method, url)
}

pub fn get(url: &str) -> Request {
    request("GET", url)
}

pub fn head(url: &str) -> Request {
    request("HEAD", url)
}

pub fn post(url: &str) -> Request {
    request("POST", url)
}

pub fn put(url: &str) -> Request {
    request("PUT", url)
}

pub fn delete(url: &str) -> Request {
    request("DELETE", url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_percent_encoded() {
        assert_eq!(encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(encode("Az09-_.~"), "Az09-_.~");
    }

    #[test]
    fn a_bad_url_is_a_transport_error_not_a_panic() {
        let e = get("not a url").call().unwrap_err();
        assert!(matches!(e, Error::Transport(_)), "{e}");
    }

    #[test]
    fn a_made_up_refusal_reads_back() {
        let r = Response::new(403, "Forbidden", "{\"error\":\"no\"}").unwrap();
        assert_eq!(r.status(), 403);
        let v: serde_json::Value = r.into_json().unwrap();
        assert_eq!(v["error"], "no");
    }
}
