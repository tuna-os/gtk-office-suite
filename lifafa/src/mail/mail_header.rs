//! Raw header values, for the readers that look at headers mail_parser does
//! not interpret (sensitivity labels, unsubscribe handles).

/// A folded header value as one line: each physical line trimmed, blank
/// ones dropped, the rest joined by a space.
pub(crate) fn unfold(value: &str) -> String {
    value
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
