//! Spike E (RFC-0004): the OCM and WebDAV binding, measured against real
//! Nextcloud. See `docs/rfc/0004-spike-e-results.md`.
//!
//! - [`WebdavStore`] is a [`dcp_core::DocumentStore`] over Nextcloud's WebDAV and
//!   OCS sharing APIs.
//! - [`dav`] and [`xml`] are the HTTP and multistatus plumbing, public so the
//!   measurement tests can show raw answers.
//! - [`clock`] estimates the server's clock to the millisecond.
//! - [`ocm`] is the Open Cloud Mesh receiver and sender helper that drives and
//!   records federation.

pub mod clock;
pub mod dav;
pub mod ocm;
pub mod store;
pub mod xml;

pub use clock::ServerClock;
pub use dav::{Dav, Resp};
pub use store::{share_permissions, WebdavConfig, WebdavStore};
