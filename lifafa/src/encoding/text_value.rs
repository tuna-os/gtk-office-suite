//! The TEXT value escapes iCalendar (RFC 5545 3.3.11) and vCard (RFC 6350
//! 3.4) share: a backslash before a comma, a semicolon or a backslash, and
//! `\n` for a line break.

/// Write a value with the escapes in place; a carriage return is dropped.
pub(crate) fn escape_text_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Undo the escapes: `\n` (and `\N`) is a line break, and any other
/// escaped character stands for itself. A lone backslash at the end is
/// kept.
pub(crate) fn unescape_text_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}
