//! The conversations behind the rows: expanding and collapsing a thread,
//! asking the cache about threads, and gathering a row's whole
//! conversation for the reader.

use super::threading::{
    heads_its_row, source_threads_of, thread_inputs, thread_slot, SourceThreads, THREADS_OFF_MAIN,
};
use super::types::{MessageListInput, MessageListOutput};
use super::MessageList;
use crate::models::Message;
use crate::ui::message_row::{RowData, RowMeta};
use relm4::prelude::*;
use std::rc::Rc;

impl MessageList {
    /// After a rebuild: a lone selected conversation head whose thread has
    /// gained members since it was opened (a reply just synced in) is
    /// reported, so the reader can show the new message without a
    /// re-selection. Members lost (deleted elsewhere) are left to the
    /// vanish handling.
    pub(super) fn report_thread_growth(&mut self, sender: &ComponentSender<Self>) {
        if !self.threading || self.selected_ids.len() != 1 {
            return;
        }
        let Some(key) = self.selected_id else { return };
        let Some(m) = self
            .shown
            .iter()
            .find(|m| (m.account_id, m.id) == key)
            .map(|m| Message::clone(m))
        else {
            return;
        };
        let (thread, solo) = self.conversation_for(&m);
        if solo || thread.len() <= 1 {
            return;
        }
        let keys: Vec<(u32, u32)> = thread.iter().map(|t| (t.account_id, t.id)).collect();
        let grew = keys.len() > self.emitted_thread.len()
            && self.emitted_thread.iter().all(|k| keys.contains(k));
        if grew {
            self.emitted_thread = keys;
            let _ = sender.output(MessageListOutput::ThreadGrew { message: m, thread });
        }
    }

    /// A message's read or starred state changed: recompute what its
    /// conversation's head row says, so a collapsed thread's highlight
    /// clears exactly when its last unread message is read.
    ///
    /// As [`MessageList::describe_group`] does, only this folder's own
    /// messages count, and they are found by slot. Going through the
    /// (account, UID) thread maps let a message in another folder (the reply
    /// you sent, filed in Sent) stand in for an unread one here that shared
    /// its UID, and the row stayed bold after it was read (#333).
    pub(super) fn refresh_thread_head(&mut self, slot: (u32, u32, u32)) {
        let Some(own) = self
            .groups
            .values()
            .find(|g| g.iter().any(|m| thread_slot(m) == slot))
            .cloned()
        else {
            return;
        };
        let head = thread_slot(&own[0]);
        let Some(idx) = self.shown.iter().position(|m| thread_slot(m) == head) else {
            return;
        };
        // A row that stands for one message shows that message's own state.
        if self
            .shared
            .model
            .row(idx)
            .is_none_or(|r| r.meta.key.is_none())
        {
            return;
        }
        let now = |m: &Rc<Message>| -> Rc<Message> {
            let s = thread_slot(m);
            self.all
                .iter()
                .find(|a| thread_slot(a) == s)
                .cloned()
                .unwrap_or_else(|| m.clone())
        };
        let unread = own.iter().any(|m| now(m).unread);
        let starred = own.iter().any(|m| now(m).starred);
        let newest = own.last().map(now);
        let (answered, forwarded) = newest
            .map(|m| (m.answered, m.forwarded))
            .unwrap_or_default();
        self.shared.model.update_row(idx, |d| {
            d.meta.unread = unread;
            d.meta.starred = starred;
            d.meta.answered = answered;
            d.meta.forwarded = forwarded;
        });
    }

    /// Begin collapsing a thread: slide its replies shut (they stay where
    /// they are in the list — only their own Revealer closes), then drop them
    /// once that animation has finished. Dropping right away — before the
    /// replies have shrunk — would just make them disappear outright. (PR #79)
    pub(super) fn start_collapse_thread(
        &mut self,
        key: (u32, String),
        sender: &ComponentSender<Self>,
    ) {
        if let Some(members) = self.thread_members.get(&key).cloned() {
            // The head survives the toggle (only its replies are removed),
            // so its own chevron can rotate shut in place, in step with the
            // replies sliding closed beneath it.
            if let Some(&head_key) = members.first() {
                if let Some(idx) = self
                    .shown
                    .iter()
                    .position(|m| (m.account_id, m.id) == head_key)
                {
                    self.shared
                        .model
                        .update_row(idx, |d| d.meta.expanded = false);
                }
            }
            // `members` is head-first (see `rebuild`); only the replies
            // beneath it animate closed.
            for child_key in members.iter().skip(1) {
                if let Some(idx) = self
                    .shown
                    .iter()
                    .position(|m| (m.account_id, m.id) == *child_key)
                {
                    self.shared
                        .model
                        .update_row(idx, |d| d.meta.revealed = false);
                }
            }
        }
        if let Some(old) = self.collapsing_threads.remove(&key) {
            old.remove();
        }
        let s = sender.clone();
        let timer_key = key.clone();
        // Matches the Revealer's own transition duration, so the rows are
        // fully closed by the time they're actually dropped from the list.
        let timer =
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                s.input(MessageListInput::FinishCollapseThread(timer_key));
            });
        self.collapsing_threads.insert(key, timer);
    }

    /// Put a thread's replies in right after its head, without touching any
    /// other row — the counterpart to `collapse_thread_rows`. Each new row
    /// slides open the first time it is shown (see `RowMeta::appear`).
    /// (PR #79)
    pub(super) fn expand_thread(&mut self, key: &(u32, String)) {
        let Some(members) = self.thread_members.get(key).cloned() else {
            return;
        };
        let Some(&head_key) = members.first() else {
            return;
        };
        let Some(head_pos) = self
            .shown
            .iter()
            .position(|m| (m.account_id, m.id) == head_key)
        else {
            return;
        };
        // `members` is already oldest-first (see `rebuild`).
        let children: Vec<Rc<Message>> = {
            let source = self.active_source();
            members
                .iter()
                .skip(1)
                .filter_map(|k| {
                    source
                        .iter()
                        .find(|m| (m.account_id, m.id) == *k)
                        .or_else(|| self.nested.get(k))
                        .cloned()
                })
                .collect()
        };
        if children.is_empty() {
            return;
        }
        // The head survives the toggle (only its replies are put in), so its
        // own chevron can rotate open in place.
        self.shared
            .model
            .update_row(head_pos, |d| d.meta.expanded = true);
        let section = self
            .shared
            .model
            .row(head_pos)
            .map(|r| r.section)
            .unwrap_or_default();
        let n = children.len();
        let rows: Vec<Rc<RowData>> = children
            .iter()
            .enumerate()
            .map(|(i, m)| {
                Rc::new(RowData {
                    msg: m.clone(),
                    meta: RowMeta {
                        is_child: true,
                        is_last: i + 1 == n,
                        revealed: true,
                        appear: true,
                        ..Default::default()
                    },
                    section,
                })
            })
            .collect();
        self.shared.model.splice(head_pos + 1, 0, rows);
        self.shown.splice(head_pos + 1..head_pos + 1, children);
    }

    /// Drop a thread's reply rows (already slid shut by `start_collapse_thread`)
    /// without touching any other row — the counterpart to `expand_thread`.
    /// (PR #79)
    pub(super) fn collapse_thread_rows(&mut self, key: &(u32, String)) {
        let Some(members) = self.thread_members.get(key).cloned() else {
            return;
        };
        let mut indices: Vec<usize> = members
            .iter()
            .skip(1)
            .filter_map(|k| self.shown.iter().position(|m| (m.account_id, m.id) == *k))
            .collect();
        indices.sort_unstable();
        // Back to front, so earlier removals don't shift the indices still
        // to come.
        for &idx in indices.iter().rev() {
            self.shown.remove(idx);
            self.shared.model.splice(idx, 1, Vec::new());
        }
    }

    /// The source changed: its conversations are to be worked out again,
    /// and an answer still on its way from a background thread is stale.
    pub(super) fn drop_threads(&self) {
        self.source_threads.take();
        // A message's address says which it is only while the list lasts:
        // the next one may be put where an old one was.
        self.search_text.borrow_mut()[0] = None;
        self.threads_gen.set(self.threads_gen.get() + 1);
    }

    /// For a large list whose conversations are not worked out yet: start
    /// that on a background thread and answer `true`, the rebuild to wait
    /// for [`MessageListInput::ThreadsReady`]. The rows on screen stay
    /// meanwhile, and the main loop stays free.
    pub(super) fn threads_pending(&self) -> bool {
        let pool = self.searching_pool();
        if self
            .source_threads
            .borrow()
            .as_ref()
            .is_some_and(|t| t.pool == pool)
        {
            return false;
        }
        let source = self.active_source();
        if source.len() < THREADS_OFF_MAIN {
            return false;
        }
        let gen = self.threads_gen.get();
        if self.threads_job.get() == Some(gen) {
            return true;
        }
        self.threads_job.set(Some(gen));
        let input = thread_inputs(source);
        let links = self.thread_links.clone();
        let sender = self.input.clone();
        std::thread::spawn(move || {
            let t = std::time::Instant::now();
            let threads = source_threads_of(&input, &links, pool);
            tracing::debug!(
                "list: {} messages threaded off the main loop in {:?}",
                input.len(),
                t.elapsed()
            );
            let _ = sender.send(MessageListInput::ThreadsReady {
                gen,
                threads: Box::new(threads),
            });
        });
        true
    }

    /// The conversations of `active_source`, from the kept copy while the
    /// source it was worked out from is still the one in use.
    pub(super) fn source_threads(&self) -> std::rc::Rc<SourceThreads> {
        let pool = self.searching_pool();
        if let Some(t) = self
            .source_threads
            .borrow()
            .as_ref()
            .filter(|t| t.pool == pool)
        {
            return t.clone();
        }
        let t = std::rc::Rc::new(source_threads_of(
            self.active_source(),
            &self.thread_links,
            pool,
        ));
        *self.source_threads.borrow_mut() = Some(t.clone());
        t
    }

    /// The shown row (a thread head) whose conversation holds the message
    /// `key`, when `key` is in the index but has no row of its own.
    pub(super) fn thread_head_for(&self, key: (u32, u32)) -> Option<(u32, u32)> {
        if !self.threading {
            return None;
        }
        let m = self
            .active_source()
            .iter()
            .find(|m| (m.account_id, m.id) == key)?;
        let threads = self.source_threads();
        let thread = threads.keys.get(&thread_slot(m))?;
        self.shown
            .iter()
            .find(|m| threads.keys.get(&thread_slot(m)) == Some(thread))
            .map(|m| (m.account_id, m.id))
    }

    /// The conversation a row stands for, or empty when the row means only its
    /// own message. A row stands for its thread when it is the head of one and
    /// the thread is not currently opened out in the list — which, with
    /// expandable conversations off, it never is.
    pub(super) fn row_conversation(&self, m: &Message) -> Vec<Message> {
        let members = self.thread_members(m);
        if members.is_empty()
            || !heads_its_row((m.account_id, m.id), &self.msg_thread, &self.thread_members)
        {
            return Vec::new();
        }
        let expanded = self.thread_expansion
            && self
                .msg_thread
                .get(&(m.account_id, m.id))
                .is_some_and(|key| self.expanded_threads.contains(key) != self.default_expanded);
        if expanded {
            Vec::new()
        } else {
            members
        }
    }

    /// Every on-screen member of `m`'s conversation (oldest first) — from any
    /// member, head or reply. Empty when threading is off or `m` stands alone,
    /// so callers can treat non-empty as "this is a real thread".
    pub(super) fn thread_members(&self, m: &Message) -> Vec<Message> {
        if !self.threading {
            return Vec::new();
        }
        let mut members = self.source_members(m);
        if members.len() <= 1 {
            return Vec::new();
        }
        members.sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then(a.uid.cmp(&b.uid)));
        members
    }

    /// Everything in `active_source` that shares `m`'s conversation, `m`
    /// included, in source order. Empty when `m` is not in the source.
    fn source_members(&self, m: &Message) -> Vec<Message> {
        let threads = self.source_threads();
        let source = self.active_source();
        threads
            .keys
            .get(&thread_slot(m))
            .and_then(|key| threads.members.get(key))
            .map(|idx| {
                idx.iter()
                    .filter_map(|&i| source.get(i))
                    .map(|m| Message::clone(m))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The conversation to show for a selected message: when `m` is the oldest
    /// (head) of a multi-message thread, every message in it (oldest first);
    /// otherwise just `m` (so opening an individual reply shows only that one).
    pub(super) fn conversation_for(&self, m: &Message) -> (Vec<Message>, bool) {
        if !self.threading {
            return (vec![m.clone()], false);
        }
        // A part of the conversation from another folder, picked out of the
        // opened-out row: shown by itself, like any reply picked out (#309).
        if self.nested.contains_key(&(m.account_id, m.id)) {
            return (vec![m.clone()], true);
        }
        // Thread within whatever set is on screen (the search pool while searching,
        // otherwise the current folder) so the conversation matches the rows shown.
        let mut members = self.source_members(m);
        if members.len() <= 1 {
            // Nothing else here to thread with. It may still have siblings in
            // another folder, so this is *not* solo — the reader may look.
            return (vec![m.clone()], false);
        }
        // Oldest first, matching the rows: the head is the message that opened
        // the conversation, and opening it shows the whole thread in order.
        members.sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then(a.uid.cmp(&b.uid)));
        // Whether `m` is the row, judged over the window the rows came from
        // (#236): a conversation whose start lies past the rendered window is
        // still one row, and that row stands for all of it.
        if heads_its_row((m.account_id, m.id), &self.msg_thread, &self.thread_members) {
            (members, false)
        } else {
            // A reply picked out of a conversation that is on screen: show it by
            // itself and leave it that way.
            (vec![m.clone()], true)
        }
    }
}
