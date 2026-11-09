//! Keywords (tags) as IMAP stores them: which are the user's, and keeping
//! the cached ones in step with the server (#166).

use async_imap::types::{Fetch, Flag};
use futures::TryStreamExt;

use crate::cache::Cache;
use crate::worker::imap::wire::{exam, fetch_uids, search_uids, uid_set};
use crate::worker::imap::ImapSession;

/// Keywords that are a server's or another client's bookkeeping, not tags a
/// person set: left out of the tag finder's report. System flags (`\Seen`…)
/// never reach here as keywords, but a leading backslash is rejected too.
pub(crate) fn is_system_keyword(keyword: &str) -> bool {
    let k = keyword.to_ascii_lowercase();
    if k.starts_with('\\') {
        return true;
    }
    const SYSTEM: &[&str] = &[
        "$junk",
        "$notjunk",
        "junk",
        "nonjunk",
        "notjunk",
        "$autojunk",
        "$forwarded",
        "$mdnsent",
        "$phishing",
        "$submitted",
        "$submitpending",
        "$hasattachment",
        "$hasnoattachment",
        "$signed",
        "$encrypted",
        "$recent",
        "$isnotification",
        "$ismailinglist",
        "$istrusted",
        "$notification",
    ];
    SYSTEM.contains(&k.as_str())
        // Apple Mail's flag colors and Fastmail's internal annotations.
        || k.starts_with("$mailflagbit")
        || k.starts_with("$x-me-")
}

/// The keyword half of a refresh with a tag view open (#166): every folder
/// is examined and, for each configured tag, the server's KEYWORD search is
/// set against the cached keywords. Tags set or cleared from another client
/// land in the index here — a folder's own sync only sees them once that
/// folder is opened. Returns the folders whose rows changed.
pub(super) async fn refresh_keywords(
    account_id: u32,
    session: &mut ImapSession,
    cache: Option<&Cache>,
    keywords: &[String],
) -> Vec<String> {
    let mut changed = Vec::new();
    let Some(cache) = cache else { return changed };
    if keywords.is_empty() {
        return changed;
    }
    for f in cache.load_folders(account_id) {
        if exam(session, &f.path).await.is_err() {
            continue;
        }
        // Where the search and the index disagree, the message's own flags
        // decide. iCloud answers SEARCH from an index that lags behind STORE
        // and FETCH, so trusting the search took a tag just set off again
        // until the folder's next sync put it back, and round again: tags
        // came and went in the tag view.
        let mut disputed: Vec<(u32, &str, bool)> = Vec::new();
        for kw in keywords {
            let Ok(server) = search_uids(session, format!("KEYWORD {kw}")).await else {
                continue;
            };
            let cached: std::collections::HashSet<u32> = cache
                .uids_with_keyword(account_id, &f.path, kw)
                .into_iter()
                .collect();
            disputed.extend(
                server
                    .symmetric_difference(&cached)
                    .map(|uid| (*uid, kw.as_str(), cached.contains(uid))),
            );
        }
        if disputed.is_empty() {
            continue;
        }
        let uids: Vec<u32> = disputed.iter().map(|(uid, _, _)| *uid).collect();
        let flags: std::collections::HashMap<u32, Vec<String>> =
            match fetch_uids(session, uid_set(&uids), "(UID FLAGS)").await {
                Ok(stream) => match stream.try_collect::<Vec<Fetch>>().await {
                    Ok(fetches) => fetches
                        .iter()
                        .filter_map(|f| {
                            Some((f.uid?, custom_flags(&f.flags().collect::<Vec<_>>())))
                        })
                        .collect(),
                    Err(_) => continue,
                },
                Err(_) => continue,
            };
        let mut touched = false;
        for (uid, kw, indexed) in disputed {
            // A uid the server did not return is gone, or new mail the index
            // does not hold yet: the folder's own sync deals with both.
            let Some(has) = flags.get(&uid) else { continue };
            let on = has.iter().any(|k| k.eq_ignore_ascii_case(kw));
            if on != indexed {
                touched |= cache.set_keyword(account_id, &f.path, uid, kw, on);
            }
        }
        if touched {
            changed.push(f.path.clone());
        }
    }
    if !changed.is_empty() {
        tracing::info!("keywords: changed {changed:?}");
    }
    changed
}

/// The keyword RFC 5550 gives a forwarded message; servers and clients
/// differ in its case.
pub(super) const FORWARDED_KEYWORD: &str = "$Forwarded";

/// The user-defined keywords among a message's flags, verbatim.
pub(super) fn custom_flags(flags: &[Flag]) -> Vec<String> {
    flags
        .iter()
        .filter_map(|f| match f {
            Flag::Custom(k) => Some(k.to_string()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_keywords_are_told_from_tags() {
        for k in [
            "\\Seen",
            "$Junk",
            "NonJunk",
            "$Forwarded",
            "$MDNSent",
            "$MailFlagBit0",
            "$X-ME-Annot-2",
            "$IsNotification",
        ] {
            assert!(is_system_keyword(k), "{k}");
        }
        for k in ["$label1", "Work", "$Important", "important"] {
            assert!(!is_system_keyword(k), "{k}");
        }
    }
}
