//! The Matrix binding (`dcp-matrix`, spike A) and the WOPI host adapter
//! (`dcp-wopi`, spike D) run against each other on a real Synapse.
//!
//! * [`rig::Rig`]: one Synapse room, three principals (owner, editor, viewer)
//!   as `MatrixStore`s, and one `WopiHost<MatrixStore>` that serves the same
//!   document to a WOPI token per principal. Every request a test makes is
//!   either native (a `MatrixStore` call) or WOPI (HTTP to the host, whose
//!   backing store is the principal's own `MatrixStore`).
//! * [`factories`]: `dcp-conformance` factories for the two stacks, so the
//!   shared section-12 journeys run against `MatrixStore` and against
//!   `WopiClientStore -> WopiHost -> MatrixStore`.
//! * [`checks`]: the cross-binding checks, written to return a `Failure`
//!   instead of panicking so that a negative control can assert they fail
//!   when a store is broken.
//!
//! Test-only support: nothing in here is a product crate.

// The Synapse helpers (`DCP_SYNAPSE_URL`, users, the HTTP fault proxy) are the
// ones spike A's own tests use. Included, not copied, so the two cannot drift.
#[path = "../../dcp-matrix/tests/common/mod.rs"]
pub mod common;

pub mod checks;
pub mod factories;
pub mod power_at_event;
pub mod profile;
pub mod raw;
pub mod rig;

pub use rig::{Handles, Rig, RigOptions, Who, Wrap};
