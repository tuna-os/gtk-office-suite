//! Privacy-aware recent-document list policy shared by all applications.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentList {
    entries: Vec<String>,
    limit: usize,
}

impl RecentList {
    pub fn new(entries: impl IntoIterator<Item = String>, limit: usize) -> Self {
        let mut list = Self { entries: Vec::new(), limit };
        for entry in entries { list.record(entry); }
        list
    }

    pub fn entries(&self) -> &[String] { &self.entries }

    pub fn record(&mut self, uri: String) {
        if uri.trim().is_empty() || self.limit == 0 { return; }
        self.entries.retain(|old| old != &uri);
        self.entries.insert(0, uri);
        self.entries.truncate(self.limit);
    }

    pub fn remove(&mut self, uri: &str) { self.entries.retain(|entry| entry != uri); }
    pub fn clear(&mut self) { self.entries.clear(); }
    pub fn retain_existing(&mut self, exists: impl Fn(&str) -> bool) {
        self.entries.retain(|entry| exists(entry));
    }
}

#[cfg(test)]
mod tests {
    use super::RecentList;

    #[test]
    fn records_most_recent_first_and_deduplicates() {
        let mut list = RecentList::new([], 3);
        list.record("file:///a".into());
        list.record("file:///b".into());
        list.record("file:///a".into());
        assert_eq!(list.entries(), &["file:///a", "file:///b"]);
    }

    #[test]
    fn caps_entries_and_supports_privacy_clear() {
        let mut list = RecentList::new([], 2);
        list.record("file:///a".into());
        list.record("file:///b".into());
        list.record("file:///c".into());
        assert_eq!(list.entries(), &["file:///c", "file:///b"]);
        list.clear();
        assert!(list.entries().is_empty());
    }

    #[test]
    fn prunes_missing_files_without_reordering() {
        let mut list = RecentList::new(
            ["file:///a".into(), "file:///b".into(), "file:///c".into()], 10,
        );
        list.retain_existing(|uri| uri != "file:///b");
        assert_eq!(list.entries(), &["file:///c", "file:///a"]);
    }
}
