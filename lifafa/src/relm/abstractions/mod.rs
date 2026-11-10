//! A module for convenient abstractions over gtk-rs.

pub mod drawing;

mod toaster;

pub use drawing::{DrawContext, DrawHandler};

pub use toaster::Toaster;
