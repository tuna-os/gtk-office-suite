//! `collabd`, the collaboration sidecar of RFC-0005 (option B).
//!
//! A process an app shell starts and talks to over a pipe: one JSON object per
//! line on stdin (`suite_collab::Command`), one per line on stdout
//! (`suite_collab::Event`), logs on stderr only. The shell never links a network
//! stack and never sees a credential; this process owns tokio, the
//! `dyn DocumentStore` of the chosen binding, TLS and the key.
//!
//! - [`daemon`]: the command loop (hello, connect, share, join, publish, ...)
//! - [`doc`]: one task per open document: the poll loop, the receiver, the outbox
//! - [`connector`]: binding name to store (Streams now; Matrix in a later pull request)
//! - [`identity`]: the Streams signing key, the one thing this process stores
//! - [`invite`]: what `share` hands out and `join` takes
//! - [`output`]: the single writer of stdout

pub mod connector;
pub mod daemon;
pub mod doc;
pub mod identity;
pub mod invite;
pub mod output;

/// The DCP update algorithm this sidecar carries (docs/collab-spec/profiles/loro-1.md).
pub const ALGORITHM: &str = "loro/1";

/// The document models it carries under that algorithm: `tables/1` (Tables) and
/// `letters/1` (Letters), both in docs/collab-spec/profiles/loro-1.md section 8.
pub const DOC_MODELS: &[&str] = &["tables/1", "letters/1"];

/// Log to stderr. Never to stdout: stdout is the protocol.
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        eprintln!("collabd: {}", format_args!($($arg)*))
    };
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}
