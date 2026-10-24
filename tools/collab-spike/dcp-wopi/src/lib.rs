//! Spike D (RFC-0004, spec 8.2): a WOPI host over any
//! [`dcp_core::DocumentStore`], and a WOPI client simulator that is a
//! `DocumentStore` too, so the shared conformance journeys run through WOPI.
//!
//! GTK-free, pure Rust, no `unsafe`.

pub mod client;
pub mod host;
pub mod package;
pub mod timefmt;

pub use client::{WopiCapabilities, WopiClientStore, CAPABILITIES};
pub use host::{HostConfig, LoggedRequest, RunningHost, WopiHost, COOL_STATUS_DOC_CHANGED, MAX_LOCK_LEN};
