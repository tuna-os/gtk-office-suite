//! Changes made on screen before the server confirms them (read marks, tags,
//! moves), and how long each is trusted over what a sync reports.

use crate::models::Message;

use super::model::AppModel;

/// How long a read/unread change sent to a worker keeps overriding what the
/// server reports for its message and folder, should the worker's
/// confirmation never come (a dropped connection mid-request). A worker
/// busy with a large sync can take well over 20 seconds to reach a mark,
/// and read mail used to turn unread again in the meantime (#255).
pub(super) const PENDING_SEEN_MAX: std::time::Duration = std::time::Duration::from_secs(60);

/// The same for a tag, which the worker confirms: a confirmed tag still
/// wins for a few seconds over a list another connection fetched before
/// the STORE, and an unconfirmed one for as long as a busy queue can take.
pub(super) const PENDING_TAG_MAX: std::time::Duration = std::time::Duration::from_secs(300);

pub(super) const PENDING_TAG_SETTLED: std::time::Duration = std::time::Duration::from_secs(10);

/// The same for mail taken out of a folder (moved, deleted, marked as
/// spam): how long it stays off that folder's list without the worker's
/// word that the move has run.
pub(super) const PENDING_MOVE_MAX: std::time::Duration = std::time::Duration::from_secs(120);

impl AppModel {
    /// Whether mail taken out of `path` is still waiting on its move.
    pub(super) fn pending_moves_in(&self, account_id: u32, path: &str) -> bool {
        self.pending_moves
            .borrow()
            .iter()
            .any(|((a, p, _), at)| *a == account_id && p == path && at.elapsed() < PENDING_MOVE_MAX)
    }

    /// A list from the cache or a server with what this session changed
    /// and the server may not reflect yet laid over it: local tags, then
    /// the pending read marks, answered marks and tags.
    pub(super) fn overlay_local_state(
        &mut self,
        account_id: u32,
        folder_id: u32,
        messages: Vec<Message>,
    ) -> Vec<Message> {
        let messages = self.merge_local_tags(account_id, messages);
        let messages = self.apply_pending_seen(account_id, folder_id, messages);
        let messages = self.apply_pending_answers(account_id, folder_id, messages);
        self.apply_pending_tags(account_id, folder_id, messages)
    }

    /// Take the mail whose move is still on its way off a folder list the
    /// worker fetched before running it.
    pub(super) fn drop_pending_moves(
        &self,
        account_id: u32,
        folder_id: u32,
        mut messages: Vec<Message>,
    ) -> Vec<Message> {
        let Some(path) = self.folder_path(account_id, folder_id) else {
            return messages;
        };
        if !self.pending_moves_in(account_id, &path) {
            return messages;
        }
        let pending = self.pending_moves.borrow();
        messages.retain(|m| {
            pending
                .get(&(account_id, path.clone(), m.uid))
                .is_none_or(|at| at.elapsed() >= PENDING_MOVE_MAX)
        });
        messages
    }

    /// Whether a read/unread change for a message in `path` is still on its
    /// way to the server (and not so old that the worker must have lost it).
    pub(super) fn pending_seen_in(&self, account_id: u32, path: &str) -> bool {
        self.pending_seen.iter().any(|((a, p, _), (_, at))| {
            *a == account_id && p == path && at.elapsed() < PENDING_SEEN_MAX
        })
    }

    pub(super) fn pending_seen_in_folder(&self, account_id: u32, folder_id: u32) -> bool {
        self.folder_by_id(account_id, folder_id)
            .is_some_and(|f| self.pending_seen_in(account_id, &f.path))
    }

    /// Overlay the read/unread changes still on their way to the server on
    /// a folder list the worker fetched before storing them.
    pub(super) fn apply_pending_seen(
        &self,
        account_id: u32,
        folder_id: u32,
        mut messages: Vec<Message>,
    ) -> Vec<Message> {
        let Some(path) = self.folder_path(account_id, folder_id) else {
            return messages;
        };
        if !self.pending_seen_in(account_id, &path) {
            return messages;
        }
        for m in messages.iter_mut() {
            if let Some((seen, at)) = self.pending_seen.get(&(account_id, path.clone(), m.uid)) {
                if at.elapsed() < PENDING_SEEN_MAX {
                    m.unread = !seen;
                }
            }
        }
        messages
    }

    /// [`apply_pending_seen`] for tags: a list fetched before a tag reached
    /// the server keeps the tag as the user just set it.
    pub(super) fn apply_pending_tags(
        &mut self,
        account_id: u32,
        folder_id: u32,
        mut messages: Vec<Message>,
    ) -> Vec<Message> {
        let now = std::time::Instant::now();
        self.pending_tags.retain(|_, (_, until, _)| *until > now);
        if !self
            .pending_tags
            .keys()
            .any(|(a, _, _, _)| *a == account_id)
        {
            return messages;
        }
        let Some(path) = self.folder_path(account_id, folder_id) else {
            return messages;
        };
        for ((a, p, uid, _), (on, _, kw)) in &self.pending_tags {
            if *a != account_id || *p != path {
                continue;
            }
            for m in messages.iter_mut().filter(|m| m.uid == *uid) {
                if self.carries_tag(m, kw) != *on {
                    match self
                        .tags
                        .iter()
                        .find(|t| t.keyword.eq_ignore_ascii_case(kw))
                    {
                        Some(t) if !*on => m.drop_tag(t),
                        _ => m.set_keyword(kw, *on),
                    }
                }
            }
        }
        messages
    }

    /// Fold an account's locally-kept tags (POP3, keyword-less servers) into
    /// summaries fresh from its worker, so they show like server-side ones.
    pub(super) fn merge_local_tags(
        &self,
        account_id: u32,
        mut messages: Vec<Message>,
    ) -> Vec<Message> {
        if let Some(cache) = self.cache.as_ref() {
            cache.apply_local_tags(account_id, &mut messages);
        }
        messages
    }
}
