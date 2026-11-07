//! DCP draft 1 Matrix binding (spec 8.1): Spike A of RFC-0004.
//!
//! [`MatrixStore`] implements [`dcp_core::DocumentStore`] over matrix-sdk with
//! end-to-end encryption. See `docs/rfc/0004-spike-a-results.md` for what was
//! measured against a real Synapse and which parts of the spec's strawman
//! survived.

mod config;
mod error;
pub mod levels;
mod queue;
pub mod state;
mod store;
pub mod synapse;

/// Re-exported so tests and measurements use exactly the SDK version this crate
/// was built against.
pub use matrix_sdk;

pub use config::{ClockSource, MatrixConfig};
pub use queue::{Stats, StatsSnapshot};
pub use levels::{Level, LevelStats, SenderCheck, SenderRule};
pub use store::{Credentials, MatrixStore, ReadReport};
