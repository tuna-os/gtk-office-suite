// recent.rs — privacy-aware MRU document manager.
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A recently accessed document record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentItem {
    pub path: PathBuf,
    pub title: String,
    pub last_opened_secs: i64,
}

/// In-memory and serializable manager for recent documents with privacy controls.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentManager {
    pub items: Vec<RecentItem>,
    pub max_items: usize,
}

impl RecentManager {
    pub fn new(max_items: usize) -> Self {
        Self {
            items: Vec::new(),
            max_items: max_items.max(1),
        }
    }

    /// Record a file access. Existing entry for `path` is updated and moved to the front.
    pub fn record(&mut self, path: PathBuf, title: Option<String>) {
        let title = title.unwrap_or_else(|| {
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
                .to_string()
        });

        // Remove any prior entry for the same path
        self.items.retain(|item| item.path != path);

        let now = chrono::Utc::now().timestamp();
        self.items.insert(
            0,
            RecentItem {
                path,
                title,
                last_opened_secs: now,
            },
        );

        if self.items.len() > self.max_items {
            self.items.truncate(self.max_items);
        }
    }

    /// Remove an entry by path (e.g. user deleted or requested removal).
    pub fn remove(&mut self, path: &Path) -> bool {
        let initial_len = self.items.len();
        self.items.retain(|item| item.path != path);
        self.items.len() < initial_len
    }

    /// Clear all recent document history (privacy wipe).
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Filter to only items whose files currently exist on disk.
    pub fn existing_items(&self) -> Vec<RecentItem> {
        self.items
            .iter()
            .filter(|item| item.path.exists())
            .cloned()
            .collect()
    }

    /// Prune missing files from the list.
    pub fn prune_missing(&mut self) {
        self.items.retain(|item| item.path.exists());
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(json_str: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_lru() {
        let mut mgr = RecentManager::new(3);
        mgr.record(PathBuf::from("/tmp/doc1.md"), None);
        mgr.record(PathBuf::from("/tmp/doc2.md"), None);
        mgr.record(PathBuf::from("/tmp/doc3.md"), None);
        assert_eq!(mgr.items.len(), 3);
        assert_eq!(mgr.items[0].path, PathBuf::from("/tmp/doc3.md"));

        // Adding 4th drops oldest
        mgr.record(PathBuf::from("/tmp/doc4.md"), None);
        assert_eq!(mgr.items.len(), 3);
        assert_eq!(mgr.items[0].path, PathBuf::from("/tmp/doc4.md"));
        assert_eq!(mgr.items[2].path, PathBuf::from("/tmp/doc2.md"));

        // Re-recording doc2 moves it to front
        mgr.record(PathBuf::from("/tmp/doc2.md"), None);
        assert_eq!(mgr.items[0].path, PathBuf::from("/tmp/doc2.md"));
        assert_eq!(mgr.items.len(), 3);
    }

    #[test]
    fn test_privacy_clear() {
        let mut mgr = RecentManager::new(5);
        mgr.record(PathBuf::from("/tmp/secret.docx"), Some("Secret".into()));
        assert_eq!(mgr.items.len(), 1);
        mgr.clear();
        assert!(mgr.items.is_empty());
    }

    #[test]
    fn test_remove_item() {
        let mut mgr = RecentManager::new(5);
        mgr.record(PathBuf::from("/tmp/a.md"), None);
        mgr.record(PathBuf::from("/tmp/b.md"), None);
        assert!(mgr.remove(Path::new("/tmp/a.md")));
        assert_eq!(mgr.items.len(), 1);
        assert_eq!(mgr.items[0].path, PathBuf::from("/tmp/b.md"));
    }
}
