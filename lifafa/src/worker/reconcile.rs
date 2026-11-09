//! Which cached rows a server listing shows to be gone (#429), for the Graph
//! and JMAP paths, whose listings no longer replace the cached folder.

use crate::models::Message;

/// The cached rows a listing of a folder's newest mail shows to be gone
/// (#429), for the Graph and JMAP paths, whose listings no longer replace
/// the cached folder. A `complete` listing is the whole folder; otherwise it
/// speaks only for mail newer than the oldest it holds, the rest being the
/// background index's to reconcile.
pub(super) fn window_gone(
    cached: &std::collections::HashMap<u32, i64>,
    listed: &[Message],
    complete: bool,
) -> Vec<u32> {
    let seen: std::collections::HashSet<u32> = listed.iter().map(|m| m.uid).collect();
    let oldest = listed.iter().map(|m| m.timestamp).min();
    let mut gone: Vec<u32> = cached
        .iter()
        .filter(|(uid, ts)| {
            !seen.contains(uid) && (complete || oldest.is_some_and(|oldest| **ts > oldest))
        })
        .map(|(uid, _)| *uid)
        .collect();
    gone.sort_unstable();
    gone
}

/// The cached rows a complete id listing of a folder shows to be gone
/// (#429). A row that arrived since the listing began (`since`, less a
/// margin for the server's clock) is spared: the listing could not know
/// it, and the next look at the folder's newest mail settles it.
pub(super) fn listing_gone(
    cached: &std::collections::HashMap<u32, i64>,
    server: &std::collections::HashSet<u32>,
    since: i64,
) -> Vec<u32> {
    let cutoff = since - 600;
    let mut gone: Vec<u32> = cached
        .iter()
        .filter(|(uid, ts)| !server.contains(uid) && **ts < cutoff)
        .map(|(uid, _)| *uid)
        .collect();
    gone.sort_unstable();
    gone
}
