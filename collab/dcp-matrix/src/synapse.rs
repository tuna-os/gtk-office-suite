//! Test-support helpers for a Synapse started by `scripts/synapse-up.sh`:
//! user creation through the shared-secret admin endpoint (which is not
//! subject to `rc_registration` or `rc_login`, so the *default* rate-limit
//! profile can still create users), and a few server facts the results doc
//! records.

use dcp_core::StoreError;
use hmac::{Hmac, Mac};
use matrix_sdk::reqwest;
use serde_json::{json, Value};
use sha1::Sha1;

/// The secret `scripts/synapse-up.sh` writes into homeserver.yaml unless
/// `SYNAPSE_SHARED_SECRET` overrides it. A throw-away test instance.
pub const DEFAULT_SHARED_SECRET: &str = "dcp-spike-shared-secret";

#[derive(Clone, Debug)]
pub struct Registered {
    pub user_id: String,
    pub device_id: String,
    pub access_token: String,
    pub password: String,
}

fn tr(e: reqwest::Error) -> StoreError {
    StoreError::Transport(e.to_string())
}

/// `POST /_synapse/admin/v1/register` with the nonce/HMAC handshake.
pub async fn register_user(
    homeserver: &str,
    shared_secret: &str,
    localpart: &str,
    password: &str,
    admin: bool,
) -> Result<Registered, StoreError> {
    let http = reqwest::Client::new();
    let url = format!("{}/_synapse/admin/v1/register", homeserver.trim_end_matches('/'));
    let nonce = http
        .get(&url)
        .send()
        .await
        .map_err(tr)?
        .json::<Value>()
        .await
        .map_err(tr)?["nonce"]
        .as_str()
        .ok_or_else(|| StoreError::Transport("register: no nonce".into()))?
        .to_owned();
    // Synapse checks this MAC with HMAC-SHA1. It is over `nonce\0user\0password\0admin|notadmin`, no trailing NUL.
    let mut mac = Hmac::<Sha1>::new_from_slice(shared_secret.as_bytes())
        .map_err(|e| StoreError::Invalid(e.to_string()))?;
    let flag = if admin { "admin" } else { "notadmin" };
    mac.update(nonce.as_bytes());
    mac.update(b"\0");
    mac.update(localpart.as_bytes());
    mac.update(b"\0");
    mac.update(password.as_bytes());
    mac.update(b"\0");
    mac.update(flag.as_bytes());
    let mac = hex::encode(mac.finalize().into_bytes());
    let resp = http
        .post(&url)
        .json(&json!({
            "nonce": nonce, "username": localpart, "password": password,
            "admin": admin, "mac": mac,
        }))
        .send()
        .await
        .map_err(tr)?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(tr)?;
    if !status.is_success() {
        return Err(StoreError::Transport(format!("register {localpart}: HTTP {status}: {body}")));
    }
    let field = |k: &str| {
        body[k].as_str().map(str::to_owned).ok_or_else(|| StoreError::Transport(format!("register: no {k} in {body}")))
    };
    Ok(Registered {
        user_id: field("user_id")?,
        device_id: field("device_id")?,
        access_token: field("access_token")?,
        password: password.to_owned(),
    })
}

/// What the homeserver says about itself: the `Server` header (Synapse puts
/// its version there) and the `Date` header.
pub async fn server_header(homeserver: &str) -> Result<String, StoreError> {
    let r = reqwest::Client::new()
        .get(format!("{}/_matrix/client/versions", homeserver.trim_end_matches('/')))
        .send()
        .await
        .map_err(tr)?;
    Ok(r.headers().get("server").and_then(|v| v.to_str().ok()).unwrap_or("(no Server header)").to_owned())
}

/// The homeserver's wall clock from the `Date` header, in ms since the epoch
/// (1 s resolution).
pub async fn server_date_ms(http: &reqwest::Client, homeserver: &str) -> Result<u64, StoreError> {
    let r = http
        .get(format!("{}/_matrix/client/versions", homeserver.trim_end_matches('/')))
        .send()
        .await
        .map_err(tr)?;
    let date = r
        .headers()
        .get("date")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| StoreError::Transport("no Date header".into()))?;
    let t = httpdate::parse_http_date(date).map_err(|e| StoreError::Transport(format!("bad Date {date:?}: {e}")))?;
    Ok(t.duration_since(std::time::UNIX_EPOCH).map_err(|e| StoreError::Transport(e.to_string()))?.as_millis() as u64)
}
