//! Messages for GNOME Shell's overview search (INTEGRATION-PLAN B1).
//!
//! The Shell asks while someone types, so this reads only what a result row
//! shows: the subject, who it is from and the preview line. Bodies and
//! recipients are not searched, and drafts, junk and trash are not offered.

use rusqlite::{params_from_iter, types::Value};

use super::assistant::like_anywhere;
use super::Cache;

/// One message as the Shell shows it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ShellHit {
    /// The Message-ID as the cache stores it; a `mid:` link opens it.
    pub message_id: String,
    pub subject: String,
    pub from: String,
    pub preview: String,
}

/// Messages the Shell does not offer: Drafts (3), Junk (5) and Trash (6),
/// as `kind_to_i64` stores them.
const HIDDEN_KINDS: &str = "3, 5, 6";

impl Cache {
    /// The message with this Message-ID (any copy), shown the way a search
    /// result is. `None` when the cache no longer has it.
    pub(crate) fn shell_hit(&self, message_id: &str) -> Option<ShellHit> {
        self.conn
            .query_row(
                "SELECT message_id, subject, from_name, from_addr, preview FROM messages \
                 WHERE message_id = ?1 LIMIT 1",
                [message_id],
                |row| {
                    let (name, addr): (String, String) = (row.get(2)?, row.get(3)?);
                    Ok(ShellHit {
                        message_id: row.get(0)?,
                        subject: row.get(1)?,
                        from: if name.trim().is_empty() { addr } else { name },
                        preview: row.get(4)?,
                    })
                },
            )
            .ok()
    }

    /// The newest messages whose subject, sender or preview contains every one
    /// of `terms`, at most `limit`, one per Message-ID (a Gmail message is
    /// listed once per label). No terms, no results.
    pub(crate) fn shell_search(&self, terms: &[String], limit: usize) -> Vec<ShellHit> {
        let terms: Vec<&str> = terms.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
        if terms.is_empty() || limit == 0 {
            return Vec::new();
        }
        let mut args: Vec<Value> = Vec::new();
        let mut wheres = vec![
            "m.message_id <> ''".to_string(),
            format!(
                "NOT EXISTS (SELECT 1 FROM folders f WHERE f.account_id = m.account_id \
                 AND f.path = m.folder_path AND f.kind IN ({HIDDEN_KINDS}))"
            ),
        ];
        for term in &terms {
            let any: Vec<String> = ["m.subject", "m.from_name", "m.from_addr", "m.preview"]
                .iter()
                .map(|c| {
                    args.push(Value::Text(like_anywhere(term)));
                    format!("{c} LIKE ? ESCAPE '\\'")
                })
                .collect();
            wheres.push(format!("({})", any.join(" OR ")));
        }
        // A few extra rows, since copies are dropped below.
        args.push(Value::Integer((limit * 3) as i64));
        let sql = format!(
            "SELECT m.message_id, m.subject, m.from_name, m.from_addr, m.preview \
             FROM messages m WHERE {} ORDER BY m.ts DESC LIMIT ?",
            wheres.join(" AND ")
        );
        let run = || -> rusqlite::Result<Vec<ShellHit>> {
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(args.iter()), |row| {
                let (name, addr): (String, String) = (row.get(2)?, row.get(3)?);
                Ok(ShellHit {
                    message_id: row.get(0)?,
                    subject: row.get(1)?,
                    from: if name.trim().is_empty() { addr } else { name },
                    preview: row.get(4)?,
                })
            })?;
            rows.collect()
        };
        let mut seen = std::collections::HashSet::new();
        run()
            .unwrap_or_else(|e| {
                tracing::warn!("cache shell_search failed: {e}");
                Vec::new()
            })
            .into_iter()
            .filter(|h| seen.insert(h.message_id.clone()))
            .take(limit)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::{add_folder, add_msg};
    use super::*;
    use crate::models::FolderKind;

    fn cache() -> Cache {
        let c = Cache::in_memory().unwrap();
        add_folder(&c, "INBOX", FolderKind::Inbox);
        add_folder(&c, "Drafts", FolderKind::Drafts);
        add_folder(&c, "Trash", FolderKind::Trash);
        c
    }

    fn with_id(c: &Cache, folder: &str, uid: u32, id: &str) {
        c.conn
            .execute(
                "UPDATE messages SET message_id = ?1 WHERE folder_path = ?2 AND uid = ?3",
                rusqlite::params![id, folder, uid],
            )
            .unwrap();
    }

    fn terms(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn every_term_must_match_and_the_newest_comes_first() {
        let c = cache();
        add_msg(&c, "INBOX", 1, "Ada Lovelace", "Quarterly report", 100);
        add_msg(&c, "INBOX", 2, "Ada Lovelace", "Lunch on Friday", 200);
        add_msg(&c, "INBOX", 3, "Grace Hopper", "Quarterly report draft", 300);
        for (uid, id) in [(1, "a@x"), (2, "b@x"), (3, "c@x")] {
            with_id(&c, "INBOX", uid, id);
        }
        let ids = |t: &[&str]| -> Vec<String> {
            c.shell_search(&terms(t), 10).into_iter().map(|h| h.message_id).collect()
        };
        assert_eq!(ids(&["quarterly"]), ["c@x", "a@x"]);
        assert_eq!(ids(&["quarterly", "ada"]), ["a@x"]);
        assert!(ids(&["nothing"]).is_empty());
        assert!(ids(&[]).is_empty());
        assert!(ids(&["  "]).is_empty());
    }

    #[test]
    fn drafts_trash_and_messages_without_an_id_are_not_offered() {
        let c = cache();
        add_msg(&c, "INBOX", 1, "Ada", "Plan", 1);
        add_msg(&c, "Drafts", 1, "Ada", "Plan", 2);
        add_msg(&c, "Trash", 1, "Ada", "Plan", 3);
        add_msg(&c, "INBOX", 2, "Ada", "Plan", 4); // no Message-ID
        with_id(&c, "INBOX", 1, "inbox@x");
        with_id(&c, "Drafts", 1, "draft@x");
        with_id(&c, "Trash", 1, "trash@x");
        let hits = c.shell_search(&terms(&["plan"]), 10);
        assert_eq!(hits.iter().map(|h| h.message_id.as_str()).collect::<Vec<_>>(), ["inbox@x"]);
    }

    #[test]
    fn wildcards_in_a_term_are_matched_literally_and_copies_are_listed_once() {
        let c = cache();
        add_msg(&c, "INBOX", 1, "Ada", "100% done", 1);
        add_msg(&c, "INBOX", 2, "Ada", "1000 done", 2);
        add_msg(&c, "INBOX", 3, "Ada", "100% done", 3);
        for (uid, id) in [(1, "same@x"), (2, "other@x"), (3, "same@x")] {
            with_id(&c, "INBOX", uid, id);
        }
        let hits = c.shell_search(&terms(&["100%"]), 10);
        assert_eq!(hits.iter().map(|h| h.message_id.as_str()).collect::<Vec<_>>(), ["same@x"]);
        assert_eq!(c.shell_search(&terms(&["done"]), 1).len(), 1);
    }

    #[test]
    fn a_result_is_looked_up_by_its_message_id() {
        let c = cache();
        add_msg(&c, "INBOX", 1, "Ada", "Plan", 1);
        with_id(&c, "INBOX", 1, "a@x");
        assert_eq!(c.shell_hit("a@x").map(|h| h.subject), Some("Plan".to_string()));
        assert!(c.shell_hit("gone@x").is_none());
    }

    #[test]
    fn a_sender_with_no_name_shows_the_address() {
        let c = cache();
        add_msg(&c, "INBOX", 1, "", "Hello", 1);
        c.conn
            .execute("UPDATE messages SET from_addr = 'ada@example.org', message_id = 'a@x'", [])
            .unwrap();
        assert_eq!(c.shell_search(&terms(&["hello"]), 5)[0].from, "ada@example.org");
    }
}
