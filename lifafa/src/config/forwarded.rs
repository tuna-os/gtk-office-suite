//! The log of messages a filter has already forwarded, so that no message is
//! forwarded twice however often the filters run over it. Kept in its own
//! file, not in the cache, which can be wiped at any time: a wipe would
//! forward everything again.

use std::collections::HashMap;

use super::{config_file, read_toml, save_private_toml};

/// How far back a filter forwards: mail dated earlier is left alone by every
/// action of a forwarding rule. It bounds the log, which only has to
/// remember forwards of mail a rule could still pick up.
pub(crate) const FORWARD_WINDOW_SECS: i64 = 30 * 24 * 3600;

/// How long a forward stays in the log, counted from the message's date.
/// Longer than [`FORWARD_WINDOW_SECS`], so an entry outlives the mail it
/// protects, whatever the clocks of the sender and this machine say.
const KEEP_SECS: i64 = 2 * FORWARD_WINDOW_SECS;

/// The key of one forward: the account, the message and the destinations.
/// `message_ref` names the message (see `filter_actions::message_ref`). The
/// destinations are sorted and lowercased, so reordering the addresses of a
/// rule does not forward the same message again, while changing them does.
pub(crate) fn forward_key(account_email: &str, message_ref: &str, to: &[String]) -> String {
    let mut to: Vec<String> = to.iter().map(|a| a.to_lowercase()).collect();
    to.sort();
    format!(
        "{}|{}|{}",
        account_email.to_lowercase(),
        message_ref,
        to.join(",")
    )
}

/// Where a forward stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Forwarded {
    /// The server took it.
    Sent,
    /// It failed and waits in the Outbox, which sends it on its own. Not
    /// sent again, but not gone either: the original stays where it is.
    Queued,
}

/// One forward in `filter_forwarded.toml`.
#[derive(serde::Serialize, serde::Deserialize)]
struct Entry {
    key: String,
    /// The forwarded message's date, which decides when the entry goes.
    date: i64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    queued: bool,
}

/// What `filter_forwarded.toml` holds.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct ForwardedFile {
    #[serde(default)]
    forwards: Vec<Entry>,
}

/// The forwards done so far: key to (message date, state).
#[derive(Default)]
pub(crate) struct ForwardedLog {
    entries: HashMap<String, (i64, Forwarded)>,
}

impl ForwardedLog {
    /// The log as it is on disk; empty when there is none yet.
    pub(crate) fn load() -> Self {
        let file =
            read_toml::<ForwardedFile>(config_file("filter_forwarded.toml")).unwrap_or_default();
        let mut log = ForwardedLog {
            entries: file
                .forwards
                .into_iter()
                .map(|e| {
                    let state = if e.queued {
                        Forwarded::Queued
                    } else {
                        Forwarded::Sent
                    };
                    (e.key, (e.date, state))
                })
                .collect(),
        };
        log.prune(crate::datefmt::now());
        log
    }

    /// Where the forward under `key` stands, `None` when it was never made.
    pub(crate) fn state(&self, key: &str) -> Option<Forwarded> {
        self.entries.get(key).map(|(_, s)| *s)
    }

    /// Remember a forward of a message dated `date`, and save the log.
    pub(crate) fn record(&mut self, key: String, date: i64, state: Forwarded) {
        if self.entries.get(&key) == Some(&(date, state)) {
            return;
        }
        self.entries.insert(key, (date, state));
        self.prune(crate::datefmt::now());
        let mut forwards: Vec<Entry> = self
            .entries
            .iter()
            .map(|(key, (date, state))| Entry {
                key: key.clone(),
                date: *date,
                queued: *state == Forwarded::Queued,
            })
            .collect();
        forwards.sort_by(|a, b| (a.date, &a.key).cmp(&(b.date, &b.key)));
        save_private_toml(
            config_file("filter_forwarded.toml"),
            &ForwardedFile { forwards },
            "forwarded messages",
            true,
        );
    }

    /// Forget forwards of mail too old for any rule to pick up again.
    fn prune(&mut self, now: i64) {
        self.entries.retain(|_, (date, _)| *date >= now - KEEP_SECS);
    }

    /// A log holding `keys` as sent, for tests.
    #[cfg(test)]
    pub(crate) fn with_keys(keys: Vec<String>) -> Self {
        ForwardedLog {
            entries: keys
                .into_iter()
                .map(|k| (k, (i64::MAX, Forwarded::Sent)))
                .collect(),
        }
    }

    /// A log holding `key` as queued in the Outbox, for tests.
    #[cfg(test)]
    pub(crate) fn with_queued(key: String) -> Self {
        let mut log = ForwardedLog::default();
        log.entries.insert(key, (i64::MAX, Forwarded::Queued));
        log
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_ignores_destination_order_and_case() {
        let a = forward_key("Me@X.org", "<m1>", &["b@y.org".into(), "A@y.org".into()]);
        let b = forward_key("me@x.org", "<m1>", &["a@y.org".into(), "B@Y.org".into()]);
        assert_eq!(a, b);
        assert_ne!(a, forward_key("me@x.org", "<m1>", &["a@y.org".into()]));
        assert_ne!(
            a,
            forward_key("me@x.org", "<m2>", &["a@y.org".into(), "b@y.org".into()])
        );
    }

    #[test]
    fn state_finds_a_recorded_key() {
        let log = ForwardedLog::with_keys(vec!["k1".into()]);
        assert_eq!(log.state("k1"), Some(Forwarded::Sent));
        assert_eq!(log.state("k2"), None);
        let log = ForwardedLog::with_queued("k3".into());
        assert_eq!(log.state("k3"), Some(Forwarded::Queued));
    }

    #[test]
    fn prune_drops_only_mail_older_than_the_window_can_reach() {
        let now = 100 * 24 * 3600;
        let mut log = ForwardedLog::default();
        log.entries
            .insert("old".into(), (now - KEEP_SECS - 1, Forwarded::Sent));
        log.entries.insert(
            "reachable".into(),
            (now - FORWARD_WINDOW_SECS - 1, Forwarded::Sent),
        );
        log.prune(now);
        assert_eq!(log.state("old"), None);
        assert_eq!(log.state("reachable"), Some(Forwarded::Sent));
    }

    #[test]
    fn file_round_trips_queued_and_sent() {
        let text = "[[forwards]]\nkey = \"a\"\ndate = 5\n\n[[forwards]]\nkey = \"b\"\ndate = 6\nqueued = true\n";
        let file: ForwardedFile = toml::from_str(text).unwrap();
        assert!(!file.forwards[0].queued && file.forwards[1].queued);
        let again = toml::to_string(&file).unwrap();
        assert_eq!(again, text);
    }
}
