//! DCP over Durable Streams (RFC-0004 Spike B).
//!
//! The server stores bytes in append-only HTTP logs and is trusted for
//! nothing else. Everything DCP needs on top is done by the client:
//! XChaCha20-Poly1305 for every record and file, Ed25519 signatures for
//! authorship, X25519 key wrapping to members, key rotation (an *epoch*) for
//! every change of roster, a lease built on the protocol's `Stream-Seq`
//! compare-and-set, and a clock built on the HTTP `Date` header.
//!
//! - [`crypto`]: the primitives
//! - [`wire`]: the bytes on the server and what is signed
//! - [`http`]: the Durable Streams HTTP protocol as used here
//! - [`clock`]: `now_ms` from `Date`
//! - [`doc`]: what a client believes about a document, and the drop rules
//! - [`store`]: [`StreamsStore`], the [`dcp_core::DocumentStore`]

pub mod clock;
pub mod crypto;
pub mod doc;
pub mod http;
pub mod store;
pub mod wire;

pub use crypto::Identity;
pub use http::Http;
pub use store::{StreamsConfig, StreamsStore};
