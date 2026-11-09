//! Small types shared across the app's modules.

use crate::models::{FolderKind, Message};

/// A folder's messages as [`AppModel::message_cache`] keeps them: shared,
/// not copied, with a search pool being put together on another thread
/// (#515). A change copies the folder only while such a pool still holds it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct FolderMessages(std::sync::Arc<Vec<Message>>);

impl std::ops::Deref for FolderMessages {
    type Target = Vec<Message>;
    fn deref(&self) -> &Vec<Message> {
        &self.0
    }
}

impl std::ops::DerefMut for FolderMessages {
    fn deref_mut(&mut self) -> &mut Vec<Message> {
        std::sync::Arc::make_mut(&mut self.0)
    }
}

impl<'a> IntoIterator for &'a FolderMessages {
    type Item = &'a Message;
    type IntoIter = std::slice::Iter<'a, Message>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a mut FolderMessages {
    type Item = &'a mut Message;
    type IntoIter = std::slice::IterMut<'a, Message>;
    fn into_iter(self) -> Self::IntoIter {
        std::sync::Arc::make_mut(&mut self.0).iter_mut()
    }
}

impl From<Vec<Message>> for FolderMessages {
    fn from(messages: Vec<Message>) -> Self {
        Self(std::sync::Arc::new(messages))
    }
}

/// The currently selected mailbox.
#[derive(Clone)]
pub(super) struct SelectedFolder {
    pub(super) account_id: u32,
    pub(super) folder_id: u32,
    pub(super) path: String,
}

/// Which folders a unified view merges (see `AppModel::unified_view`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnifiedView {
    /// Every account's folder of this kind.
    Kind(FolderKind),
    /// Every filter rule's folder listed under All Inboxes.
    Filtered,
    /// Every favorite folder.
    Favorites,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::msg;

    /// #515: a search pool shares a folder rather than copying it, and a
    /// change made while it does reaches only the app's own copy.
    #[test]
    fn a_shared_folder_is_copied_only_when_changed() {
        let mut folder = FolderMessages::from(vec![msg(1, 1, 10), msg(1, 1, 11)]);
        let pool_copy = folder.clone();
        assert!(std::sync::Arc::ptr_eq(&folder.0, &pool_copy.0));
        folder[0].unread = !pool_copy[0].unread;
        assert!(!std::sync::Arc::ptr_eq(&folder.0, &pool_copy.0));
        assert_ne!(folder[0].unread, pool_copy[0].unread);
        for m in &mut folder {
            m.starred = true;
        }
        assert!(folder.iter().all(|m| m.starred) && !pool_copy.iter().any(|m| m.starred));
    }
}
