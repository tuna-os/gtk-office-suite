//! Presence, spec 4.5 (L3): cursors, selections, the active page. Ephemeral:
//! this roster lives in memory only (nothing here is serializable, on
//! purpose: presence MUST NOT be written to a durable log) and a participant
//! is dropped 15 seconds after it was last refreshed.

use std::collections::HashMap;

/// Receivers drop presence after this long without refresh.
pub const TTL_MS: u64 = 15_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub principal: String,
    pub session: String,
    /// Defined by the algorithm profile; opaque here.
    pub payload: Vec<u8>,
    pub seen_ms: u64,
}

#[derive(Debug, Default)]
pub struct Roster {
    entries: HashMap<(String, String), Entry>,
}

impl Roster {
    /// A presence message arrived at local time `now_ms`. It must name the
    /// principal and session; a refresh replaces the earlier payload.
    pub fn refresh(&mut self, principal: &str, session: &str, payload: Vec<u8>, now_ms: u64) {
        self.entries.insert(
            (principal.to_owned(), session.to_owned()),
            Entry {
                principal: principal.to_owned(),
                session: session.to_owned(),
                payload,
                seen_ms: now_ms,
            },
        );
    }

    /// Who is present at `now_ms`: everyone refreshed within [`TTL_MS`].
    pub fn present(&self, now_ms: u64) -> Vec<&Entry> {
        let mut v: Vec<&Entry> = self
            .entries
            .values()
            .filter(|e| now_ms.saturating_sub(e.seen_ms) < TTL_MS)
            .collect();
        v.sort_by(|a, b| (&a.principal, &a.session).cmp(&(&b.principal, &b.session)));
        v
    }

    /// Forget the expired entries.
    pub fn expire(&mut self, now_ms: u64) {
        self.entries
            .retain(|_, e| now_ms.saturating_sub(e.seen_ms) < TTL_MS);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_until_fifteen_seconds_without_refresh() {
        let mut r = Roster::default();
        r.refresh("@a:x", "s1", b"cell B2".to_vec(), 1_000);
        assert_eq!(r.present(1_000).len(), 1);
        assert_eq!(r.present(15_999).len(), 1);
        assert!(r.present(16_000).is_empty());
    }

    #[test]
    fn refreshing_extends_and_replaces() {
        let mut r = Roster::default();
        r.refresh("@a:x", "s1", b"B2".to_vec(), 0);
        r.refresh("@a:x", "s1", b"C3".to_vec(), 10_000);
        let p = r.present(20_000);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].payload, b"C3");
    }

    #[test]
    fn two_sessions_of_one_principal_are_two_participants() {
        let mut r = Roster::default();
        r.refresh("@a:x", "s1", vec![], 0);
        r.refresh("@a:x", "s2", vec![], 0);
        assert_eq!(r.present(1).len(), 2);
    }

    #[test]
    fn expire_forgets_the_gone() {
        let mut r = Roster::default();
        r.refresh("@a:x", "s1", vec![], 0);
        r.refresh("@b:x", "s1", vec![], 14_000);
        r.expire(16_000);
        assert_eq!(r.len(), 1);
        assert_eq!(r.present(16_000)[0].principal, "@b:x");
    }
}
