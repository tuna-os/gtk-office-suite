//! Bare web addresses and email addresses in Markdown text.

#[derive(Clone, Copy)]
pub(super) enum UrlKind {
    /// Carries its own scheme.
    Web,
    /// `www.` with no scheme.
    Bare,
    Mail,
}

/// Byte ranges of the links in `text`. Trailing punctuation is left out, so
/// a sentence ending "see https://example.com." keeps its full stop.
pub(super) fn find_urls(text: &str) -> Vec<(usize, usize, UrlKind)> {
    let b = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0usize;
    let mut last_end = 0usize;
    while i < b.len() {
        // Byte stepping: never slice mid-character.
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }
        // An email address: the word around an `@`. Checked before the
        // word-boundary rule below, because an address's `@` is never at
        // the start of a word.
        if b[i] == b'@' && i > 0 {
            let start = text[..i]
                .char_indices()
                .rfind(|&(_, c)| is_break_char(c))
                .map_or(0, |(p, c)| p + c.len_utf8());
            let end = i + text[i..].find(is_break_char).unwrap_or(text.len() - i);
            let end = trim_trailing(text, i, end);
            let domain = text.get(i + 1..end).unwrap_or("");
            if start >= last_end && start < i && domain.contains('.') && !domain.starts_with('.') {
                found.push((start, end, UrlKind::Mail));
                i = end;
                last_end = end;
                continue;
            }
        }
        // A URL only ever starts at a word boundary.
        if text[..i].chars().next_back().is_none_or(is_break) {
            let rest = &text[i..];
            let kind = if rest.starts_with("https://") || rest.starts_with("http://") {
                Some(UrlKind::Web)
            } else if rest.starts_with("www.") {
                Some(UrlKind::Bare)
            } else {
                None
            };
            if let Some(kind) = kind {
                let end = i + rest
                    .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
                    .unwrap_or(rest.len());
                let end = trim_trailing(text, i, end);
                if end > i {
                    found.push((i, end, kind));
                    i = end;
                    last_end = end;
                    continue;
                }
            }
        }
        i += 1;
    }
    found
}

fn is_break(c: char) -> bool {
    c.is_whitespace() || matches!(c, '(' | '[' | '<' | '"' | '\'')
}

fn is_break_char(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '(' | ')' | '[' | ']' | '<' | '>' | '"' | '\'' | ',' | ';'
        )
}

/// Pull sentence punctuation (and an unbalanced closing bracket) back off
/// the end of a link.
fn trim_trailing(text: &str, start: usize, mut end: usize) -> usize {
    while end > start {
        let c = text[..end].chars().next_back().unwrap_or(' ');
        let drop = matches!(
            c,
            '.' | ',' | ';' | ':' | '!' | '?' | '"' | '\'' | '*' | '_'
        ) || (c == ')'
            && text[start..end].matches('(').count() < text[start..end].matches(')').count());
        if !drop {
            break;
        }
        end -= c.len_utf8();
    }
    end
}
