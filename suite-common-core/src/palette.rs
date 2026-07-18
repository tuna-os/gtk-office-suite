// palette.rs — GTK-free command palette filtering and ranking.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The palette widget (suite-common) feeds the query and the candidate
// list here; everything rankable is unit-tested without a display.

/// One palette candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    /// Fully qualified action name, e.g. `"app.bold"`.
    pub name: String,
    /// Human label shown in the row, e.g. `"Bold"`.
    pub label: String,
    /// Human-readable accelerator, e.g. `"Ctrl+B"`, if any.
    pub accel: Option<String>,
}

/// Match quality, higher is better. `None` = no match.
fn score(query: &str, label: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }
    let q = query.to_lowercase();
    let l = label.to_lowercase();
    if l == q {
        return Some(1000);
    }
    if l.starts_with(&q) {
        return Some(800);
    }
    // Word-start match: "li" hits "Insert Link…"
    if l.split_whitespace().any(|w| w.starts_with(&q)) {
        return Some(600);
    }
    if l.contains(&q) {
        return Some(400);
    }
    // Subsequence match: "epdf" hits "Export as PDF…"
    let mut it = l.chars();
    if q.chars().all(|qc| it.any(|lc| lc == qc)) {
        return Some(100);
    }
    None
}

/// Filter and rank entries for a query. Ties keep the input order, so
/// callers can pre-sort alphabetically for a stable empty-query listing.
pub fn filter_entries(query: &str, entries: &[PaletteEntry]) -> Vec<PaletteEntry> {
    let mut scored: Vec<(u32, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| score(query, &e.label).map(|s| (s, i)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| entries[i].clone()).collect()
}

/// Filter and rank with a most-recent-first list of action names.
/// Recency is a small bonus (< one match tier), so it reorders entries
/// of equal match quality — a recent weak match never beats a strong
/// one. With an empty query, recently used actions float to the top in
/// recency order.
pub fn filter_entries_with_recency(
    query: &str,
    entries: &[PaletteEntry],
    recent: &[String],
) -> Vec<PaletteEntry> {
    let bonus = |name: &str| -> u32 {
        match recent.iter().position(|r| r == name) {
            Some(pos) => 90u32.saturating_sub((pos as u32) * 10),
            None => 0,
        }
    };
    let mut scored: Vec<(u32, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| score(query, &e.label).map(|s| (s + bonus(&e.name), i)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| entries[i].clone()).collect()
}

/// Record a use of `name` in the MRU list (most recent first, deduped,
/// truncated to `cap`).
pub fn push_recent(recent: &mut Vec<String>, name: &str, cap: usize) {
    recent.retain(|r| r != name);
    recent.insert(0, name.to_string());
    recent.truncate(cap);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(label: &str) -> PaletteEntry {
        PaletteEntry { name: format!("app.{}", label.to_lowercase()), label: label.into(), accel: None }
    }

    #[test]
    fn empty_query_returns_everything_in_order() {
        let entries = vec![e("Bold"), e("Italic"), e("Underline")];
        let out = filter_entries("", &entries);
        assert_eq!(out, entries);
    }

    #[test]
    fn prefix_beats_substring() {
        let entries = vec![e("Insert Bold Marker"), e("Bold")];
        let out = filter_entries("bo", &entries);
        assert_eq!(out[0].label, "Bold");
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn word_start_matches() {
        let entries = vec![e("Insert Link…"), e("Blink")];
        let out = filter_entries("li", &entries);
        assert_eq!(out[0].label, "Insert Link…");
    }

    #[test]
    fn subsequence_matches_last() {
        let entries = vec![e("Export as PDF…"), e("Print…")];
        let out = filter_entries("epdf", &entries);
        assert_eq!(out, vec![e("Export as PDF…")]);
    }

    #[test]
    fn no_match_filters_out() {
        let entries = vec![e("Bold")];
        assert!(filter_entries("zzz", &entries).is_empty());
    }

    #[test]
    fn case_insensitive() {
        let entries = vec![e("Merge Cells")];
        assert_eq!(filter_entries("MERGE", &entries).len(), 1);
    }

    #[test]
    fn recency_floats_recent_to_top_on_empty_query() {
        let entries = vec![e("Bold"), e("Italic"), e("Underline")];
        let recent = vec!["app.underline".to_string(), "app.italic".to_string()];
        let out = filter_entries_with_recency("", &entries, &recent);
        assert_eq!(out[0].label, "Underline");
        assert_eq!(out[1].label, "Italic");
        assert_eq!(out[2].label, "Bold");
    }

    #[test]
    fn recent_weak_match_never_beats_strong_match() {
        let entries = vec![e("Bold"), e("Insert Bold Marker")];
        // "Insert Bold Marker" was used recently, but "bo" prefix-matches Bold.
        let recent = vec!["app.insert bold marker".to_string()];
        let out = filter_entries_with_recency("bo", &entries, &recent);
        assert_eq!(out[0].label, "Bold");
    }

    #[test]
    fn recency_breaks_ties_within_a_tier() {
        let entries = vec![e("Merge Cells"), e("Merge Rows")];
        let recent = vec!["app.merge rows".to_string()];
        let out = filter_entries_with_recency("merge", &entries, &recent);
        assert_eq!(out[0].label, "Merge Rows");
    }

    #[test]
    fn push_recent_dedupes_and_caps() {
        let mut r = vec!["a".to_string(), "b".to_string()];
        push_recent(&mut r, "b", 3);
        assert_eq!(r, vec!["b", "a"]);
        push_recent(&mut r, "c", 2);
        assert_eq!(r, vec!["c", "b"]);
    }
}
