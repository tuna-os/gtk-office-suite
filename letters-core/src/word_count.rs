// word_count.rs — how many words the status bar should report.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The editor buffer contains presentation as well as prose: list markers
// ("- ", "3. ") and, since tables gained a buffer mapping, the pipes and
// dashes of a rendered grid. None of that is the user's writing, so none
// of it may be counted — a three-column empty table would otherwise add
// eleven "words" to a document containing nothing.
//
// GTK-free and counted from text rather than from a captured Document:
// this runs on every keystroke, where rebuilding the whole model would be
// wasteful, and the two agree because both read the same rendered lines.

use crate::table_text;

/// Words in what the user actually wrote, given the editor's text.
pub fn editor_word_count(text: &str) -> usize {
    text.lines().map(line_word_count).sum()
}

fn line_word_count(line: &str) -> usize {
    if table_text::is_delimiter_line(line) {
        return 0; // "| --- | --- |" is structure, not writing.
    }
    if let Some(cells) = table_text::parse_row(line) {
        let chars: Vec<char> = line.chars().collect();
        return cells
            .iter()
            .map(|range| chars[range.clone()].iter().collect::<String>().split_whitespace().count())
            .sum();
    }
    strip_list_marker(line).split_whitespace().count()
}

/// Drop a rendered list marker ("- " / "12. ", after any indent) so the
/// bullet is not counted as a word of its own.
fn strip_list_marker(line: &str) -> &str {
    let body = line.trim_start_matches(' ');
    if let Some(rest) = body.strip_prefix("- ") {
        return rest;
    }
    if let Some(dot) = body.find(". ") {
        if dot > 0 && body[..dot].chars().all(|c| c.is_ascii_digit()) {
            return &body[dot + 2..];
        }
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_prose_counts_its_words() {
        assert_eq!(editor_word_count("the quick brown fox"), 4);
        assert_eq!(editor_word_count("one\ntwo three"), 3);
        assert_eq!(editor_word_count(""), 0);
    }

    #[test]
    fn an_empty_table_contributes_nothing() {
        // What the status bar showed before: "|  |  |  |" and
        // "| --- | --- | --- |" split into 11 whitespace-separated tokens.
        let table = "|  |  |  |\n| --- | --- | --- |\n|  |  |  |\n|  |  |  |";
        assert_eq!(editor_word_count(table), 0);
        assert_eq!(editor_word_count(&format!("intro\n{table}")), 1);
    }

    #[test]
    fn table_cells_count_their_own_words() {
        let table = "| Name | Unit price |\n| --- | --- |\n| Bolts | 12 |";
        assert_eq!(editor_word_count(table), 5);
    }

    #[test]
    fn list_markers_are_not_words() {
        assert_eq!(editor_word_count("- first item"), 2);
        assert_eq!(editor_word_count("    3. nested item"), 2);
        // Prose that merely starts with a dash-like word still counts.
        assert_eq!(editor_word_count("-dash prefixed"), 2);
    }

    #[test]
    fn prose_with_a_pipe_is_still_prose() {
        assert_eq!(editor_word_count("a | b"), 3);
    }
}
