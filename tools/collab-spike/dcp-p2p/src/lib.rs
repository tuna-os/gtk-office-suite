//! DCP draft-1 peer-to-peer binding (RFC-0004 spike C): iroh for transport and files, p2panda for
//! signed append-only logs, group membership and group encryption.
//!
//! Start at [`P2pStore`]. How it works and what was measured: `docs/rfc/0004-spike-c-results.md`.

pub mod acl;
pub mod apply;
pub mod auth_check;
pub mod blobcrypt;
pub mod clock;
pub mod codec;
pub mod envelope;
pub mod ext;
pub mod msg;
pub mod net;
pub mod ops;
pub mod pathlog;
pub mod replica;
pub mod signed;
pub mod store;
pub mod sync;
pub mod wire;

pub use clock::{Clock, SkewedClock, SystemClock};
pub use replica::{Counters, P2pConfig};
pub use store::P2pStore;
