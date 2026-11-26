//! Conversations: thread keys computed off the main thread, the cache's
//! answers about threads, and expanding or collapsing one.

use super::threading::{unasked_threads, SourceThreads};
use super::types::MessageListOutput;
use super::MessageList;
use crate::models::ThreadSummary;
use relm4::prelude::*;

impl MessageList {
    pub(super) fn on_threads_ready(&mut self, gen: u64, threads: SourceThreads) {
        if self.threads_job.get() == Some(gen) {
            self.threads_job.set(None);
        }
        if gen == self.threads_gen.get() && threads.pool == self.searching_pool() {
            *self.source_threads.borrow_mut() = Some(std::rc::Rc::new(threads));
            self.queue_rebuild(true);
        }
    }

    pub(super) fn on_ask_threads(&mut self, sender: &ComponentSender<Self>) {
        // Only the conversations rows have come to show, and only once
        // each: the answer arrives as a refresh of those rows, so
        // asking again on the strength of it would never settle.
        let wanted = self.shared.take_wanted();
        if !self.threading {
            return;
        }
        let mut listed: Vec<(u32, String, Vec<String>)> = Vec::new();
        for key in wanted {
            if listed.iter().any(|(a, r, _)| (*a, r) == (key.0, &key.1)) {
                continue;
            }
            let Some(members) = self.groups.get(&key) else {
                continue;
            };
            let ids = crate::models::thread_ids(members.iter().map(|m| &**m));
            if !ids.is_empty() {
                listed.push((key.0, key.1, ids));
            }
        }
        let fresh = unasked_threads(&listed, &self.asked_threads);
        if !fresh.is_empty() {
            for (aid, root, _) in &fresh {
                self.asked_threads.insert((*aid, root.clone()));
            }
            let _ = sender.output(MessageListOutput::ThreadsListed { groups: fresh });
        }
    }

    pub(super) fn on_set_thread_links(&mut self, links: Vec<(u32, String, String)>) {
        if self.thread_links != links {
            self.thread_links = links;
            self.drop_threads();
            if self.threading {
                self.queue_rebuild(true);
            }
        }
    }

    pub(super) fn on_set_thread_summaries(
        &mut self,
        summaries: Vec<((u32, String), ThreadSummary)>,
    ) {
        // They arrive a beat after the rows that asked are shown (the
        // cache is the worker's, not ours). Only what those rows say
        // moves; which conversations are listed does not — which is
        // what keeps this from asking again and looping.
        let mut changed = Vec::new();
        for (key, summary) in summaries {
            if self.thread_summaries.get(&key) != Some(&summary) {
                self.thread_summaries.insert(key.clone(), summary);
                changed.push(key);
            }
        }
        if !changed.is_empty() && self.threading {
            // A rebuild already on its way reads them anyway.
            if self.rebuild_queued.is_none() {
                self.apply_summaries(changed);
            }
        }
    }

    pub(super) fn on_recheck_thread_summaries(&mut self, account_id: u32) {
        // The summaries are kept, not dropped: dropping them turned
        // every conversation row on screen back into its folder-only
        // self, and a row kept across the rebuild is not bound again,
        // so it never asked again either (#330). The answers replace
        // whatever has changed.
        self.asked_threads.retain(|(aid, _)| *aid != account_id);
        self.recheck_accounts.insert(account_id);
        if self.rebuild_queued.is_none() {
            self.recheck_bound_rows();
        }
    }

    pub(super) fn on_conversation_size(&mut self, account_id: u32, id: u32, size: usize) {
        if !self.threading || size < 2 {
            return;
        }
        let key = (account_id, id);
        let row = self
            .shown
            .iter()
            .position(|m| (m.account_id, m.id) == key)
            .or_else(|| {
                let head = self.thread_head_for(key)?;
                self.shown.iter().position(|m| (m.account_id, m.id) == head)
            })
            .and_then(|pos| self.shared.model.row(pos));
        let Some(row) = row.filter(|r| r.meta.count < size) else {
            return;
        };
        if let Some(group) = row.meta.group.clone() {
            self.asked_threads.remove(&group);
            self.shared.want(group);
        }
    }

    pub(super) fn on_set_thread_row_newest(&mut self, on: bool) {
        if self.thread_row_newest != on {
            self.thread_row_newest = on;
            if self.threading {
                self.queue_rebuild(true);
            }
        }
    }

    pub(super) fn on_set_own_addresses(&mut self, addresses: std::collections::HashSet<String>) {
        if self.own_addresses != addresses {
            self.own_addresses = addresses;
            if self.threading && self.thread_expansion && !self.thread_row_newest {
                self.queue_rebuild(true);
            }
        }
    }

    pub(super) fn on_set_threading(&mut self, on: bool) {
        if self.threading != on {
            self.threading = on;
            self.rebuild();
            self.scroll_top();
        }
    }

    pub(super) fn on_set_thread_expansion(&mut self, on: bool) {
        if self.thread_expansion != on {
            self.thread_expansion = on;
            // Turning expansion off folds every open thread (the
            // `expanded` computation ignores the stored toggles while
            // off); turning it on restores them.
            self.sync_look();
            self.rebuild();
        }
    }

    pub(super) fn on_set_threads_expanded(&mut self, on: bool) {
        if self.default_expanded != on {
            self.default_expanded = on;
            // Per-thread toggles were exceptions to the old default;
            // drop them so everything follows the new one.
            self.expanded_threads.clear();
            self.rebuild();
            self.scroll_top();
        }
    }

    pub(super) fn on_toggle_thread(&mut self, key: (u32, String), sender: &ComponentSender<Self>) {
        // Expansion disabled: the chevron stays but does nothing — the
        // conversation is read through the reader's cards.
        if !self.thread_expansion {
            return;
        }
        let was_expanded = self.expanded_threads.contains(&key) != self.default_expanded;
        if was_expanded {
            // Slide the replies shut in place; the list only drops
            // them once that animation has actually finished (see
            // `start_collapse_thread`) — dropping them right away
            // would just make them vanish instead of collapsing.
            self.start_collapse_thread(key, sender);
        } else {
            if !self.expanded_threads.remove(&key) {
                self.expanded_threads.insert(key.clone());
            }
            // Insert just this thread's replies rather than rebuilding
            // the whole list — on a long list a full rebuild tears
            // down and recreates every row (up to RENDER_CAP), which
            // stutters right as the reveal animation is trying to run.
            self.expand_thread(&key);
        }
    }

    pub(super) fn on_finish_collapse_thread(&mut self, key: (u32, String)) {
        self.collapsing_threads.remove(&key);
        if !self.expanded_threads.remove(&key) {
            self.expanded_threads.insert(key.clone());
        }
        // Same reasoning as `expand_thread`: drop just these rows.
        self.collapse_thread_rows(&key);
    }
}
