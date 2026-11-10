// search_provider.rs — the GTK-free half of a GNOME Shell search provider
// (INTEGRATION-PLAN B1).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// `org.gnome.Shell.SearchProvider2` asks four things of an app: the result
// ids for the typed terms (`GetInitialResultSet`), the same narrowed after
// another word was typed (`GetSubsearchResultSet`), what to show for each id
// (`GetResultMetas`), and to open one (`ActivateResult`). The D-Bus service and
// its `*.search-provider.ini` are the GTK/zbus half; this module is what they
// call, so the matching and the ordering are tested without a bus.
//
// What an app searches here is its recent documents (`RecentItem`), which the
// apps already keep and which is exactly the set a person types a name of.

use crate::recent::RecentItem;

/// What the Shell shows for one result (the `a{sv}` of `GetResultMetas`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResultMeta {
    /// The id passed back to `ActivateResult`: the document's path.
    pub id: String,
    /// The first line: the document's title.
    pub name: String,
    /// The second line: the folder it is in.
    pub description: String,
    /// A themed icon name for the result.
    pub icon: String,
}

/// The id the Shell is given for `item`. A path is stable, unique and what
/// `ActivateResult` needs to open the document.
pub fn result_id(item: &RecentItem) -> String {
    item.path.to_string_lossy().into_owned()
}

/// The search terms the Shell sent, lower-cased, with empty ones dropped.
fn normalise(terms: &[String]) -> Vec<String> {
    terms.iter().map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty()).collect()
}

/// Whether every term is found in the title or the path (so a folder name
/// narrows too), ignoring case.
fn matches(item: &RecentItem, terms: &[String]) -> bool {
    let title = item.title.to_lowercase();
    let path = item.path.to_string_lossy().to_lowercase();
    terms.iter().all(|t| title.contains(t) || path.contains(t))
}

/// How good a match is, lower is better: a title that starts with the first
/// term beats a word inside the title, which beats a path-only match.
fn rank(item: &RecentItem, terms: &[String]) -> u8 {
    let title = item.title.to_lowercase();
    let first = &terms[0];
    if title.starts_with(first.as_str()) {
        0
    } else if title.split(|c: char| !c.is_alphanumeric()).any(|w| !w.is_empty() && w.starts_with(first.as_str())) {
        1
    } else if title.contains(first.as_str()) {
        2
    } else {
        3
    }
}

/// `GetInitialResultSet`: the ids of the documents matching every term, best
/// first. `items` is in recency order (most recent first), which breaks ties.
/// With no usable term nothing is returned, so an empty Shell box lists
/// nothing. At most `limit` ids are returned.
pub fn initial_result_set(items: &[RecentItem], terms: &[String], limit: usize) -> Vec<String> {
    let terms = normalise(terms);
    if terms.is_empty() || limit == 0 {
        return Vec::new();
    }
    let mut hits: Vec<(u8, usize, &RecentItem)> =
        items.iter().enumerate().filter(|(_, item)| matches(item, &terms)).map(|(i, item)| (rank(item, &terms), i, item)).collect();
    hits.sort_by_key(|(rank, position, _)| (*rank, *position));
    hits.into_iter().take(limit).map(|(_, _, item)| result_id(item)).collect()
}

/// `GetSubsearchResultSet`: the Shell typed another word, so only the earlier
/// results can still match. Ids that are no longer in `items` are dropped.
pub fn subsearch_result_set(items: &[RecentItem], previous: &[String], terms: &[String], limit: usize) -> Vec<String> {
    let still_there: Vec<RecentItem> = items.iter().filter(|item| previous.contains(&result_id(item))).cloned().collect();
    initial_result_set(&still_there, terms, limit)
}

/// `GetResultMetas`: what to draw for each id, in the order asked. Unknown ids
/// are skipped, not an error: the document may have left the list since.
pub fn result_metas(items: &[RecentItem], ids: &[String], icon: &str) -> Vec<ResultMeta> {
    ids.iter()
        .filter_map(|id| items.iter().find(|item| &result_id(item) == id))
        .map(|item| ResultMeta {
            id: result_id(item),
            name: item.title.clone(),
            description: item.path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            icon: icon.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn item(title: &str, path: &str) -> RecentItem {
        RecentItem { path: PathBuf::from(path), title: title.into(), last_opened_secs: 0 }
    }

    fn terms(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn items() -> Vec<RecentItem> {
        vec![
            item("Quarterly budget", "/home/a/Finance/budget-q3.xlsx"),
            item("Budget notes", "/home/a/Notes/notes.docx"),
            item("Holiday plan", "/home/a/Finance/holiday.docx"),
        ]
    }

    #[test]
    fn a_title_prefix_ranks_above_a_word_inside_it() {
        let ids = initial_result_set(&items(), &terms(&["budget"]), 10);
        assert_eq!(ids, ["/home/a/Notes/notes.docx", "/home/a/Finance/budget-q3.xlsx"]);
    }

    #[test]
    fn every_term_must_match_and_the_folder_counts() {
        let ids = initial_result_set(&items(), &terms(&["budget", "finance"]), 10);
        assert_eq!(ids, ["/home/a/Finance/budget-q3.xlsx"]);
    }

    #[test]
    fn matching_ignores_case_and_blank_terms() {
        let ids = initial_result_set(&items(), &terms(&["  HOLIDAY ", ""]), 10);
        assert_eq!(ids, ["/home/a/Finance/holiday.docx"]);
    }

    #[test]
    fn recency_order_breaks_a_tie() {
        let items = vec![item("Plan B", "/x/b.docx"), item("Plan A", "/x/a.docx")];
        assert_eq!(initial_result_set(&items, &terms(&["plan"]), 10), ["/x/b.docx", "/x/a.docx"]);
    }

    #[test]
    fn no_terms_or_a_zero_limit_returns_nothing() {
        assert!(initial_result_set(&items(), &[], 10).is_empty());
        assert!(initial_result_set(&items(), &terms(&[" "]), 10).is_empty());
        assert!(initial_result_set(&items(), &terms(&["budget"]), 0).is_empty());
    }

    #[test]
    fn the_limit_applies_after_ranking() {
        let ids = initial_result_set(&items(), &terms(&["budget"]), 1);
        assert_eq!(ids, ["/home/a/Notes/notes.docx"]);
    }

    #[test]
    fn a_subsearch_only_narrows_the_earlier_results() {
        let first = initial_result_set(&items(), &terms(&["budget"]), 10);
        let narrowed = subsearch_result_set(&items(), &first, &terms(&["budget", "q3"]), 10);
        assert_eq!(narrowed, ["/home/a/Finance/budget-q3.xlsx"]);
        // "holiday" matches a document, but not one the first search returned.
        assert!(subsearch_result_set(&items(), &first, &terms(&["holiday"]), 10).is_empty());
    }

    #[test]
    fn metas_carry_title_folder_and_icon_and_skip_unknown_ids() {
        let ids = vec!["/home/a/Finance/holiday.docx".to_string(), "/gone.docx".to_string()];
        let metas = result_metas(&items(), &ids, "org.tunaos.letters");
        assert_eq!(
            metas,
            [ResultMeta {
                id: "/home/a/Finance/holiday.docx".into(),
                name: "Holiday plan".into(),
                description: "/home/a/Finance".into(),
                icon: "org.tunaos.letters".into(),
            }]
        );
    }
}
