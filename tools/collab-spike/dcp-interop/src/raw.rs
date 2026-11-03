//! A step-by-step WOPI client: one HTTP request per method, no hidden
//! sequencing. `WopiClientStore` does CheckFileInfo, GetFile, LOCK, PutFile,
//! UNLOCK in one call; the interop checks need to put another writer between
//! two of those steps, so they speak the wire themselves.

use dcp_conformance::Failure;
use dcp_wopi::WopiHost;
use reqwest::Method;
use serde_json::Value;

/// What came back.
#[derive(Debug, Clone)]
pub struct Resp {
    pub status: u16,
    /// `X-WOPI-Lock` of the response.
    pub lock: Option<String>,
    /// `X-WOPI-ItemVersion` of the response.
    pub item_version: Option<String>,
    /// `X-DCP-Heads` of the response (GetFile).
    pub heads: Option<String>,
    pub body: Vec<u8>,
    /// Wall time from sending the request to having its whole body, ms.
    pub elapsed_ms: u128,
}

impl Resp {
    pub fn json(&self) -> Result<Value, Failure> {
        serde_json::from_slice(&self.body).map_err(|e| {
            Failure::new(format!(
                "HTTP {}: body is not JSON ({e}): {:?}",
                self.status,
                self.text()
            ))
        })
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body[..self.body.len().min(300)]).into_owned()
    }
}

#[derive(Clone)]
pub struct RawWopi {
    http: reqwest::Client,
    /// `<base>/wopi/files/<encoded doc>`
    src: String,
    token: String,
}

impl RawWopi {
    pub fn new(base_url: &str, doc: &str, token: &str) -> Self {
        RawWopi {
            http: reqwest::Client::builder().no_proxy().build().expect("reqwest client"),
            src: WopiHost::wopi_src(base_url, doc),
            token: token.to_owned(),
        }
    }

    async fn send(
        &self,
        method: Method,
        contents: bool,
        query: &[(&str, &str)],
        headers: &[(&str, &str)],
        body: Option<Vec<u8>>,
    ) -> Result<Resp, Failure> {
        let url = if contents { format!("{}/contents", self.src) } else { self.src.clone() };
        let mut req = self
            .http
            .request(method, url)
            .query(&[("access_token", self.token.as_str())])
            .query(query);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        req = match body {
            Some(b) => req.header("content-type", "application/octet-stream").body(b),
            None => req.header("content-length", "0"),
        };
        let started = std::time::Instant::now();
        let r = req.send().await.map_err(|e| Failure::new(format!("WOPI request failed: {e}")))?;
        let header = |name: &str| r.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
        let (status, lock, item_version, heads) = (
            r.status().as_u16(),
            header("x-wopi-lock"),
            header("x-wopi-itemversion"),
            header("x-dcp-heads"),
        );
        let body = r
            .bytes()
            .await
            .map_err(|e| Failure::new(format!("WOPI response body: {e}")))?
            .to_vec();
        Ok(Resp { status, lock, item_version, heads, body, elapsed_ms: started.elapsed().as_millis() })
    }

    pub async fn check_file_info(&self) -> Result<Resp, Failure> {
        self.send(Method::GET, false, &[], &[], None).await
    }

    /// `GetFile`: the head, or (host extension) a retained version.
    pub async fn get_file(&self, dcp_version: Option<&str>) -> Result<Resp, Failure> {
        let q: Vec<(&str, &str)> = dcp_version.map(|v| ("dcp_version", v)).into_iter().collect();
        self.send(Method::GET, true, &q, &[], None).await
    }

    async fn lock_op(&self, op: &str, lock: &str) -> Result<Resp, Failure> {
        self.send(Method::POST, false, &[], &[("X-WOPI-Override", op), ("X-WOPI-Lock", lock)], None)
            .await
    }

    pub async fn lock(&self, lock: &str) -> Result<Resp, Failure> {
        self.lock_op("LOCK", lock).await
    }

    pub async fn unlock(&self, lock: &str) -> Result<Resp, Failure> {
        self.lock_op("UNLOCK", lock).await
    }

    /// `REFRESH_LOCK`: Collabora never sends it (it refreshes with `LOCK` and
    /// the same string), other WOPI clients do.
    pub async fn refresh_lock(&self, lock: &str) -> Result<Resp, Failure> {
        self.lock_op("REFRESH_LOCK", lock).await
    }

    /// `UNLOCK_AND_RELOCK`: `LOCK` carrying `X-WOPI-OldLock`.
    pub async fn unlock_and_relock(&self, new_lock: &str, old_lock: &str) -> Result<Resp, Failure> {
        self.send(
            Method::POST,
            false,
            &[],
            &[("X-WOPI-Override", "LOCK"), ("X-WOPI-Lock", new_lock), ("X-WOPI-OldLock", old_lock)],
            None,
        )
        .await
    }

    pub async fn get_lock(&self) -> Result<Resp, Failure> {
        self.send(Method::POST, false, &[], &[("X-WOPI-Override", "GET_LOCK")], None).await
    }

    /// `PutFile`. `timestamp` is Collabora's `X-COOL-WOPI-Timestamp`, the
    /// `LastModifiedTime` it was last given; `None` is a forced save.
    pub async fn put_file(
        &self,
        lock: Option<&str>,
        timestamp: Option<&str>,
        bytes: &[u8],
    ) -> Result<Resp, Failure> {
        let mut h = vec![("X-WOPI-Override", "PUT")];
        if let Some(l) = lock {
            h.push(("X-WOPI-Lock", l));
        }
        if let Some(t) = timestamp {
            h.push(("X-COOL-WOPI-Timestamp", t));
        }
        self.send(Method::POST, true, &[], &h, Some(bytes.to_vec())).await
    }
}
