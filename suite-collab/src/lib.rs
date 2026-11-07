// SPDX-License-Identifier: GPL-3.0-or-later
//! The wire protocol between an app shell and the `collabd` sidecar
//! (RFC-0005 section 2.2).
//!
//! GTK-free and synchronous. No async runtime, no network stack, no `dcp-*`
//! crate: the shell links this and nothing that talks to a server. The sidecar
//! (its own cargo workspace, `collab/`) links it too, so the protocol is one
//! file and a unit-testable one.
//!
//! - [`protocol`]: [`Command`] (shell to sidecar) and [`Event`] (sidecar to shell)
//! - [`codec`]: one JSON object per line, a bounded [`LineReader`], the decoders
//! - [`client`]: [`SessionClient`] and the [`ProcessClient`] over any `Read`/`Write`
//! - [`b64`]: the base64 of every byte payload

pub mod b64;
pub mod client;
pub mod codec;
pub mod protocol;

pub use client::{ClientError, Negotiated, ProcessClient, SessionClient};
pub use codec::{
    decode_message, decode_request, encode_message, encode_request, write_line, DecodeError, Line,
    LineReader, MAX_LINE_BYTES,
};
pub use protocol::*;
