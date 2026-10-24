//! DCP conformance: spec section 12 written once against
//! [`dcp_core::DocumentStore`] and run against every binding.
//!
//! A binding provides a [`StoreFactory`] and calls [`run_all`], or picks
//! journeys by name with [`run_selected`] / [`run_one`] / the `pub async fn`s
//! in [`journeys`]. A journey whose capability the binding declares absent is
//! reported as skipped, never as passed; [`Report::assert_skipped_exactly`]
//! makes a binding state its skips.
//!
//! The suite must be able to fail: [`wrappers`] holds deliberately broken
//! stores, and `tests/mutation.rs` asserts that each is caught by the
//! journey it breaks.

pub mod compare;
mod failure;
pub mod fixture;
pub mod journeys;
pub mod memory_factory;
pub mod runner;
pub mod wrappers;

pub use compare::{assert_same_content, assert_same_view};
pub use failure::Failure;
pub use fixture::{
    new_file, standard_meta, Capabilities, Faults, Fixture, Forger, RealTime, StoreFactory, Time,
    ALGORITHM, BASE_FILE, DOC_MODEL, MIME,
};
pub use memory_factory::{MemoryFactory, MemoryForger};
pub use runner::{
    journey_names, run_all, run_one, run_selected, Journey, JourneyResult, Need, Outcome, Report,
    JOURNEYS,
};
