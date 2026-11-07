//! The invite `share` returns and `join` takes: `<binding>:<server url>#<document id>`.
//!
//! It names where the document is and nothing else: no key, no token. For
//! Streams the person joining must already be on the roster with their public key
//! (the owner added it at `share` or with `grant`), and that is what lets them read.

use suite_collab::Binding;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invite {
    pub binding: Binding,
    /// Always ends with `/` (stream paths are appended to it).
    pub server: String,
    pub doc: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InviteError(pub String);

impl std::fmt::Display for InviteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not an invite: {}", self.0)
    }
}

pub fn normalise_server(server: &str) -> String {
    let s = server.trim();
    if s.ends_with('/') {
        s.to_owned()
    } else {
        format!("{s}/")
    }
}

impl Invite {
    pub fn new(binding: Binding, server: &str, doc: &str) -> Self {
        Invite {
            binding,
            server: normalise_server(server),
            doc: doc.to_owned(),
        }
    }

    pub fn encode(&self) -> String {
        format!("{}:{}#{}", self.binding.as_str(), self.server, self.doc)
    }

    pub fn parse(text: &str) -> Result<Invite, InviteError> {
        let text = text.trim();
        let (name, rest) = text
            .split_once(':')
            .ok_or_else(|| InviteError("no binding name".into()))?;
        let binding = match name {
            "streams" => Binding::Streams,
            "matrix" => Binding::Matrix,
            other => return Err(InviteError(format!("unknown binding {other:?}"))),
        };
        let (server, doc) = rest
            .rsplit_once('#')
            .ok_or_else(|| InviteError("no document id after `#`".into()))?;
        if doc.is_empty() || doc.chars().any(char::is_whitespace) {
            return Err(InviteError("empty or spaced document id".into()));
        }
        if !(server.starts_with("https://") || server.starts_with("http://")) {
            return Err(InviteError("the server is not an http(s) URL".into()));
        }
        Ok(Invite::new(binding, server, doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_invite_round_trips() {
        let i = Invite::new(Binding::Streams, "https://s.example/v1/stream", "AbC-_9");
        assert_eq!(i.server, "https://s.example/v1/stream/");
        assert_eq!(Invite::parse(&i.encode()).unwrap(), i);
    }

    #[test]
    fn a_matrix_room_id_with_colons_and_a_bang_survives() {
        let i = Invite::new(Binding::Matrix, "https://matrix.example", "!room:matrix.example");
        assert_eq!(Invite::parse(&i.encode()).unwrap(), i);
    }

    #[test]
    fn garbage_is_refused() {
        for bad in [
            "",
            "streams",
            "streams:https://s/",
            "streams:https://s/#",
            "ftp:https://s/#d",
            "streams:file:///etc/passwd#d",
            "streams:https://s/#a b",
        ] {
            assert!(Invite::parse(bad).is_err(), "{bad:?}");
        }
    }
}
