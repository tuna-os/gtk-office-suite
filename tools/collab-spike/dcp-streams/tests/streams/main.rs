//! RFC-0004 Spike B: the Durable Streams binding against a real Durable
//! Streams server (the Node reference server, pinned in
//! `scripts/streams-server/package.json`), and, in the TLS spike, over TLS and HTTP/2.
//!
//! Needs `DCP_STREAMS_URL` (for example `http://127.0.0.1:4437/v1/stream/`, or an `https://` URL
//! from `scripts/streams-tls-up.sh` with `DCP_STREAMS_CA` and `DCP_STREAMS_HTTP`);
//! every test FAILS without it unless `DCP_SKIP_STREAMS=1`, which prints that
//! it skipped. Tests that start their own server (restart, crash) need
//! `node` and the installed `scripts/streams-server` (and, over TLS, the Caddy of
//! `scripts/streams-caddy-fetch.sh`); they fail without them unless `DCP_SKIP_STREAMS=1`.
//! The TLS-only tests (`tls::`, `idle::`, `tls_measure::`) fail without an https server unless
//! `DCP_SKIP_STREAMS_TLS=1`.

mod auth;
mod common;
mod conformance;
mod fwd;
mod idle;
mod live;
mod measure;
mod properties;
mod rawtls;
mod restart;
mod server;
mod tls;
mod tls_measure;
