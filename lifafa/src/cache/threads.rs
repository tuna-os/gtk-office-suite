//! Conversations across folders: member lookups, the per-thread counts
//! and the backfills of References and Gmail labels.

use std::collections::{HashMap, HashSet};

use rusqlite::params;

use crate::models::{FolderKind, Message, ThreadLatest, ThreadSummary};

use super::{message_columns, message_from_row, Cache};

/// Rowids written into a query as a literal list: they are integers, and a
/// conversation can match more rows than a statement takes bound values.
fn rowid_list(rowids: &[i64]) -> String {
    rowids
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Most Message-IDs one cross-folder conversation lookup matches against. A
/// thread's ancestry is short in practice, and the references half of that query
/// is a scan — this keeps a pathological References header (some mailing lists
/// accumulate hundreds) from turning a message-open into a long one.
const THREAD_ID_LIMIT: usize = 24;

/// How many messages one conversation may pull in from other folders. A
/// conversation is rendered as a single document with every member's body
/// inlined, so this bounds both the fetching and the rendering.
const THREAD_MEMBER_LIMIT: usize = 100;

/// Most Message-IDs one *batched* conversation count matches against, across
/// every thread on the page put together (#222). The references half of the
/// lookup is a scan, and batching exists precisely so a page of threads costs
/// one scan instead of fifty; the cap bounds how much a page of long
/// mailing-list threads can pull in. Threads past the cap keep their
/// folder-local count.
const THREAD_COUNT_ID_LIMIT: usize = 256;

/// Most rows one batched conversation count inspects. Counting is per-thread,
/// so this is the whole page's budget, not one thread's.
const THREAD_COUNT_ROW_LIMIT: i64 = 5_000;

impl Cache {
    /// The rows of the account's messages that are one of `ids` or name one
    /// of them in their References: what both conversation lookups match on.
    ///
    /// The own-id half is answered by the index. The References half is a
    /// scan either way, but it is one pass over the column with every token
    /// looked up in a set. It used to be SQL, an `instr` per id on every
    /// row, which cost rows times ids: four seconds for one page's badges
    /// over a 95,000-message Gmail cache, on the cache lane that every
    /// cached body waits behind (#259).
    fn thread_member_rowids(&self, account_id: u32, ids: &[&String]) -> rusqlite::Result<Vec<i64>> {
        let wanted: std::collections::HashSet<&str> = ids.iter().map(|i| i.as_str()).collect();
        let mut rowids: Vec<i64> = Vec::new();
        let slots = (0..ids.len())
            .map(|i| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT rowid FROM messages WHERE account_id = ?1 AND message_id IN ({slots})"
        ))?;
        let mut binds: Vec<&dyn rusqlite::ToSql> = vec![&account_id];
        for i in ids {
            binds.push(*i as &dyn rusqlite::ToSql);
        }
        let own = stmt.query_map(binds.as_slice(), |row| row.get::<_, i64>(0))?;
        for rowid in own {
            rowids.push(rowid?);
        }
        let mut stmt = self.conn.prepare_cached(
            "SELECT rowid, references_ FROM messages WHERE account_id = ?1 AND references_ <> ''",
        )?;
        let mut rows = stmt.query([account_id])?;
        while let Some(row) = rows.next()? {
            let refs = row.get_ref(1)?.as_str().unwrap_or("");
            if refs.split_whitespace().any(|r| wanted.contains(r)) {
                rowids.push(row.get(0)?);
            }
        }
        rowids.sort_unstable();
        rowids.dedup();
        Ok(rowids)
    }

    /// Every cached message across the account's folders that belongs to the same
    /// conversation as `ids` — messages naming one of those ids as their own
    /// Message-ID (an ancestor), or referencing one (a descendant, e.g. the reply
    /// you sent, filed away in Sent).
    ///
    /// Returns each message with the folder path it lives in, since the caller
    /// needs that both to label it and to fetch its body. Cache-only: the point
    /// is to assemble a conversation without going near the network (#21).
    /// A message's age is not a factor — what it answers is written in its
    /// headers whenever it was sent. [`THREAD_MEMBER_LIMIT`] caps what one
    /// conversation can drag in, which is the bound that matters: every member
    /// found is a body the reader will load and render.
    pub(crate) fn messages_by_thread_ids(
        &self,
        account_id: u32,
        ids: &[String],
    ) -> Vec<(String, Message)> {
        let ids: Vec<&String> = ids
            .iter()
            .filter(|i| !i.is_empty())
            .take(THREAD_ID_LIMIT)
            .collect();
        if ids.is_empty() {
            return Vec::new();
        }
        let run = || -> rusqlite::Result<Vec<(String, Message)>> {
            let members = self.thread_member_rowids(account_id, &ids)?;
            if members.is_empty() {
                return Ok(Vec::new());
            }
            let cols = message_columns();
            let sql = format!(
                "SELECT folder_path, {cols} \
                 FROM messages WHERE rowid IN ({members}) ORDER BY ts DESC LIMIT ?1",
                members = rowid_list(&members),
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map([THREAD_MEMBER_LIMIT as i64], |row| {
                let m = message_from_row(row, 1, account_id, 0)?;
                Ok((row.get::<_, String>(0)?, m))
            })?;
            rows.collect()
        };
        run().unwrap_or_else(|e| {
            tracing::warn!("cache messages_by_thread_ids failed: {e}");
            Vec::new()
        })
    }

    /// What each of `groups` looks like across the account's *other* folders:
    /// how many messages it really holds (#222) and which of them is the
    /// newest (#236).
    ///
    /// The list can only count what it lists: a thread sitting in the Inbox
    /// shows a badge of 2 when the conversation is 2 inbox messages and the
    /// three replies you sent, because Sent is a different folder and often
    /// isn't even loaded. For the same reason the row reads as though the
    /// other side spoke last, whatever you answered. [`messages_by_thread_ids`]
    /// is what the reader uses to fill that gap when a conversation is
    /// *opened*; this answers the same question for a whole page of threads at
    /// once, without hydrating a single `Message`.
    ///
    /// Each group is `(tag, ids)` — an opaque tag echoed back, and the
    /// Message-IDs its known members are threaded by. The answer holds one
    /// entry per group that matched anything, and counts *distinct mail*: a
    /// Gmail conversation filed under All Mail and a label is one message, not
    /// two, the same trap `dedupe_label_copies` exists for. It dedupes on the
    /// Message-ID alone where the reader also compares sender and time, so two
    /// different mails sharing an id would be counted once and shown twice —
    /// malformed, vanishingly rare, and erring low is the safe direction here
    /// (the row never shows fewer than the messages under it). Trash and Junk
    /// are left out, as they are for the reader; a draft is left out of the
    /// newest message too, since a reply you have not sent is not one the
    /// other side has heard.
    pub(crate) fn thread_summaries(
        &self,
        account_id: u32,
        groups: &[(String, Vec<String>)],
    ) -> Vec<(String, ThreadSummary)> {
        let ids = capped_thread_ids(groups);
        if ids.is_empty() || groups.is_empty() {
            return Vec::new();
        }
        let rows = self.thread_rows(account_id, &ids);
        if rows.is_empty() {
            return Vec::new();
        }
        let (hidden, unsent) = self.uncounted_folders(account_id);

        // Which groups an id belongs to, so each row is placed by lookup
        // rather than by walking every group.
        let mut owners: HashMap<&str, Vec<usize>> = HashMap::new();
        for (gi, (_, group)) in groups.iter().enumerate() {
            for id in group {
                if !id.is_empty() {
                    owners.entry(id.as_str()).or_default().push(gi);
                }
            }
        }

        // Distinct mail per group. A message with no Message-ID can't be told
        // apart from another one, so it is counted where it sits.
        let mut named: Vec<HashSet<&str>> = vec![HashSet::new(); groups.len()];
        let mut anonymous: Vec<usize> = vec![0; groups.len()];
        // The scan is newest-first, so the first row a group accepts for
        // display is its newest message and later ones are ignored.
        let mut latest: Vec<Option<&ThreadLatest>> = vec![None; groups.len()];
        for row in &rows {
            let ThreadRow {
                folder_path,
                message_id,
                references,
                latest: this,
            } = row;
            if hidden.contains(folder_path) {
                continue;
            }
            let showable = !unsent.contains(folder_path);
            let mut place = |gi: usize| {
                if message_id.is_empty() {
                    anonymous[gi] += 1;
                } else {
                    named[gi].insert(message_id.as_str());
                }
                if showable && latest[gi].is_none() {
                    latest[gi] = Some(this);
                }
            };
            let mut placed: HashSet<usize> = HashSet::new();
            for gi in owners.get(message_id.as_str()).into_iter().flatten() {
                if placed.insert(*gi) {
                    place(*gi);
                }
            }
            for r in references.split_whitespace() {
                for gi in owners.get(r).into_iter().flatten() {
                    if placed.insert(*gi) {
                        place(*gi);
                    }
                }
            }
        }

        groups
            .iter()
            .enumerate()
            .filter_map(|(gi, (tag, _))| {
                let n = named[gi].len() + anonymous[gi];
                (n > 0).then(|| {
                    (
                        tag.clone(),
                        ThreadSummary {
                            count: n,
                            latest: latest[gi].cloned(),
                            members: Vec::new(),
                        },
                    )
                })
            })
            .collect()
    }

    /// The messages matching any of `ids`, newest first and capped: the
    /// same matching as `messages_by_thread_ids`, the id itself or the id as
    /// a whole token inside someone's References.
    fn thread_rows(&self, account_id: u32, ids: &[&String]) -> Vec<ThreadRow> {
        let run = || -> rusqlite::Result<Vec<ThreadRow>> {
            let members = self.thread_member_rowids(account_id, ids)?;
            if members.is_empty() {
                return Ok(Vec::new());
            }
            let sql = format!(
            "SELECT folder_path, message_id, references_, from_name, from_addr, preview, date, ts \
             FROM messages WHERE rowid IN ({members}) ORDER BY ts DESC LIMIT ?1",
            members = rowid_list(&members),
        );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map([THREAD_COUNT_ROW_LIMIT], |row| {
                Ok(ThreadRow {
                    folder_path: row.get(0)?,
                    message_id: row.get(1)?,
                    references: row.get(2)?,
                    latest: ThreadLatest {
                        from_name: row.get(3)?,
                        from_addr: row.get(4)?,
                        preview: row.get(5)?,
                        date: row.get(6)?,
                        timestamp: row.get(7)?,
                    },
                })
            })?;
            rows.collect()
        };
        run().unwrap_or_else(|e| {
            tracing::warn!("cache thread_summaries failed: {e}");
            Vec::new()
        })
    }

    /// The folders whose mail a conversation count leaves out (Trash, Junk,
    /// Templates), and those it counts but never shows a row as (Drafts).
    fn uncounted_folders(&self, account_id: u32) -> (HashSet<String>, HashSet<String>) {
        // A conversation the reader won't show isn't one the badge should
        // promise: deleted and spam copies are dropped here as they are in
        // `related_from_cache`.
        let folders = self.load_folders(account_id);
        let hidden: HashSet<String> = folders
            .iter()
            .filter(|f| {
                matches!(
                    f.kind,
                    FolderKind::Trash | FolderKind::Junk | FolderKind::Templates
                )
            })
            .map(|f| f.path.clone())
            .collect();
        // Counted, but never the message a row is shown as: a draft is a
        // reply you are still writing.
        let unsent: HashSet<String> = folders
            .iter()
            .filter(|f| matches!(f.kind, FolderKind::Drafts))
            .map(|f| f.path.clone())
            .collect();
        (hidden, unsent)
    }

    /// The next chunk of *replies* whose threading references are still the
    /// In-Reply-To-only ones an ENVELOPE fetch gives — a single id, or none.
    ///
    /// Replies only, and that is the whole economy of this: a message that opens
    /// a conversation has no References and never will, so asking the server for
    /// its header buys nothing. On a real 71k-message cache 68k rows hold a thin
    /// reference and only 3.9k of them are replies, so targeting them turns days
    /// of fetching into minutes.
    ///
    /// "None at all" cannot be told from a message that genuinely has no
    /// References, hence the `next_uid` watermark: each pass walks strictly
    /// downwards, so a message with nothing to find is visited once and never
    /// again.
    pub(crate) fn uids_needing_references(
        &self,
        account_id: u32,
        folder_path: &str,
        below_uid: u32,
        limit: usize,
    ) -> Vec<(u32, String)> {
        let run = || -> rusqlite::Result<Vec<(u32, String)>> {
            let mut stmt = self.conn.prepare(
                "SELECT uid, references_ FROM messages \
                 WHERE account_id = ?1 AND folder_path = ?2 AND uid < ?3 \
                   AND instr(trim(references_), ' ') = 0 \
                   AND (references_ <> '' OR subject LIKE 'Re:%' OR subject LIKE 'Fwd:%' \
                        OR subject LIKE 'Fw:%' OR subject LIKE 'Aw:%' OR subject LIKE 'Sv:%') \
                 ORDER BY uid DESC LIMIT ?4",
            )?;
            let rows = stmt.query_map(
                params![account_id, folder_path, below_uid, limit as i64],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
            )?;
            rows.collect()
        };
        run().unwrap_or_default()
    }

    pub(crate) fn set_references(
        &self,
        account_id: u32,
        folder_path: &str,
        uid: u32,
        references: &str,
    ) {
        let _ = self.conn.execute(
            "UPDATE messages SET references_ = ?1 \
             WHERE account_id = ?2 AND folder_path = ?3 AND uid = ?4",
            params![references, account_id, folder_path, uid],
        );
    }

    /// How far down a folder's References repair has walked: the uid to look
    /// below next, and whether the pass has finished. A folder never touched
    /// starts at the top (`u32::MAX`), so the first chunk is its newest mail.
    pub(crate) fn refs_repair_state(&self, account_id: u32, folder_path: &str) -> (u32, bool) {
        self.walk_state("refs_repair", account_id, folder_path)
    }

    pub(crate) fn set_refs_repair_state(
        &self,
        account_id: u32,
        folder_path: &str,
        next_uid: u32,
        done: bool,
    ) {
        self.set_walk_state("refs_repair", account_id, folder_path, next_uid, done);
    }

    /// How far down a folder the one-time Gmail label fill (#374) has walked,
    /// as [`Cache::refs_repair_state`] does for References.
    pub(crate) fn label_fill_state(&self, account_id: u32, folder_path: &str) -> (u32, bool) {
        self.walk_state("label_fill", account_id, folder_path)
    }

    pub(crate) fn set_label_fill_state(
        &self,
        account_id: u32,
        folder_path: &str,
        next_uid: u32,
        done: bool,
    ) {
        self.set_walk_state("label_fill", account_id, folder_path, next_uid, done);
    }

    /// Where a backfill walking down a folder stands, kept in `table`
    /// (`refs_repair` or `label_fill`): the next UID to look at and whether
    /// it is done. A folder not started yet starts from the top.
    fn walk_state(&self, table: &str, account_id: u32, folder_path: &str) -> (u32, bool) {
        self.conn
            .query_row(
                &format!(
                    "SELECT next_uid, done FROM {table} WHERE account_id = ?1 AND folder_path = ?2"
                ),
                params![account_id, folder_path],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, i64>(1)? != 0)),
            )
            .unwrap_or((u32::MAX, false))
    }

    fn set_walk_state(
        &self,
        table: &str,
        account_id: u32,
        folder_path: &str,
        next_uid: u32,
        done: bool,
    ) {
        let _ = self.conn.execute(
            &format!(
                "INSERT INTO {table} (account_id, folder_path, next_uid, done) \
                 VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(account_id, folder_path) DO UPDATE SET next_uid = ?3, done = ?4"
            ),
            params![account_id, folder_path, next_uid, done as i64],
        );
    }
}

/// One message matched by [`Cache::thread_summaries`]: what it is threaded
/// by, and what it would look like on a row.
struct ThreadRow {
    folder_path: String,
    message_id: String,
    references: String,
    latest: ThreadLatest,
}

/// The union of every group's ids, deduped and capped, for one scan.
fn capped_thread_ids(groups: &[(String, Vec<String>)]) -> Vec<&String> {
    // The union of every group's ids, deduped and capped: one scan for the
    // page. A group whose ids all fall past the cap simply gets no answer,
    // and the row keeps the count the list worked out for itself.
    let mut seen: HashSet<&str> = HashSet::new();
    let mut ids: Vec<&String> = Vec::new();
    for (_, group) in groups {
        for id in group {
            if id.is_empty() || !seen.insert(id.as_str()) {
                continue;
            }
            ids.push(id);
            if ids.len() == THREAD_COUNT_ID_LIMIT {
                break;
            }
        }
        if ids.len() == THREAD_COUNT_ID_LIMIT {
            break;
        }
    }
    ids
}
