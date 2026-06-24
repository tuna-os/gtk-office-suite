// search.rs — Shared search/find infrastructure.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pattern: LibreOffice svl/srchitem.hxx (SvxSearchItem).
// Generic text search with case sensitivity, whole word, and regex
// support. Used by Letters (document search), Tables (find in sheet),
// and Decks (find across slides).


/// Search query configuration.
#[derive(Clone, Debug)]
pub struct SearchQuery {
    pub query: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

impl SearchQuery {
    pub fn new(query: &str) -> Self {
        SearchQuery { query: query.into(), case_sensitive: false, whole_word: false, regex: false }
    }

    pub fn case_sensitive(mut self, yes: bool) -> Self { self.case_sensitive = yes; self }
    pub fn whole_word(mut self, yes: bool) -> Self { self.whole_word = yes; self }
    pub fn regex(mut self, yes: bool) -> Self { self.regex = yes; self }
}

/// A search match with position and matched text.
#[derive(Clone, Debug)]
pub struct SearchMatch {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// Search for matches in a single string. Returns all non-overlapping matches.
pub fn search(haystack: &str, query: &SearchQuery) -> Vec<SearchMatch> {
    if query.query.is_empty() { return vec![]; }

    if query.regex {
        search_regex(haystack, query)
    } else {
        search_simple(haystack, query)
    }
}

fn search_simple(haystack: &str, query: &SearchQuery) -> Vec<SearchMatch> {
    let mut results = Vec::new();
    let haystack_lower: String;
    let query_lower: String;

    let (hay, q) = if query.case_sensitive {
        (haystack, query.query.as_str())
    } else {
        haystack_lower = haystack.to_lowercase();
        query_lower = query.query.to_lowercase();
        (haystack_lower.as_str(), query_lower.as_str())
    };

    let mut offset = 0usize;
    while let Some(pos) = hay[offset..].find(q) {
        let start = offset + pos;
        let end = start + q.len();

        if query.whole_word && !is_word_boundary(haystack, start, end) {
            offset = start + 1;
            continue;
        }

        results.push(SearchMatch {
            start,
            end,
            text: haystack[start..end].to_string(),
        });
        offset = end;
    }
    results
}

fn search_regex(haystack: &str, query: &SearchQuery) -> Vec<SearchMatch> {
    let re = match regex::Regex::new(&query.query) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    re.find_iter(haystack)
        .filter(|m| !query.whole_word || is_word_boundary(haystack, m.start(), m.end()))
        .map(|m| SearchMatch {
            start: m.start(),
            end: m.end(),
            text: m.as_str().to_string(),
        })
        .collect()
}

fn is_word_boundary(text: &str, start: usize, end: usize) -> bool {
    let before = start == 0 || text.as_bytes().get(start - 1).map_or(true, |b| !b.is_ascii_alphanumeric());
    let after = end >= text.len() || text.as_bytes().get(end).map_or(true, |b| !b.is_ascii_alphanumeric());
    before && after
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_search() {
        let q = SearchQuery::new("hello");
        let matches = search("hello world hello", &q);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].text, "hello");
        assert_eq!(matches[1].text, "hello");
    }

    #[test]
    fn test_case_insensitive() {
        let q = SearchQuery::new("Hello");
        let matches = search("hello HELLO Hello", &q);
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_case_sensitive() {
        let q = SearchQuery::new("Hello").case_sensitive(true);
        let matches = search("hello Hello HELLO", &q);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_whole_word() {
        let q = SearchQuery::new("cat").whole_word(true);
        let matches = search("cat catalog cat", &q);
        assert_eq!(matches.len(), 2); // "cat" not "catalog"
    }

    #[test]
    fn test_regex() {
        let q = SearchQuery::new(r"\d+").regex(true);
        let matches = search("abc 123 def 45", &q);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].text, "123");
        assert_eq!(matches[1].text, "45");
    }

    #[test]
    fn test_no_match() {
        let q = SearchQuery::new("xyz");
        let matches = search("hello world", &q);
        assert!(matches.is_empty());
    }
}
