// SPDX-License-Identifier: GPL-3.0-or-later
//! The messages of RFC-0005 2.2: [`Command`] (shell to sidecar) and [`Event`]
//! (sidecar to shell), one JSON object per line.
//!
//! Shape of a line: `{"req": 7, "cmd": "share", ...}` and
//! `{"req": 7, "event": "shared", ...}`. `req` is chosen by the shell; the
//! sidecar copies it onto the event that answers that command (and onto an
//! `error` for it). Events that nobody asked for (an update from another
//! person, a state change) carry no `req`. Byte payloads are standard base64.
//!
//! Credentials are not in any message: a command names a server, never a
//! token, a password or a key (the sidecar keeps those, see `collabd`).

use serde::{Deserialize, Serialize};

use crate::b64;

/// The newest protocol version this crate speaks. The shell opens with `hello`
/// naming the newest it speaks; the sidecar answers with the version both will
/// use ([`negotiate`]).
pub const PROTOCOL_VERSION: u32 = 1;
/// The oldest version this crate still speaks.
pub const MIN_PROTOCOL_VERSION: u32 = 1;

/// The version two ends will use, or `None` when they share none.
///
/// `theirs` is the newest version the other end speaks and `their_min` is the
/// oldest it still speaks (the sidecar answers a `hello` that has no minimum
/// with its own `theirs`, so a client that names only one number speaks
/// exactly that one).
pub fn negotiate(theirs: u32, their_min: u32) -> Option<u32> {
    let best = theirs.min(PROTOCOL_VERSION);
    (best >= MIN_PROTOCOL_VERSION && best >= their_min).then_some(best)
}

/// Which service holds the document (RFC-0005: the two bindings tried in the apps).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Binding {
    Matrix,
    Streams,
}

impl Binding {
    pub fn as_str(self) -> &'static str {
        match self {
            Binding::Matrix => "matrix",
            Binding::Streams => "streams",
        }
    }
}

/// DCP spec section 5.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Viewer,
    Commenter,
    Editor,
    Owner,
}

/// Where a binding is. Never a credential.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectConfig {
    /// Streams: the base URL under which stream paths are appended, for the
    /// reference server `http://127.0.0.1:4437/v1/stream/`. Matrix: the homeserver URL.
    pub server: String,
    /// Streams over TLS: a PEM file with the CA that signed the server's
    /// certificate. The client has no built-in trust roots, so a private CA is
    /// the only way to a private server; this is a path, not a secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_file: Option<String>,
}

/// A person given a role when a document is shared (Streams: a public key,
/// `ed25519:<base64url>`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    pub principal: String,
    pub role: Role,
}

/// Shell to sidecar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// The first line. `protocol` is the newest version the shell speaks.
    Hello {
        protocol: u32,
        /// The oldest version the shell still speaks; absent means `protocol`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_protocol: Option<u32>,
        client: String,
    },
    /// Choose a binding. Answered by `connected`, or by `error` with code
    /// `unsupported_binding` (this sidecar build cannot speak it) or
    /// `unconfigured_binding` (it can, but is not set up).
    Connect {
        binding: Binding,
        config: ConnectConfig,
    },
    /// Create a document from `first_version` (a saved file, published as the
    /// root version) and, for a live session, publish `genesis` (the sharer's
    /// Loro snapshot, loro/1 profile section 4) as update `seq 0`. Answered by `shared`.
    Share {
        title: String,
        mime: String,
        #[serde(with = "b64::bytes")]
        first_version: Vec<u8>,
        /// Absent: share the versions only, no live session.
        #[serde(default, skip_serializing_if = "Option::is_none", with = "b64::option")]
        genesis: Option<Vec<u8>>,
        /// The application's document model, `tables/1`.
        doc_model: String,
        /// Everyone else who may open the document, with their role.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        people: Vec<Person>,
    },
    /// Open a shared document. Answered by `joined`, then `genesis`, `update`
    /// and `heads` events as the sidecar reads the document.
    Join { invite: String },
    /// One delta of the live document. The sidecar numbers it, stamps it with
    /// the session's base version (loro/1 profile section 3) and publishes it,
    /// as a file payload when it is over 34,000 bytes. Answered by `published`.
    PublishUpdate {
        doc: String,
        #[serde(with = "b64::bytes")]
        bytes: Vec<u8>,
    },
    /// The saved file. Parents: the version this session last saved or started
    /// from together with every head this session already holds (`covered`), minus
    /// any that another of them descends from; or exactly the versions in
    /// `resolve` when the user merges heads. In a live session the version
    /// carries `covers` (loro/1 profile section 8.3): the last update of the
    /// prefix of the update log this session has applied. Answered by `published`.
    PublishVersion {
        doc: String,
        #[serde(with = "b64::bytes")]
        bytes: Vec<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        /// Heads the user chose to merge (DCP spec 6.3); empty for an ordinary save.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        resolve: Vec<String>,
    },
    /// Ask for the file of one version; answered by `version_file`.
    FetchVersion { doc: String, id: String },
    /// The shell's dry run of an `update` passed and it is in the replica.
    Applied { doc: String, id: String },
    /// The shell's dry run (DCP spec 7 rule 3) failed: the update is dropped and reported.
    Rejected { doc: String, id: String, why: String },
    /// Read the whole update log again from the start of the session (after a
    /// `gap` the shell has repaired by loading the latest version).
    Resync { doc: String },
    /// Stop reading and publishing this document. The document is not changed.
    Leave { doc: String },
    /// Owner: give `principal` a role.
    Grant { doc: String, principal: String, role: Role },
    /// Owner: remove `principal`.
    Revoke { doc: String, principal: String },
}

impl Command {
    /// The name on the wire, for logs.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Hello { .. } => "hello",
            Command::Connect { .. } => "connect",
            Command::Share { .. } => "share",
            Command::Join { .. } => "join",
            Command::PublishUpdate { .. } => "publish_update",
            Command::PublishVersion { .. } => "publish_version",
            Command::FetchVersion { .. } => "fetch_version",
            Command::Applied { .. } => "applied",
            Command::Rejected { .. } => "rejected",
            Command::Resync { .. } => "resync",
            Command::Leave { .. } => "leave",
            Command::Grant { .. } => "grant",
            Command::Revoke { .. } => "revoke",
        }
    }
}

/// The names [`Command`] has on the wire, for telling an unknown command from a
/// malformed one.
pub const COMMAND_NAMES: &[&str] = &[
    "hello",
    "connect",
    "share",
    "join",
    "publish_update",
    "publish_version",
    "fetch_version",
    "applied",
    "rejected",
    "resync",
    "leave",
    "grant",
    "revoke",
];

/// A line the shell wrote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub req: Option<u64>,
    #[serde(flatten)]
    pub command: Command,
}

/// The indicator (RFC-0005 5.1): what the status button says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnState {
    Connecting,
    Online,
    /// The server cannot be reached; edits are queued, nothing is lost.
    Offline { since_ms: u64 },
    /// The server refused this principal.
    AuthRequired,
    Error { message: String },
}

/// Who wrote a record, as the binding authenticated it (DCP spec 11).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    pub principal: String,
    pub device: String,
}

/// One head of the version graph (`dcp_core::view::build`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadInfo {
    pub id: String,
    pub author: Author,
    /// The binding's own clock.
    pub ts_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The `covers.through` update id of this version, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub covers: Option<String>,
    /// This sidecar session published this version.
    #[serde(default)]
    pub mine: bool,
    /// This session already holds the version's content: it is `mine`, or the
    /// update named by `covers` is inside the prefix of the update log this
    /// session has applied (so it has nothing the live replica lacks).
    #[serde(default)]
    pub covered: bool,
}

/// What a `dropped` event is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DropKind {
    /// A record the binding refused to read: wrong signature, will not
    /// decrypt, author without the role, malformed.
    Record,
    /// An update the receiver refused (bad payload) or the shell rejected.
    Update,
    /// A version `view::build` discarded.
    Version,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishKind {
    Update,
    Version,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The line is not JSON, or not an object of the right shape.
    MalformedLine,
    /// The line was longer than the limit; it was skipped.
    LineTooLong,
    UnknownCommand,
    /// `hello` named only versions this sidecar does not speak.
    UnsupportedProtocol,
    /// A command other than `hello` before `hello`, or a document command before `connect`.
    NotConnected,
    /// This sidecar build has no code for that binding.
    UnsupportedBinding,
    /// The binding exists but is not set up (for Matrix: no login).
    UnconfiguredBinding,
    InvalidInvite,
    UnknownDocument,
    NotFound,
    Forbidden,
    Conflict,
    RateLimited,
    Invalid,
    Transport,
    /// The sidecar could not keep its own state (the identity key file).
    Storage,
}

/// Sidecar to shell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// The answer to `hello`: the version both ends will use.
    Hello {
        protocol: u32,
        server: String,
        /// The bindings this build can speak.
        bindings: Vec<Binding>,
        /// The longest line this sidecar reads, in bytes.
        max_line: usize,
    },
    /// The answer to `connect`. `principal` is what the other people put on a roster.
    Connected {
        binding: Binding,
        principal: String,
        device: String,
    },
    /// The answer to `share`. `invite` is all another person's `join` needs.
    Shared {
        doc: String,
        invite: String,
        principal: String,
        /// The root version, which is the base of every update of the session.
        base_version: String,
    },
    /// The answer to `join`. `base_version` is absent when the document has no
    /// live session (versions only) or its genesis is not visible yet.
    Joined {
        doc: String,
        title: String,
        mime: String,
        role: Role,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_version: Option<String>,
    },
    State {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        doc: Option<String>,
        state: ConnState,
    },
    /// The sharer's Loro snapshot (loro/1 profile section 4), for a joiner
    /// that has no replica yet. Once per session and document, before any `update`.
    Genesis {
        doc: String,
        id: String,
        base_version: String,
        author: Author,
        #[serde(with = "b64::bytes")]
        bytes: Vec<u8>,
    },
    /// One update the receiver has ordered and accepted, waiting for the
    /// shell's dry run; answer `applied` or `rejected`. Never an update this
    /// process published itself.
    Update {
        doc: String,
        id: String,
        author: Author,
        ts_ms: u64,
        #[serde(with = "b64::bytes")]
        bytes: Vec<u8>,
    },
    /// The version graph changed, or a head became `covered` (updates arrived
    /// or the shell answered `applied`): sent again whenever the heads, any
    /// head's `covered` flag or `conflict` changes.
    Heads {
        doc: String,
        heads: Vec<HeadInfo>,
        /// More than one head and at least one of them is not `covered`: show it,
        /// never pick one silently (DCP spec 4.2 rule 5). Two heads whose content
        /// this live replica already holds are not a conflict (RFC-0005 2.5).
        conflict: bool,
    },
    /// The answer to `fetch_version`.
    VersionFile {
        doc: String,
        id: String,
        mime: String,
        #[serde(with = "b64::bytes")]
        bytes: Vec<u8>,
    },
    /// The answer to `publish_update` and `publish_version`, once the binding has the record.
    Published {
        doc: String,
        kind: PublishKind,
        id: String,
        /// `seq` for an update, absent for a version.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seq: Option<u64>,
        /// Parents of a version.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        parents: Vec<String>,
    },
    /// DCP spec 7 rules 3 and 10: dropped and reported.
    Dropped {
        doc: String,
        kind: DropKind,
        id: String,
        reason: String,
    },
    /// DCP spec 7 rule 2: update `expected` of this stream never arrived, though
    /// `found` did. The stream is stopped.
    Gap {
        doc: String,
        author: Author,
        expected: u64,
        found: u64,
    },
    /// The answer to a command that has nothing to report but that it was done:
    /// `leave`, `grant`, `revoke`, `resync`.
    Done {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        doc: Option<String>,
    },
    Error {
        code: ErrorCode,
        message: String,
    },
}

impl Event {
    pub fn error(code: ErrorCode, message: impl Into<String>) -> Event {
        Event::Error {
            code,
            message: message.into(),
        }
    }
}

pub const EVENT_NAMES: &[&str] = &[
    "hello",
    "connected",
    "shared",
    "joined",
    "state",
    "genesis",
    "update",
    "heads",
    "version_file",
    "published",
    "dropped",
    "gap",
    "done",
    "error",
];

/// A line the sidecar wrote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub req: Option<u64>,
    #[serde(flatten)]
    pub event: Event,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation_picks_the_newest_version_both_speak() {
        assert_eq!(negotiate(1, 1), Some(1));
        // A newer shell that still speaks 1: we answer 1.
        assert_eq!(negotiate(5, 1), Some(1));
        // A shell that needs at least 2 cannot talk to a sidecar that speaks 1.
        assert_eq!(negotiate(5, 2), None);
        // An older shell than the oldest we speak.
        assert_eq!(negotiate(0, 0), None);
    }
}
