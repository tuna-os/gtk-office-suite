//! What the other suite apps may ask of Mandelbrot, as a GTK-free contract.
//!
//! Letters, Tables, Decks and Lifafa reach Mandelbrot through `GApplication`
//! actions (so they work over D-Bus and with `gapplication action`). This
//! module says what an action's string parameter means, so that the meaning is
//! unit-tested apart from the GUI. The actions themselves live in
//! `application.rs`.
//!
//! * `open-room(target)` shows a room, user or event.
//! * `start-call(target)` shows the call view of a room.
//!
//! `target` is anything a person might paste: a room ID (`!abc:example.org`),
//! a room alias (`#room:example.org`), a user ID (`@alice:example.org`), a
//! `matrix:` URI or a `https://matrix.to/#/…` link.

use std::fmt;

/// Why a `target` could not be understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TargetError {
    /// Nothing but whitespace.
    Empty,
    /// Not a Matrix identifier, `matrix:` URI or matrix.to link.
    NotMatrix(String),
}

impl fmt::Display for TargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("no room was given"),
            Self::NotMatrix(target) => write!(f, "`{target}` is not a Matrix room, user or link"),
        }
    }
}

/// The `matrix:` URI (or matrix.to link, which Mandelbrot already parses) for
/// a `target`.
///
/// Links and URIs are returned as they are; a bare identifier becomes the
/// `matrix:` URI that names it. Only the shape is checked here; whether the
/// room exists is for the session to find out.
pub(crate) fn target_to_uri(target: &str) -> Result<String, TargetError> {
    let target = target.trim();
    if target.is_empty() {
        return Err(TargetError::Empty);
    }

    let lower = target.to_ascii_lowercase();
    if lower.starts_with("matrix:") || lower.starts_with("https://matrix.to/") {
        return Ok(target.to_owned());
    }

    let (kind, rest) = if let Some(rest) = target.strip_prefix('!') {
        ("roomid", rest)
    } else if let Some(rest) = target.strip_prefix('#') {
        ("r", rest)
    } else if let Some(rest) = target.strip_prefix('@') {
        ("u", rest)
    } else {
        return Err(TargetError::NotMatrix(target.to_owned()));
    };

    // `localpart:server`, both parts present, no path or query to smuggle in.
    let well_formed = rest
        .split_once(':')
        .is_some_and(|(local, server)| !local.is_empty() && !server.is_empty())
        && !rest.contains(['/', '?', '#', ' ']);
    if !well_formed {
        return Err(TargetError::NotMatrix(target.to_owned()));
    }

    Ok(format!("matrix:{kind}/{rest}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_room_id_becomes_a_roomid_uri() {
        assert_eq!(
            target_to_uri("!abc:example.org").as_deref(),
            Ok("matrix:roomid/abc:example.org")
        );
    }

    #[test]
    fn an_alias_becomes_a_room_uri() {
        assert_eq!(
            target_to_uri("#suite:example.org").as_deref(),
            Ok("matrix:r/suite:example.org")
        );
    }

    #[test]
    fn a_user_id_becomes_a_user_uri() {
        assert_eq!(
            target_to_uri("@alice:example.org").as_deref(),
            Ok("matrix:u/alice:example.org")
        );
    }

    #[test]
    fn links_and_uris_pass_through() {
        for link in [
            "matrix:r/suite:example.org?via=example.org",
            "MATRIX:u/alice:example.org",
            "https://matrix.to/#/#suite:example.org",
        ] {
            assert_eq!(target_to_uri(link).as_deref(), Ok(link));
        }
    }

    #[test]
    fn surrounding_whitespace_is_ignored() {
        assert_eq!(
            target_to_uri("  !abc:example.org\n").as_deref(),
            Ok("matrix:roomid/abc:example.org")
        );
    }

    #[test]
    fn nothing_given_is_said_to_be_empty() {
        assert_eq!(target_to_uri("   "), Err(TargetError::Empty));
    }

    #[test]
    fn what_is_not_matrix_is_reported_with_the_text() {
        for bad in [
            "suite",
            "https://example.org/",
            "!abc",
            "#:example.org",
            "@a:",
        ] {
            assert_eq!(
                target_to_uri(bad),
                Err(TargetError::NotMatrix(bad.to_owned())),
                "{bad}"
            );
        }
    }

    #[test]
    fn an_identifier_cannot_carry_a_path_or_query() {
        assert!(target_to_uri("!abc:example.org/../x").is_err());
        assert!(target_to_uri("#a:example.org?via=evil").is_err());
    }
}
