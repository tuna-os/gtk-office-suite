use std::sync::Arc;
use std::time::Duration;

use dcp_core::DocumentStore;
use dcp_streams::http::{HttpVersion, Transport};
use dcp_streams::{Http, Identity, StreamsConfig, StreamsStore};

/// `Some(base url)` when a server is configured. Panics when it is not,
/// unless `DCP_SKIP_STREAMS=1`, in which case it says so and returns `None`.
pub fn server_url(test: &str) -> Option<String> {
    match std::env::var("DCP_STREAMS_URL") {
        Ok(u) if !u.is_empty() => Some(u),
        _ => {
            if std::env::var("DCP_SKIP_STREAMS").as_deref() == Ok("1") {
                eprintln!("SKIPPED {test} (DCP_SKIP_STREAMS=1): no Durable Streams server");
                None
            } else {
                panic!(
                    "DCP_STREAMS_URL is not set: start the reference server (scripts/streams-up.sh) or set DCP_SKIP_STREAMS=1 to skip these tests explicitly"
                );
            }
        }
    }
}

#[macro_export]
macro_rules! require_streams {
    () => {
        match $crate::common::server_url(module_path!()) {
            Some(u) => u,
            None => return,
        }
    };
}

/// An environment variable that must be set for a TLS test, with the same rule as
/// `DCP_STREAMS_URL`: a missing one FAILS the test unless `DCP_SKIP_STREAMS_TLS=1`,
/// which prints that it skipped.
pub fn tls_env(test: &str, var: &str) -> Option<String> {
    match std::env::var(var) {
        Ok(v) if !v.is_empty() => Some(v),
        _ => {
            if std::env::var("DCP_SKIP_STREAMS_TLS").as_deref() == Ok("1") {
                eprintln!("SKIPPED {test} (DCP_SKIP_STREAMS_TLS=1): {var} is not set");
                None
            } else {
                panic!("{var} is not set: start a TLS server (scripts/streams-tls-up.sh) or set DCP_SKIP_STREAMS_TLS=1 to skip the TLS tests explicitly");
            }
        }
    }
}

/// `Some(https base url)` for a TLS test. A run against an `http://` server is not a TLS run:
/// the test fails, or, with `DCP_SKIP_STREAMS_TLS=1`, says it skipped.
pub fn tls_base(test: &str) -> Option<String> {
    let u = tls_env(test, "DCP_STREAMS_URL")?;
    if u.starts_with("https://") {
        return Some(u);
    }
    if std::env::var("DCP_SKIP_STREAMS_TLS").as_deref() == Ok("1") {
        eprintln!("SKIPPED {test} (DCP_SKIP_STREAMS_TLS=1): DCP_STREAMS_URL is not https:// ({u})");
        return None;
    }
    panic!("DCP_STREAMS_URL is not https:// ({u}): this test needs a TLS server (scripts/streams-tls-up.sh), or DCP_SKIP_STREAMS_TLS=1 to skip the TLS tests explicitly");
}

#[macro_export]
macro_rules! require_tls {
    () => {
        match $crate::common::tls_base(module_path!()) {
            Some(u) => u,
            None => return,
        }
    };
}

/// The HTTP version this run is meant to exercise: `DCP_STREAMS_HTTP` = `1.1` or `2`.
/// Required whenever the base URL is `https://` (so a run cannot silently test the
/// wrong one); `auto` offers both and accepts either.
pub fn wanted_version() -> Option<HttpVersion> {
    match std::env::var("DCP_STREAMS_HTTP").as_deref() {
        Ok("1.1") => Some(HttpVersion::Http1),
        Ok("2") => Some(HttpVersion::Http2),
        Ok("auto") => None,
        other => panic!(
            "DCP_STREAMS_HTTP must be 1.1, 2 or auto for an https:// server, got {other:?}"
        ),
    }
}

pub fn ca_pem() -> Vec<u8> {
    let p = std::env::var("DCP_STREAMS_CA")
        .expect("DCP_STREAMS_CA (path of the test CA) is not set: an https:// base URL needs it");
    std::fs::read(&p).unwrap_or_else(|e| panic!("cannot read DCP_STREAMS_CA {p}: {e}"))
}

/// The transport every test uses for an `https://` base: trust only the test CA,
/// and the HTTP version of this run.
pub fn transport() -> Transport {
    Transport {
        ca_pem: Some(ca_pem()),
        version: wanted_version(),
        require_tls: true,
        ..Transport::default()
    }
}

/// An `Http` for `base`. `http://` is plain; `https://` trusts the CA of
/// `DCP_STREAMS_CA` only and requires `DCP_STREAMS_HTTP`.
pub fn http(base: &str) -> Http {
    if base.starts_with("https://") {
        Http::with_transport(base, transport())
    } else {
        Http::new(base)
    }
}

pub fn store_at(base: &str, device: &str) -> Arc<StreamsStore> {
    Arc::new(StreamsStore::new(
        http(base),
        Identity::generate(),
        StreamsConfig::new(device),
    ))
}

pub fn store_with(base: &str, device: &str, id: Identity) -> Arc<StreamsStore> {
    Arc::new(StreamsStore::new(
        http(base),
        id,
        StreamsConfig::new(device),
    ))
}

pub fn dynamic(s: &Arc<StreamsStore>) -> Arc<dyn DocumentStore> {
    s.clone()
}

pub const SETTLE: Duration = Duration::from_secs(10);
