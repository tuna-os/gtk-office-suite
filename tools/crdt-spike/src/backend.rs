//! The interface each candidate implements, so the scenarios in main.rs are
//! written once and run identically against all three.

use crate::decks::{DOp, DeckRead};
use crate::tables::{Canon, TOp, View};

pub trait TablesDoc: Sized {
    /// Build the shared starting document (the 1000 empty rows) as peer 0.
    fn base(base: &View) -> Self;
    /// A replica of this document for another peer.
    fn fork(&mut self, peer: u64) -> Self;
    /// One user action = one transaction / commit.
    fn apply(&mut self, action: &[TOp]);
    fn read(&mut self) -> Canon;
    /// Every encoding the library offers, by name, in bytes.
    fn encodings(&mut self) -> Vec<(&'static str, Vec<u8>)>;
    /// Decode the first entry of `encodings()`.
    fn load(bytes: &[u8]) -> Self;
    /// Pull everything `other` has that we lack; returns the bytes exchanged.
    fn merge_from(&mut self, other: &mut Self) -> usize;
}

pub trait DecksDoc: Sized {
    fn base(ops: &[DOp]) -> Self;
    fn fork(&mut self, peer: u64) -> Self;
    fn apply(&mut self, action: &[DOp]);
    fn read(&mut self) -> DeckRead;
    fn encodings(&mut self) -> Vec<(&'static str, Vec<u8>)>;
    fn load(bytes: &[u8]) -> Self;
    fn merge_from(&mut self, other: &mut Self) -> usize;
}
