//! Binding name to store.
//!
//! [`Connector`] is what the daemon asks for a `dyn DocumentStore`. The real one
//! ([`DefaultConnector`]) speaks Durable Streams and refuses Matrix with
//! `unsupported_binding`: the Matrix store, its login and its keyring come with
//! the Matrix pull request. A test connector hands out an in-memory store.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use dcp_core::{DocumentStore, StoreError};
use dcp_streams::http::Transport;
use dcp_streams::{Http, StreamsConfig, StreamsStore};
use suite_collab::{Binding, ConnectConfig, ErrorCode};

use crate::identity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectError {
    pub code: ErrorCode,
    pub message: String,
}

impl ConnectError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        ConnectError {
            code,
            message: message.into(),
        }
    }
}

impl From<StoreError> for ConnectError {
    fn from(e: StoreError) -> Self {
        ConnectError::new(code_of(&e), e.to_string())
    }
}

pub fn code_of(e: &StoreError) -> ErrorCode {
    match e {
        StoreError::NotFound(_) => ErrorCode::NotFound,
        StoreError::Forbidden(_) => ErrorCode::Forbidden,
        StoreError::Conflict(_) => ErrorCode::Conflict,
        StoreError::RateLimited { .. } => ErrorCode::RateLimited,
        StoreError::Invalid(_) => ErrorCode::Invalid,
        StoreError::Transport(_) => ErrorCode::Transport,
    }
}

#[async_trait]
pub trait Connector: Send + Sync {
    /// The bindings this build can speak; `hello` reports them.
    fn bindings(&self) -> Vec<Binding>;

    /// A store acting as this process's principal. `device` is this process's
    /// device id. Called after the daemon has checked that `binding` is in
    /// [`Self::bindings`] and that `config.server` is not empty.
    async fn connect(
        &self,
        binding: Binding,
        config: &ConnectConfig,
        device: &str,
    ) -> Result<Arc<dyn DocumentStore>, ConnectError>;
}

/// The production connector.
pub struct DefaultConnector {
    data_dir: PathBuf,
}

impl DefaultConnector {
    pub fn new(data_dir: PathBuf) -> Self {
        DefaultConnector { data_dir }
    }
}

/// Plain `http://` is for a server on this machine only: everything past the
/// loopback interface must be `https://`, whatever the shell asked for.
fn is_loopback(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn transport_for(config: &ConnectConfig) -> Result<Transport, ConnectError> {
    let server = config.server.trim();
    if server.starts_with("https://") {
        let Some(ca) = &config.ca_file else {
            // The client has no built-in trust roots (dcp-streams::http): without a
            // CA it cannot verify any server, so say so now instead of failing
            // every request later.
            return Err(ConnectError::new(
                ErrorCode::UnconfiguredBinding,
                "an https:// server needs `ca_file`, a PEM file with the CA that signed its certificate: this client trusts no built-in roots",
            ));
        };
        let pem = std::fs::read(ca).map_err(|e| {
            ConnectError::new(ErrorCode::Invalid, format!("cannot read ca_file {ca}: {e}"))
        })?;
        Ok(Transport {
            ca_pem: Some(pem),
            require_tls: true,
            ..Transport::default()
        })
    } else if is_loopback(server) {
        Ok(Transport::default())
    } else {
        Err(ConnectError::new(
            ErrorCode::Invalid,
            "plain http:// is only allowed to localhost; use https://",
        ))
    }
}

#[async_trait]
impl Connector for DefaultConnector {
    fn bindings(&self) -> Vec<Binding> {
        vec![Binding::Streams]
    }

    async fn connect(
        &self,
        binding: Binding,
        config: &ConnectConfig,
        device: &str,
    ) -> Result<Arc<dyn DocumentStore>, ConnectError> {
        match binding {
            Binding::Matrix => Err(ConnectError::new(
                ErrorCode::UnsupportedBinding,
                "this collabd build has no Matrix binding yet",
            )),
            Binding::Streams => {
                // Everything that can be refused is refused before a key is made: a
                // rejected `connect` leaves nothing on disk.
                let transport = transport_for(config)?;
                let http =
                    Http::with_transport(&crate::invite::normalise_server(&config.server), transport);
                let me = identity::load_or_create(&self.data_dir)
                    .map_err(|e| ConnectError::new(ErrorCode::Storage, e.to_string()))?;
                let store = StreamsStore::new(http, me, StreamsConfig::new(device));
                // Reach the server now, so an unreachable one is an error of `connect`.
                store.calibrate().await?;
                Ok(Arc::new(store))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_is_for_loopback_only() {
        for ok in [
            "http://127.0.0.1:4437/v1/stream/",
            "http://localhost/x",
            "http://[::1]:80/",
        ] {
            assert!(is_loopback(ok), "{ok}");
        }
        for bad in [
            "http://example.org/",
            "http://127.evil.example/x",
            "http://10.0.0.5/",
            "https://127.0.0.1/",
            "ftp://localhost/",
        ] {
            assert!(!is_loopback(bad), "{bad}");
        }
    }

    #[test]
    fn https_without_a_ca_is_an_unconfigured_binding_not_a_late_failure() {
        let cfg = ConnectConfig {
            server: "https://s.example/v1/stream/".into(),
            ca_file: None,
        };
        let e = transport_for(&cfg).unwrap_err();
        assert_eq!(e.code, ErrorCode::UnconfiguredBinding);
        let cfg = ConnectConfig {
            server: "http://s.example/v1/stream/".into(),
            ca_file: None,
        };
        assert_eq!(transport_for(&cfg).unwrap_err().code, ErrorCode::Invalid);
    }

    #[tokio::test]
    async fn matrix_is_an_unsupported_binding_in_this_build() {
        let c = DefaultConnector::new(std::env::temp_dir().join("collabd-unused"));
        assert_eq!(c.bindings(), vec![Binding::Streams]);
        let cfg = ConnectConfig {
            server: "https://matrix.example".into(),
            ca_file: None,
        };
        let Err(e) = c.connect(Binding::Matrix, &cfg, "dev").await else {
            panic!("matrix connected")
        };
        assert_eq!(e.code, ErrorCode::UnsupportedBinding);
    }
}
