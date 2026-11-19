//! Mail that arrived end-to-end encrypted in an account whose helper
//! decrypts it (Tuta, Proton), marked on its cached row so the assistant
//! server ([`crate::mcp`]) keeps it out of an assistant's reach as it keeps
//! OpenPGP mail: no text, no preview, no files, no reply and no forward.
//!
//! OpenPGP mail marks itself: its row's preview is the lock marker, and its
//! text never reaches the cache. Mail in these accounts is decrypted by the
//! helper and cached as ordinary MIME, so nothing in a row says how it
//! arrived. Their listings do say it, for every message, and what they say
//! is kept as two keywords on the row:
//!
//! - [`SEALED`]: an assistant gets the message's headers and nothing else.
//! - [`CHECKED`]: a listing has said which kind the message is.
//!
//! A row with neither was cached before listings were read for this. It is
//! sealed when its account's worker starts ([`Cache::seal_unchecked`]), and
//! the next index pass lists it again ([`Cache::checked_uid_times`]), which
//! puts the right mark on it.
//!
//! A backslash is not a character a tag's keyword can hold (see
//! `Tag::valid_keyword`), so no tag change, the user's or an assistant's,
//! takes a mark off a row.

use std::collections::HashMap;

use rusqlite::params;

use super::Cache;
use crate::models::Message;

/// On a row an assistant gets nothing of beyond its headers.
pub(crate) const SEALED: &str = "\\Sealed";

/// On a row whose listing said whether it arrived end-to-end encrypted.
pub(crate) const CHECKED: &str = "\\SealChecked";

/// The keywords of a row as its listing gives them: `end_to_end` is the
/// helper's word on how the message arrived. A helper that does not say is
/// not taken to mean ordinary mail.
pub(crate) fn listed_marks(end_to_end: Option<bool>) -> Vec<String> {
    match end_to_end {
        Some(false) => vec![CHECKED.to_string()],
        Some(true) | None => vec![SEALED.to_string(), CHECKED.to_string()],
    }
}

/// Whether `m` is kept from assistants by its mark.
pub(crate) fn is_sealed(m: &Message) -> bool {
    m.has_keyword(SEALED)
}

/// Whether a row of `messages` carries `keyword`, as SQL. Only for the two
/// keywords above: the keyword goes into the text.
fn has_keyword_sql(keyword: &str) -> String {
    format!(
        "instr(' ' || lower(keywords) || ' ', ' {} ') > 0",
        keyword.to_ascii_lowercase()
    )
}

/// Whether a row of `messages` is sealed, as SQL.
pub(super) fn sealed_sql() -> String {
    has_keyword_sql(SEALED)
}

impl Cache {
    /// Seal every row of `account_id` no listing has marked yet, and say how
    /// many there were: until one is listed again, nothing says it is not
    /// end-to-end encrypted mail.
    pub(crate) fn seal_unchecked(&self, account_id: u32) -> usize {
        let sql = format!(
            "UPDATE messages SET keywords = trim(keywords || ' {SEALED}') \
             WHERE account_id = ?1 AND NOT ({}) AND NOT ({})",
            has_keyword_sql(CHECKED),
            has_keyword_sql(SEALED),
        );
        self.conn
            .execute(&sql, params![account_id])
            .unwrap_or_else(|e| {
                tracing::warn!("cache seal_unchecked failed: {e}");
                0
            })
    }

    /// [`Cache::cached_uid_times`], of the rows a listing has marked. An
    /// index pass that plans from these fetches the others again.
    pub(crate) fn checked_uid_times(
        &self,
        account_id: u32,
        folder_path: &str,
    ) -> HashMap<u32, i64> {
        let run = || -> rusqlite::Result<HashMap<u32, i64>> {
            let mut stmt = self.conn.prepare(&format!(
                "SELECT uid, ts FROM messages \
                 WHERE account_id = ?1 AND folder_path = ?2 AND {}",
                has_keyword_sql(CHECKED)
            ))?;
            let rows = stmt.query_map(params![account_id, folder_path], |row| {
                Ok((row.get::<_, u32>(0)?, row.get::<_, i64>(1)?))
            })?;
            rows.collect()
        };
        run().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::MailScope;
    use crate::ui::message_list::Query;

    fn row(uid: u32, subject: &str, keywords: Vec<String>) -> Message {
        Message {
            id: uid,
            uid,
            subject: subject.into(),
            from_addr: "ann@example.com".into(),
            message_id: format!("{uid}@example.com"),
            timestamp: i64::from(uid),
            keywords,
            ..Message::blank()
        }
    }

    #[test]
    fn a_listing_seals_end_to_end_mail_and_what_it_does_not_vouch_for() {
        assert!(is_sealed(&row(1, "a", listed_marks(Some(true)))));
        assert!(!is_sealed(&row(2, "b", listed_marks(Some(false)))));
        assert!(is_sealed(&row(3, "c", listed_marks(None))));
        for said in [Some(true), Some(false), None] {
            assert!(listed_marks(said).iter().any(|k| k == CHECKED));
        }
        // An ordinary row, of any other account, is not sealed.
        assert!(!is_sealed(&row(4, "d", vec!["Work".into()])));
        // No tag can be given either keyword.
        assert!(!crate::config::Tag::valid_keyword(SEALED));
        assert!(!crate::config::Tag::valid_keyword(CHECKED));
    }

    #[test]
    fn rows_cached_before_the_marks_are_sealed_until_listed_again() {
        let cache = Cache::in_memory().unwrap();
        cache.upsert_messages(
            1,
            "INBOX",
            &[
                row(1, "old", Vec::new()),
                row(2, "old, tagged", vec!["Work".into()]),
                row(3, "ordinary", listed_marks(Some(false))),
                row(4, "end to end", listed_marks(Some(true))),
            ],
        );
        cache.upsert_messages(2, "INBOX", &[row(5, "another account", Vec::new())]);
        let sealed =
            |account: u32, uid: u32| is_sealed(&cache.message_at(account, "INBOX", uid).unwrap());
        assert!(!sealed(1, 1) && !sealed(1, 2), "nothing marks them yet");

        assert_eq!(cache.seal_unchecked(1), 2);
        assert!(sealed(1, 1) && sealed(1, 2) && sealed(1, 4));
        assert!(!sealed(1, 3), "what a listing called ordinary stays open");
        assert!(!sealed(2, 5), "another account's rows are its worker's");
        assert!(cache.message_at(1, "INBOX", 2).unwrap().has_keyword("Work"));
        assert_eq!(cache.seal_unchecked(1), 0, "once is enough");

        // Only what a listing marked counts as indexed.
        let mut checked: Vec<u32> = cache.checked_uid_times(1, "INBOX").into_keys().collect();
        checked.sort_unstable();
        assert_eq!(checked, [3, 4]);
        assert_eq!(cache.cached_uid_times(1, "INBOX").len(), 4);

        // Listed again, a row wears what its listing says.
        cache.upsert_messages(1, "INBOX", &[row(1, "old", listed_marks(Some(false)))]);
        assert!(!sealed(1, 1));
        assert_eq!(cache.checked_uid_times(1, "INBOX").len(), 3);
    }

    #[test]
    fn a_search_finds_a_sealed_message_by_its_headers_only() {
        let cache = Cache::in_memory().unwrap();
        let mut sealed = row(1, "Quarterly figures", listed_marks(Some(true)));
        sealed.preview = "the secret word is pineapple".into();
        let mut ordinary = row(2, "Lunch", listed_marks(Some(false)));
        ordinary.preview = "pineapple on a pizza".into();
        cache.upsert_messages(1, "INBOX", &[sealed, ordinary]);
        cache.save_body(1, "INBOX", 1, "<p>pineapple, and the code is zanzibar</p>");
        cache.save_body(1, "INBOX", 2, "<p>pineapple on a pizza in zanzibar</p>");
        let scope = MailScope {
            account_id: 1,
            read_bodies: true,
            shared_paths: vec!["INBOX".into()],
            folder: None,
        };
        let found = |query: &str| -> Vec<u32> {
            cache
                .find_messages(std::slice::from_ref(&scope), &Query::parse(query), 20, 0)
                .into_iter()
                .map(|(_, _, m)| m.uid)
                .collect()
        };
        // What a search finds would say what the preview and the text hold.
        assert_eq!(found("pineapple"), [2]);
        assert_eq!(found("zanzibar"), [2]);
        assert!(found("quarterly pineapple").is_empty());
        // Its subject and its sender are an assistant's to see.
        assert_eq!(found("quarterly"), [1]);
        assert_eq!(found("from:ann"), [2, 1]);
    }
}
