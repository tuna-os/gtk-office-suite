//! Shared helpers for the Synapse-backed tests.
//!
//! A test that needs Synapse MUST fail when `DCP_SYNAPSE_URL` is unset, unless
//! `DCP_SKIP_SYNAPSE=1` says to skip, in which case it prints that it skipped.
//! (A silently skipping test counts as a pass; that hid bugs here before.)
#![allow(dead_code)]

pub mod proxy;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dcp_core::{Level, Meta, NewFile};
use dcp_matrix::synapse::{register_user, Registered, DEFAULT_SHARED_SECRET};
use dcp_matrix::{Credentials, MatrixConfig, MatrixStore};

#[derive(Clone, Debug)]
pub struct Synapse {
    pub url: String,
    pub secret: String,
    /// `default` or `relaxed`, as started by scripts/synapse-up.sh.
    pub profile: String,
}

/// `Some` when a Synapse is configured. `None` only when the environment
/// explicitly asks to skip; otherwise this panics so the run goes red.
pub fn synapse() -> Option<Synapse> {
    match std::env::var("DCP_SYNAPSE_URL") {
        Ok(url) if !url.is_empty() => Some(Synapse {
            url,
            secret: std::env::var("DCP_SYNAPSE_SHARED_SECRET").unwrap_or_else(|_| DEFAULT_SHARED_SECRET.into()),
            profile: std::env::var("DCP_SYNAPSE_PROFILE").unwrap_or_else(|_| "unknown".into()),
        }),
        _ => {
            if std::env::var("DCP_SKIP_SYNAPSE").as_deref() == Ok("1") {
                eprintln!("SKIPPED (DCP_SKIP_SYNAPSE=1): this test needs a Synapse at DCP_SYNAPSE_URL");
                None
            } else {
                panic!(
                    "DCP_SYNAPSE_URL is not set. Start one with `scripts/synapse-up.sh relaxed` and \
                     `eval` its output, or set DCP_SKIP_SYNAPSE=1 to skip these tests explicitly."
                );
            }
        }
    }
}

/// `let Some(syn) = require_synapse!() else { return };`
#[macro_export]
macro_rules! require_synapse {
    () => {
        $crate::common::synapse()
    };
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn unique(prefix: &str) -> String {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    format!("{prefix}{:x}{:x}{}", t, std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed))
}

impl Synapse {
    pub fn config(&self) -> MatrixConfig {
        MatrixConfig::new(self.url.clone())
    }

    pub async fn register(&self, prefix: &str) -> Registered {
        register_user(&self.url, &self.secret, &unique(prefix), "dcp-spike-password-1", false)
            .await
            .expect("register user")
    }

    pub async fn user(&self, prefix: &str) -> MatrixStore {
        self.user_with(prefix, |_| {}).await
    }

    pub async fn user_with(&self, prefix: &str, tweak: impl FnOnce(&mut MatrixConfig)) -> MatrixStore {
        let mut cfg = self.config();
        tweak(&mut cfg);
        let reg = self.register(prefix).await;
        MatrixStore::connect(cfg, &Credentials::from(&reg)).await.expect("connect")
    }

    /// Same account as `reg`, with an optional config tweak (for a second
    /// store of one user, or to point at a proxy).
    pub async fn connect_as(&self, reg: &Registered, tweak: impl FnOnce(&mut MatrixConfig)) -> MatrixStore {
        let mut cfg = self.config();
        tweak(&mut cfg);
        MatrixStore::connect(cfg, &Credentials::from(reg)).await.expect("connect")
    }
}

pub fn meta(title: &str) -> Meta {
    Meta { dcp: 1, title: title.into(), mime: "application/vnd.oasis.opendocument.text".into(), levels: vec![Level::L0, Level::L1, Level::L2], algorithms: Some(vec!["loro/1".into()]), editor_services: None }
}

pub fn file(bytes: &[u8]) -> NewFile {
    NewFile { mime: "application/vnd.oasis.opendocument.text".into(), bytes: bytes.to_vec() }
}

/// Deterministic, incompressible-looking bytes.
pub fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

pub async fn sleep_ms(ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

/// Append one measurement as a JSON line to `$DCP_MEASURE_OUT`
/// (default `target/spike-a-measurements.jsonl`), and print it.
pub fn record(syn: &Synapse, name: &str, data: serde_json::Value) {
    use std::io::Write;
    let line = serde_json::json!({ "profile": syn.profile, "name": name, "data": data });
    println!("MEASURE {line}");
    let path = std::env::var("DCP_MEASURE_OUT").unwrap_or_else(|_| "target/spike-a-measurements.jsonl".into());
    if let Some(dir) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path).expect("open measurement file");
    writeln!(f, "{line}").expect("write measurement");
}

pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx]
}

pub fn summarize(mut v: Vec<f64>) -> serde_json::Value {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    serde_json::json!({
        "n": v.len(),
        "min": v.first(), "p50": percentile(&v, 0.5), "p95": percentile(&v, 0.95), "max": v.last(),
        "mean": if v.is_empty() { f64::NAN } else { v.iter().sum::<f64>() / v.len() as f64 },
    })
}

use dcp_matrix::matrix_sdk::reqwest;

/// The event exactly as the homeserver stores it (so ciphertext for an
/// encrypted room), fetched with `as`'s token.
pub async fn server_event(syn: &Synapse, as_: &MatrixStore, doc: &str, event_id: &str) -> Result<serde_json::Value, u16> {
    let mut url = reqwest::Url::parse(&syn.url).unwrap();
    url.path_segments_mut().unwrap().extend(["_matrix", "client", "v3", "rooms", doc, "event", event_id]);
    let token = as_.client().access_token().expect("access token");
    let r = reqwest::Client::new().get(url).bearer_auth(token).send().await.expect("GET event");
    if r.status().is_success() {
        Ok(r.json().await.expect("event json"))
    } else {
        Err(r.status().as_u16())
    }
}

/// Can `device`'s store decrypt this server-side encrypted event with the keys
/// it holds (no network involved beyond what the SDK does for key backup)?
pub async fn can_decrypt(device: &MatrixStore, doc: &str, encrypted_event: &serde_json::Value) -> bool {
    use dcp_matrix::matrix_sdk::ruma::events::room::encrypted::OriginalSyncRoomEncryptedEvent;
    use dcp_matrix::matrix_sdk::ruma::{serde::Raw, RoomId};
    let rid = RoomId::parse(doc).unwrap();
    let room = device.client().get_room(&rid).expect("device has the room in its store");
    let raw: Raw<OriginalSyncRoomEncryptedEvent> = Raw::from_json(serde_json::value::to_raw_value(encrypted_event).unwrap());
    let ev = room.decrypt_event(&raw, None).await.expect("decrypt_event call");
    !ev.kind.is_utd()
}

/// The megolm session ID an event was encrypted with, as seen by `reader`.
pub async fn session_id_of(reader: &MatrixStore, doc: &str, event_id: &str) -> Option<String> {
    use dcp_matrix::matrix_sdk::ruma::{EventId, RoomId};
    let room = reader.client().get_room(&RoomId::parse(doc).unwrap())?;
    let ev = room.event(&EventId::parse(event_id).unwrap(), None).await.ok()?;
    ev.encryption_info().and_then(|i| i.session_id().map(str::to_owned))
}
