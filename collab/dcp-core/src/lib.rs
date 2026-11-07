//! DCP draft-0 (docs/DOCUMENT-COLLABORATION-SPEC.md): the binding-agnostic
//! types, the [`DocumentStore`] trait that Matrix, WOPI and the in-memory
//! reference store all implement, and the rule logic every binding shares.
//!
//! The spec's JSON schemas (docs/collab-spec/schemas) are the source of truth
//! for the wire shapes; `model` mirrors them and [`schema`] validates against
//! them.
//!
//! Logic modules, by spec section:
//! - [`graph`]: heads, parent validation, conflicts (4.2 rules 5 to 7, 6)
//! - [`lease`]: lease rules (4.3)
//! - [`integrity`]: file references and verification (4.2 rule 2)
//! - [`schema`]: validate any DCP object (4)
//! - [`outbox`]: queue and back off, never drop a version (10)
//! - [`presence`]: ephemeral roster with the 15 s rule (4.5)
//! - [`publish`]: the writer's checks (4.2 rules 3 and 4)
//! - [`updates`]: the update profile (4.4, 7)
//! - [`view`]: the reader pipeline combining the above
//! - [`memory`]: the in-memory reference store

pub mod graph;
pub mod integrity;
pub mod lease;
pub mod memory;
pub mod model;
pub mod outbox;
pub mod presence;
pub mod publish;
pub mod schema;
pub mod store;
pub mod updates;
pub mod view;

pub use model::*;
pub use store::*;
