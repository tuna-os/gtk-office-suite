// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs the unit tests of the `join_call` example's command line parsing.
//!
//! The example itself requires the `livekit` feature, so its own test target
//! is not built by the default `cargo test`; its parsing module is compiled
//! here instead. Among other things this proves that the access token is not
//! accepted as a command line argument (#168).

#[path = "../examples/support/join_call_args.rs"]
mod join_call_args;
