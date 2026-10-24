//! DCP draft-0 (docs/DOCUMENT-COLLABORATION-SPEC.md): the binding-agnostic
//! types and the [`DocumentStore`] trait that Matrix, WOPI and the in-memory
//! reference store all implement.
//!
//! The spec's JSON schemas (docs/collab-spec/schemas) are the source of truth
//! for the wire shapes; `model` mirrors them and its tests validate against
//! them.

pub mod model;
pub mod store;

pub use model::*;
pub use store::*;
