// session.rs — multi-window and document session restore policy.
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Serializable state for an application session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionState {
    pub open_files: Vec<PathBuf>,
    pub active_index: usize,
    pub window_width: i32,
    pub window_height: i32,
    pub is_maximized: bool,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            open_files: Vec::new(),
            active_index: 0,
            window_width: 960,
            window_height: 640,
            is_maximized: false,
        }
    }

    /// Add an open file to the session state.
    pub fn add_file(&mut self, path: PathBuf) {
        if !self.open_files.contains(&path) {
            self.open_files.push(path);
        }
    }

    /// Remove a closed file from the session state.
    pub fn remove_file(&mut self, path: &Path) {
        self.open_files.retain(|p| p != path);
        if self.active_index >= self.open_files.len() && !self.open_files.is_empty() {
            self.active_index = self.open_files.len() - 1;
        }
    }

    /// Filter session files to only existing paths.
    pub fn prune_missing(&mut self) {
        self.open_files.retain(|p| p.exists());
        if self.active_index >= self.open_files.len() {
            self.active_index = self.open_files.len().saturating_sub(1);
        }
    }

    /// Serialize session to JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize session from JSON string.
    pub fn from_json(json_str: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_state_lifecycle() {
        let mut session = SessionState::new();
        session.add_file(PathBuf::from("/tmp/doc1.odt"));
        session.add_file(PathBuf::from("/tmp/doc2.odt"));
        assert_eq!(session.open_files.len(), 2);

        session.remove_file(Path::new("/tmp/doc1.odt"));
        assert_eq!(session.open_files.len(), 1);
        assert_eq!(session.open_files[0], PathBuf::from("/tmp/doc2.odt"));

        let json = session.to_json().expect("serialize");
        let restored = SessionState::from_json(&json).expect("deserialize");
        assert_eq!(restored, session);
    }
}
