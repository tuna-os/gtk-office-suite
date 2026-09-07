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
///
/// `start`/`end` are **byte** offsets into `haystack`, always on a character
/// boundary. Callers working in character offsets (GtkTextBuffer, the Letters
/// document model) must convert — see [`byte_to_char_offset`].
pub fn search(haystack: &str, query: &SearchQuery) -> Vec<SearchMatch> {
    matches_of(haystack, query)
        .into_iter()
        .map(|(start, end)| SearchMatch { start, end, text: haystack[start..end].to_string() })
        .collect()
}

/// Replace the first occurrence of `query` in `haystack` with `replacement`.
///
/// Returns the new string with the replacement applied, or the original string
/// unchanged if there is no match.
///
/// For regex queries, `replacement` may contain `$1`, `$2`, … back-references to
/// capture groups. The `case_sensitive` and `whole_word` flags are respected on
/// every path, regex included, exactly as they are in [`search`].
pub fn replace_first(haystack: &str, query: &SearchQuery, replacement: &str) -> String {
    replace_upto(haystack, query, replacement, 1).0
}

/// Replace all non-overlapping occurrences of `query` in `haystack` with
/// `replacement`.
///
/// Returns a tuple `(new_string, count)` where `count` is the number of
/// replacements actually made — it always describes the returned string. When
/// `count` is `0` the returned string equals `haystack`.
///
/// For regex queries, `replacement` may contain `$1`, `$2`, … back-references to
/// capture groups. The `case_sensitive` and `whole_word` flags are respected on
/// every path, regex included, exactly as they are in [`search`].
pub fn replace_all(haystack: &str, query: &SearchQuery, replacement: &str) -> (String, usize) {
    replace_upto(haystack, query, replacement, usize::MAX)
}

/// Convert a byte offset into `text` to a character offset.
///
/// [`search`] reports byte offsets; the Letters document model and
/// GtkTextBuffer both address text in characters. Feeding a byte offset to a
/// character API silently corrupts any document containing non-ASCII text, so
/// conversion is not optional.
pub fn byte_to_char_offset(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Byte ranges of every non-overlapping match, honouring all query flags.
/// Single source of truth for both [`search`] and the replace entry points, so
/// a match found by one is always a match replaced by the other.
fn matches_of(haystack: &str, query: &SearchQuery) -> Vec<(usize, usize)> {
    if query.query.is_empty() {
        return vec![];
    }
    if query.regex {
        let Some(re) = build_regex(query) else { return vec![] };
        re.find_iter(haystack)
            .filter(|m| !query.whole_word || is_word_boundary(haystack, m.start(), m.end()))
            .map(|m| (m.start(), m.end()))
            .collect()
    } else {
        plain_matches(haystack, query)
    }
}

/// Literal (non-regex) match scan over `haystack`.
fn plain_matches(haystack: &str, query: &SearchQuery) -> Vec<(usize, usize)> {
    let mut results = Vec::new();
    let mut offset = 0usize;
    while let Some((start, end)) = find_from(haystack, &query.query, offset, query.case_sensitive) {
        if query.whole_word && !is_word_boundary(haystack, start, end) {
            // Advance by one *character*, never one byte: `start + 1` can land
            // inside a multi-byte character and panic the next slice.
            offset = next_char_boundary(haystack, start);
            continue;
        }
        results.push((start, end));
        offset = end;
    }
    results
}

/// Byte range of the next occurrence of `needle` in `haystack` at or after
/// `from`, or `None`.
///
/// Case-insensitive matching compares character by character against the
/// original string rather than against a lowercased copy: case folding can
/// change a character's byte length (Turkish `İ`), which would skew every
/// offset derived from such a copy.
fn find_from(haystack: &str, needle: &str, from: usize, case_sensitive: bool) -> Option<(usize, usize)> {
    if needle.is_empty() || from > haystack.len() {
        return None;
    }
    if case_sensitive {
        return haystack[from..]
            .find(needle)
            .map(|pos| (from + pos, from + pos + needle.len()));
    }
    let needle_chars: Vec<char> = needle.chars().collect();
    for (relative, _) in haystack[from..].char_indices() {
        let start = from + relative;
        let mut end = start;
        let mut matched = 0usize;
        for ch in haystack[start..].chars() {
            if matched == needle_chars.len() || !chars_eq_ignore_case(ch, needle_chars[matched]) {
                break;
            }
            end += ch.len_utf8();
            matched += 1;
        }
        if matched == needle_chars.len() {
            return Some((start, end));
        }
    }
    None
}

fn chars_eq_ignore_case(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Byte offset of the character boundary immediately after `from`.
fn next_char_boundary(text: &str, from: usize) -> usize {
    text[from..].chars().next().map_or(text.len(), |ch| from + ch.len_utf8())
}

/// Build a [`regex::Regex`] respecting `case_sensitive`.
fn build_regex(query: &SearchQuery) -> Option<regex::Regex> {
    let pattern = if query.case_sensitive {
        query.query.clone()
    } else {
        format!("(?i){}", query.query)
    };
    regex::Regex::new(&pattern).ok()
}

/// Replace at most `limit` matches, returning the new string and the number of
/// replacements actually performed.
fn replace_upto(
    haystack: &str,
    query: &SearchQuery,
    replacement: &str,
    limit: usize,
) -> (String, usize) {
    if query.query.is_empty() || limit == 0 {
        return (haystack.to_string(), 0);
    }
    if query.regex {
        return replace_regex(haystack, query, replacement, limit);
    }
    let mut result = String::with_capacity(haystack.len());
    let mut last = 0usize;
    let mut count = 0usize;
    for (start, end) in plain_matches(haystack, query) {
        result.push_str(&haystack[last..start]);
        result.push_str(replacement);
        last = end;
        count += 1;
        if count == limit {
            break;
        }
    }
    result.push_str(&haystack[last..]);
    (result, count)
}

/// Regex replacement with capture-group expansion. Written by hand rather than
/// via `Regex::replacen` so that `whole_word` filtering applies to the string
/// that is produced, not only to the count that is reported.
fn replace_regex(
    haystack: &str,
    query: &SearchQuery,
    replacement: &str,
    limit: usize,
) -> (String, usize) {
    let Some(re) = build_regex(query) else { return (haystack.to_string(), 0) };
    let mut result = String::with_capacity(haystack.len());
    let mut last = 0usize;
    let mut count = 0usize;
    for caps in re.captures_iter(haystack) {
        let Some(whole) = caps.get(0) else { continue };
        if query.whole_word && !is_word_boundary(haystack, whole.start(), whole.end()) {
            continue;
        }
        result.push_str(&haystack[last..whole.start()]);
        caps.expand(replacement, &mut result);
        last = whole.end();
        count += 1;
        if count == limit {
            break;
        }
    }
    result.push_str(&haystack[last..]);
    (result, count)
}

/// True when `[start, end)` is not butted up against an alphanumeric character
/// on either side. Character-based, so accented and non-Latin words behave the
/// same way ASCII ones do.
fn is_word_boundary(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back().is_none_or(|ch| !ch.is_alphanumeric());
    let after = text[end..].chars().next().is_none_or(|ch| !ch.is_alphanumeric());
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

    // -----------------------------------------------------------------------
    // replace_first
    // -----------------------------------------------------------------------

    /// Only the first occurrence is replaced; the second is left alone.
    #[test]
    fn test_replace_first_basic() {
        let q = SearchQuery::new("foo");
        assert_eq!(replace_first("foo bar foo", &q, "baz"), "baz bar foo");
    }

    /// No match: the original string is returned unchanged.
    #[test]
    fn test_replace_first_no_match() {
        let q = SearchQuery::new("xyz");
        assert_eq!(replace_first("hello world", &q, "REPLACED"), "hello world");
    }

    /// Empty query: returns the original string unchanged.
    #[test]
    fn test_replace_first_empty_query() {
        let q = SearchQuery::new("");
        assert_eq!(replace_first("hello", &q, "X"), "hello");
    }

    /// A regex replacement stops after one match, back-reference included.
    #[test]
    fn test_replace_first_regex_stops_after_one() {
        let q = SearchQuery::new(r"(\d+)").regex(true);
        assert_eq!(replace_first("a 1 b 2", &q, "<$1>"), "a <1> b 2");
    }

    // -----------------------------------------------------------------------
    // replace_all
    // -----------------------------------------------------------------------

    /// Multiple occurrences are all replaced; count is correct.
    #[test]
    fn test_replace_all_multiple() {
        let q = SearchQuery::new("cat");
        let (result, count) = replace_all("cat and cat and cat", &q, "dog");
        assert_eq!(result, "dog and dog and dog");
        assert_eq!(count, 3);
    }

    /// Case-insensitive flag replaces all variants of the word.
    #[test]
    fn test_replace_all_case_insensitive() {
        let q = SearchQuery::new("hello");
        let (result, count) = replace_all("Hello HELLO hello", &q, "hi");
        assert_eq!(result, "hi hi hi");
        assert_eq!(count, 3);
    }

    /// Whole-word flag: partial matches inside longer words are skipped.
    #[test]
    fn test_replace_all_whole_word_skips_partial() {
        let q = SearchQuery::new("cat").whole_word(true);
        let (result, count) = replace_all("cat catalog cat", &q, "dog");
        assert_eq!(result, "dog catalog dog");
        assert_eq!(count, 2);
    }

    /// Empty query: returns (original, 0).
    #[test]
    fn test_replace_all_empty_query() {
        let q = SearchQuery::new("");
        let (result, count) = replace_all("hello", &q, "X");
        assert_eq!(result, "hello");
        assert_eq!(count, 0);
    }

    /// Regex with capture-group back-reference in replacement.
    #[test]
    fn test_replace_all_regex_backreference() {
        let q = SearchQuery::new(r"(\d+)").regex(true);
        let (result, count) = replace_all("abc 42 def 7", &q, "<$1>");
        assert_eq!(result, "abc <42> def <7>");
        assert_eq!(count, 2);
    }

    /// An invalid regex is inert rather than fatal: no matches, no edits.
    #[test]
    fn test_replace_all_invalid_regex_is_inert() {
        let q = SearchQuery::new("(unclosed").regex(true);
        let (result, count) = replace_all("(unclosed here", &q, "X");
        assert_eq!(result, "(unclosed here");
        assert_eq!(count, 0);
        assert!(search("(unclosed here", &q).is_empty());
    }

    // -----------------------------------------------------------------------
    // Non-ASCII / flag-interaction regressions
    // -----------------------------------------------------------------------

    /// Whole-word search over multi-byte text used to slice mid-character and
    /// panic when a rejected match started on a non-ASCII byte.
    #[test]
    fn test_whole_word_multibyte_does_not_panic() {
        let q = SearchQuery::new("é").whole_word(true);
        let (result, count) = replace_all("café cafés", &q, "e");
        // Both "é" occurrences are inside words, so neither is a whole word.
        assert_eq!(result, "café cafés");
        assert_eq!(count, 0);
        assert!(search("café cafés", &q).is_empty());
    }

    /// Word boundaries are character-based: an accented letter next to the
    /// match is a word character, not a separator.
    #[test]
    fn test_whole_word_boundary_is_character_based() {
        let q = SearchQuery::new("cafe").whole_word(true);
        // "cafeé" is one word, so the embedded "cafe" is not a whole word.
        assert!(search("cafeé", &q).is_empty());
        assert_eq!(search("cafe é", &q).len(), 1);
    }

    /// Match offsets are byte offsets on a character boundary, so slicing the
    /// haystack with them is always safe.
    #[test]
    fn test_match_offsets_are_char_boundaries() {
        let q = SearchQuery::new("cat");
        let matches = search("naïve cat", &q);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 7); // "naïve " is 7 bytes, 6 chars
        assert_eq!(byte_to_char_offset("naïve cat", matches[0].start), 6);
        assert_eq!(&"naïve cat"[matches[0].start..matches[0].end], "cat");
    }

    /// Case-insensitive matching over multi-byte text reports offsets into the
    /// original string, not into a lowercased copy.
    #[test]
    fn test_case_insensitive_multibyte_offsets() {
        let q = SearchQuery::new("café");
        let matches = search("un CAFÉ noir", &q);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].text, "CAFÉ");
        let (result, count) = replace_all("un CAFÉ noir", &q, "thé");
        assert_eq!(result, "un thé noir");
        assert_eq!(count, 1);
    }

    /// The regex path honours `case_sensitive` the same way the literal path
    /// does — previously it was always case-sensitive.
    #[test]
    fn test_regex_honours_case_flags() {
        let insensitive = SearchQuery::new("hello").regex(true);
        assert_eq!(search("HELLO hello", &insensitive).len(), 2);
        let sensitive = SearchQuery::new("hello").regex(true).case_sensitive(true);
        assert_eq!(search("HELLO hello", &sensitive).len(), 1);
    }

    /// `whole_word` applies to the regex *replacement*, not only to the count:
    /// the returned string and the returned count must describe each other.
    #[test]
    fn test_replace_all_regex_whole_word_filters_output() {
        let q = SearchQuery::new("cat").regex(true).whole_word(true);
        let (result, count) = replace_all("cat catalog cat", &q, "dog");
        assert_eq!(result, "dog catalog dog");
        assert_eq!(count, 2);
    }

    /// The same query must yield the same matches through `search` and through
    /// `replace_all` — they share one match scanner precisely so this holds.
    #[test]
    fn test_search_and_replace_agree_on_match_count() {
        let cases = [
            SearchQuery::new("cat").whole_word(true),
            SearchQuery::new("CAT"),
            SearchQuery::new("cat").regex(true).whole_word(true),
            SearchQuery::new(r"ca.").regex(true),
        ];
        let hay = "cat catalog CAT";
        for q in cases {
            let (_, count) = replace_all(hay, &q, "x");
            assert_eq!(search(hay, &q).len(), count, "disagreement for {q:?}");
        }
    }
}
