//! What typed or pasted text does to the field: plain string rules, no widgets.

/// What to do with text about to go into the field, when it is more than
/// plain typing.
#[derive(Debug, PartialEq)]
pub(super) enum Insert {
    /// Leave it out: a space before anything is typed, or a lone comma.
    Swallow,
    /// Make pills of `done` and leave `rest` as the text.
    Commit { done: String, rest: String },
}

/// Decide what inserting `new` between `before` and `after` does. `None`
/// lets it go in as typed.
pub(super) fn on_insert(before: &str, new: &str, after: &str) -> Option<Insert> {
    let joined = format!("{before}{new}{after}");
    // A separator outside quotes and angle brackets ends a recipient: typed
    // or pasted, everything up to the last one becomes pills.
    if let Some(cut) = last_separator(&joined) {
        let (done, rest) = joined.split_at(cut);
        let rest = rest[1..].trim_start().to_string();
        if done.trim().is_empty() {
            return Some(if rest.is_empty() {
                Insert::Swallow
            } else {
                Insert::Commit {
                    done: String::new(),
                    rest,
                }
            });
        }
        return Some(Insert::Commit {
            done: done.to_string(),
            rest,
        });
    }
    if new.chars().all(char::is_whitespace) {
        // Nothing to separate yet.
        if before.trim().is_empty() && after.trim().is_empty() {
            return Some(Insert::Swallow);
        }
        // A space at the end of a whole address finishes it; anywhere else,
        // or after a name, it is part of what is being typed.
        if after.trim().is_empty() && is_whole_address(before) {
            return Some(Insert::Commit {
                done: before.to_string(),
                rest: String::new(),
            });
        }
        return None;
    }
    // Several addresses pasted with spaces between them.
    if new.chars().any(char::is_whitespace) {
        let words: Vec<&str> = joined.split_whitespace().collect();
        let ended =
            words.len() > 1 || (after.trim().is_empty() && new.ends_with(char::is_whitespace));
        if ended && !words.is_empty() && words.iter().all(|w| is_whole_address(w)) {
            return Some(Insert::Commit {
                done: words.join(", "),
                rest: String::new(),
            });
        }
    }
    None
}

/// The byte index of the last comma, semicolon or line break that separates
/// two recipients, outside a quoted name and an address's angle brackets.
fn last_separator(s: &str) -> Option<usize> {
    let (mut quoted, mut escaped, mut angle) = (false, false, false);
    let mut last = None;
    for (i, ch) in s.char_indices() {
        match ch {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' | ';' | '\n' | '\r' if !quoted && !angle => last = Some(i),
            _ => {}
        }
    }
    last
}

/// Outlook's semicolons and pasted line breaks read as commas.
pub(super) fn normalise_separators(s: &str) -> String {
    let (mut quoted, mut escaped, mut angle) = (false, false, false);
    s.chars()
        .map(|ch| {
            match ch {
                _ if escaped => escaped = false,
                '\\' if quoted => escaped = true,
                '"' => quoted = !quoted,
                '<' if !quoted => angle = true,
                '>' if !quoted => angle = false,
                ';' | '\n' | '\r' if !quoted && !angle => return ',',
                _ => {}
            }
            ch
        })
        .collect()
}

/// Whether the text is one complete recipient that can be sent to: an
/// address, or a name with its address closed in angle brackets.
pub(super) fn is_whole_address(s: &str) -> bool {
    let s = s.trim();
    if !s.contains('@') || (s.contains('<') && !s.ends_with('>')) {
        return false;
    }
    if s.chars().filter(|&c| c == '"').count() % 2 == 1 {
        return false;
    }
    crate::worker::parse_recipients(s).len() == 1 && crate::worker::invalid_recipient(s).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(done: &str, rest: &str) -> Option<Insert> {
        Some(Insert::Commit {
            done: done.into(),
            rest: rest.into(),
        })
    }

    #[test]
    fn a_space_after_a_whole_address_makes_a_pill() {
        assert_eq!(
            on_insert("ann@example.com", " ", ""),
            commit("ann@example.com", "")
        );
        assert_eq!(
            on_insert("Ann Lee <ann@example.com>", " ", ""),
            commit("Ann Lee <ann@example.com>", "")
        );
    }

    #[test]
    fn a_space_in_a_name_or_a_half_address_is_typed() {
        assert_eq!(on_insert("Ann", " ", ""), None);
        assert_eq!(on_insert("Ann <ann@exa", " ", ""), None);
        assert_eq!(on_insert("\"Lee", " ", ""), None);
        // Inside the text rather than at its end.
        assert_eq!(on_insert("ann@example.com", " ", "x"), None);
    }

    #[test]
    fn a_comma_makes_a_pill_and_never_stays() {
        assert_eq!(
            on_insert("ann@example.com", ",", ""),
            commit("ann@example.com", "")
        );
        assert_eq!(on_insert("Ann", ",", ""), commit("Ann", ""));
        assert_eq!(on_insert("", ",", ""), Some(Insert::Swallow));
        assert_eq!(on_insert("", " ", ""), Some(Insert::Swallow));
        assert_eq!(
            on_insert("bob@example.com", ";", ""),
            commit("bob@example.com", "")
        );
    }

    #[test]
    fn a_comma_in_a_quoted_name_is_part_of_it() {
        assert_eq!(on_insert("\"Lee", ",", ""), None);
        assert_eq!(on_insert("\"Lee, Ann\" <ann@exa", ",", ""), None);
    }

    #[test]
    fn a_pasted_list_becomes_pills_and_keeps_its_tail() {
        assert_eq!(
            on_insert("", "ann@example.com, Bob <bob@example.com>, car", ""),
            commit("ann@example.com, Bob <bob@example.com>", "car"),
        );
        assert_eq!(
            on_insert("", "ann@example.com bob@example.com", ""),
            commit("ann@example.com, bob@example.com", ""),
        );
        assert_eq!(on_insert("", "Ann Lee", ""), None);
    }

    #[test]
    fn separators_outside_quotes_read_as_commas() {
        assert_eq!(
            normalise_separators("a@x.com; b@y.com\nc@z.com"),
            "a@x.com, b@y.com,c@z.com"
        );
        assert_eq!(
            normalise_separators("\"Lee; Ann\" <a@x.com>"),
            "\"Lee; Ann\" <a@x.com>"
        );
    }

    #[test]
    fn whole_addresses() {
        assert!(is_whole_address("ann@example.com"));
        assert!(is_whole_address("\"Lee, Ann\" <ann@example.com>"));
        assert!(!is_whole_address("ann"));
        assert!(!is_whole_address("Ann <ann@example.com"));
        assert!(!is_whole_address("ann@"));
    }

    // Characterization tests (restructure, area D).

    #[test]
    fn pinned_separators_skip_quoted_names_and_angle_brackets() {
        assert_eq!(last_separator("\"a\\\", b\" <x@y.z>, c"), Some(16));
        assert_eq!(last_separator("\"a\\\\\", b"), Some(5));
        assert_eq!(last_separator("Ann <a,b@x.y>; c"), Some(13));
        // A backslash outside quotes escapes nothing.
        assert_eq!(last_separator("x\\\"y, z"), None);
        assert_eq!(normalise_separators("Ann <a,b@x.y>; c"), "Ann <a,b@x.y>, c");
        assert_eq!(
            normalise_separators("\"a\\\", b\" <x@y.z>, c"),
            "\"a\\\", b\" <x@y.z>, c"
        );
    }

    #[test]
    fn pinned_an_escaped_quote_counts_as_a_quote() {
        assert!(!is_whole_address("\"A \\\" B\" <a@b.c>"));
        assert!(is_whole_address("\"A B\" <a@b.c>"));
        assert!(!is_whole_address("Name <>"));
        assert!(is_whole_address("a@b"));
    }
}
