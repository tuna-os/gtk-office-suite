//! The addresses mail was sent to and received from, for the
//! composer's suggestions.

use rusqlite::params;

use super::Cache;

impl Cache {
    /// Record recipients the user has sent to, so they autocomplete even before
    /// the Sent folder syncs. Each send bumps the address's frequency. The app
    /// records at the moment of sending, whatever route the message then
    /// takes (straight out, the Outbox, Send Later).
    pub(crate) fn record_addresses(&self, entries: &[(String, String)]) {
        for (name, email) in entries {
            let email = email.trim().to_lowercase();
            if email.is_empty() || !email.contains('@') {
                continue;
            }
            if let Err(e) = self.conn.execute(
                "INSERT INTO addresses(email, name, count) VALUES(?1, ?2, 1) \
                 ON CONFLICT(email) DO UPDATE SET count = count + 1, \
                   name = CASE WHEN excluded.name <> '' THEN excluded.name ELSE addresses.name END",
                params![email, name.trim()],
            ) {
                tracing::warn!("could not record recipient {email}: {e}");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Outbox: messages that could not be sent yet
    // -----------------------------------------------------------------------

    /// Aggregate every address seen in stored mail (senders received +
    /// recipients sent/recorded), as (name, email, frequency), most-frequent first.
    pub(crate) fn address_history(&self) -> Vec<(String, String, u32)> {
        use std::collections::HashMap;
        let mut counts: HashMap<String, (String, u32)> = HashMap::new();
        fn bump(counts: &mut HashMap<String, (String, u32)>, name: &str, email: &str, n: u32) {
            let email = email.trim();
            if email.is_empty() || !email.contains('@') {
                return;
            }
            let key = email.to_lowercase();
            let entry = counts.entry(key).or_insert_with(|| (String::new(), 0));
            entry.1 += n;
            if entry.0.is_empty() && !name.trim().is_empty() && name.trim() != email {
                entry.0 = name.trim().to_string();
            }
            if entry.0.is_empty() {
                entry.0 = email.to_string();
            }
        }

        if let Ok(mut stmt) = self
            .conn
            .prepare("SELECT from_name, from_addr, recipients, cc FROM messages")
        {
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap_or_default(),
                    r.get::<_, String>(1).unwrap_or_default(),
                    r.get::<_, String>(2).unwrap_or_default(),
                    r.get::<_, String>(3).unwrap_or_default(),
                ))
            });
            if let Ok(rows) = rows {
                for (from_name, from_addr, to, cc) in rows.flatten() {
                    bump(&mut counts, &from_name, &from_addr, 1);
                    for list in [to, cc] {
                        for addr in list.split(',') {
                            bump(&mut counts, addr.trim(), addr.trim(), 1);
                        }
                    }
                }
            }
        }

        if let Ok(mut stmt) = self
            .conn
            .prepare("SELECT name, email, count FROM addresses")
        {
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap_or_default(),
                    r.get::<_, String>(1).unwrap_or_default(),
                    r.get::<_, u32>(2).unwrap_or(0),
                ))
            });
            if let Ok(rows) = rows {
                for (name, email, c) in rows.flatten() {
                    bump(&mut counts, &name, &email, c);
                }
            }
        }

        let mut out: Vec<(String, String, u32)> = counts
            .into_iter()
            .map(|(email, (name, n))| (name, email, n))
            .collect();
        out.sort_by_key(|entry| std::cmp::Reverse(entry.2));
        out
    }

    /// Every address in one account's mail, lower case: what the composer
    /// keeps of the history when that account suggests only its own (#410).
    pub(crate) fn account_addresses(&self, account_id: u32) -> std::collections::HashSet<String> {
        let mut out = std::collections::HashSet::new();
        let Ok(mut stmt) = self
            .conn
            .prepare("SELECT from_addr, recipients, cc FROM messages WHERE account_id = ?1")
        else {
            return out;
        };
        let rows = stmt.query_map(params![account_id], |r| {
            Ok((
                r.get::<_, String>(0).unwrap_or_default(),
                r.get::<_, String>(1).unwrap_or_default(),
                r.get::<_, String>(2).unwrap_or_default(),
            ))
        });
        for (from, to, cc) in rows.into_iter().flatten().flatten() {
            for addr in std::iter::once(from.as_str())
                .chain(to.split(','))
                .chain(cc.split(','))
            {
                let addr = addr.trim();
                if addr.contains('@') {
                    out.insert(addr.to_lowercase());
                }
            }
        }
        out
    }
}
