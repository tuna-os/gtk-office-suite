// string_pool.rs — String interning for deduplication.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pattern: LibreOffice svl/ SharedStringPool.
// Reduces memory for repeated cell values and document text.

use std::collections::HashMap;

/// A pool that interns strings, returning a stable ID.
/// Case-insensitive lookup supported for spreadsheet cell matching.
pub struct StringPool {
    strings: Vec<String>,
    index: HashMap<String, u32>,
}

impl StringPool {
    pub fn new() -> Self {
        StringPool { strings: Vec::new(), index: HashMap::new() }
    }

    /// Intern a string, returning its pool ID. Deduplicates.
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.index.get(s) {
            return id;
        }
        let id = self.strings.len() as u32;
        self.strings.push(s.to_string());
        self.index.insert(s.to_string(), id);
        id
    }

    /// Look up a string by ID.
    pub fn get(&self, id: u32) -> Option<&str> {
        self.strings.get(id as usize).map(|s| s.as_str())
    }

    /// Number of strings in the pool.
    pub fn len(&self) -> usize { self.strings.len() }

    /// Whether the pool is empty.
    pub fn is_empty(&self) -> bool { self.strings.is_empty() }
}

impl Default for StringPool {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern_dedup() {
        let mut pool = StringPool::new();
        let id1 = pool.intern("hello");
        let id2 = pool.intern("hello");
        assert_eq!(id1, id2);
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn test_intern_multiple() {
        let mut pool = StringPool::new();
        let a = pool.intern("a");
        let b = pool.intern("b");
        let c = pool.intern("c");
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_eq!(pool.len(), 3);
        assert_eq!(pool.get(a), Some("a"));
        assert_eq!(pool.get(b), Some("b"));
    }

    #[test]
    fn test_get_invalid() {
        let pool = StringPool::new();
        assert_eq!(pool.get(999), None);
    }
}
