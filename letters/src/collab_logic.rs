// SPDX-License-Identifier: GPL-3.0-or-later
//
// collab_logic.rs — what a Letters session decides without GTK or a process:
// the words it shows, what a join code names, who "theirs" is. Plain functions,
// tested below.

use letters_core::collab::Unsupported;
use suite_collab::{ConnState, HeadInfo};

use crate::live::collab::Refusal;

/// The document model of the DCP update profile `loro/1` that Letters speaks
/// (docs/collab-spec/profiles/loro-1.md, section 8).
pub(crate) const DOC_MODEL: &str = "letters/1";

/// What a shared Letters document's versions are: a `.docx`.
pub(crate) const DOCX_MIME: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

/// A person's key, short enough to read in a list: its first characters.
pub(crate) fn short(principal: &str) -> String {
    match principal.split_once(':') {
        Some((scheme, key)) if key.chars().count() > 12 => format!("{scheme}:{}…", key.chars().take(12).collect::<String>()),
        _ => principal.to_owned(),
    }
}

/// The server a join code names: `streams:<server url>#<document id>`.
pub(crate) fn invite_server(code: &str) -> Option<String> {
    let (binding, rest) = code.trim().split_once(':')?;
    if binding != "streams" {
        return None;
    }
    let (server, doc) = rest.rsplit_once('#')?;
    (!server.is_empty() && !doc.is_empty()).then(|| server.to_owned())
}

/// The people a Share dialog's field names: keys separated by spaces,
/// commas or line breaks.
pub(crate) fn people(field: &str) -> Vec<String> {
    field.split(|c: char| c.is_whitespace() || c == ',').filter(|k| !k.is_empty()).map(str::to_owned).collect()
}

/// What the status button says: its icon, the state in a word (the first
/// part of its accessible name) and the whole name, `Sharing: online, 2 people`.
pub(crate) struct Words {
    pub icon: &'static str,
    #[cfg_attr(not(test), allow(dead_code))]
    pub state: String,
    pub label: String,
}

pub(crate) fn connection_words(state: &ConnState, people: usize) -> Words {
    let (icon, state) = match state {
        ConnState::Connecting => ("network-transmit-symbolic", suite_common::i18n("connecting")),
        ConnState::Online => ("network-transmit-receive-symbolic", suite_common::i18n("online")),
        ConnState::Offline { .. } => ("network-offline-symbolic", suite_common::i18n("offline")),
        ConnState::AuthRequired => ("dialog-password-symbolic", suite_common::i18n("sign-in needed")),
        ConnState::Error { .. } => ("dialog-warning-symbolic", suite_common::i18n("problem")),
    };
    let n = u64::try_from(people).unwrap_or(u64::MAX);
    let count = suite_common::ni18n("%d person", "%d people", n).replace("%d", &people.to_string());
    let label = format!("{}: {state}, {count}", suite_common::i18n("Sharing"));
    Words { icon, state, label }
}

/// What is not shared yet, as a noun phrase of a sentence.
fn unsupported_text(what: Unsupported, sharing: bool) -> String {
    let i = suite_common::i18n;
    match (what, sharing) {
        (Unsupported::Tables, true) => i("This document has a table, and shared documents cannot hold tables yet. Remove it, or share a copy without it."),
        (Unsupported::Footnotes, true) => i("This document has footnotes, and shared documents cannot hold footnotes yet. Remove them, or share a copy without them."),
        (Unsupported::HeadersFooters, true) => i("This document has a header or footer, and shared documents cannot hold them yet. Remove them, or share a copy without them."),
        (Unsupported::Sections, true) => i("This document has section breaks, and shared documents cannot hold them yet. Remove them, or share a copy without them."),
        (Unsupported::Comments, true) => i("This document has comments, and shared documents cannot hold comments yet. Delete them, or share a copy without them."),
        (Unsupported::RunStyle, true) => i("This document has character formatting that shared documents cannot hold yet. Remove it, or share a copy without it."),
        (Unsupported::Tables, false) => i("Tables are not supported in shared documents yet. Nothing was changed."),
        (Unsupported::Footnotes, false) => i("Footnotes are not supported in shared documents yet. Nothing was changed."),
        (Unsupported::HeadersFooters, false) => i("Headers and footers are not supported in shared documents yet. Nothing was changed."),
        (Unsupported::Sections, false) => i("Section breaks are not supported in shared documents yet. Nothing was changed."),
        (Unsupported::Comments, false) => i("Comments are not supported in shared documents yet. Nothing was changed."),
        (Unsupported::RunStyle, false) => i("This character formatting is not supported in shared documents yet. Nothing was changed."),
    }
}

/// The message for a document that cannot be shared: what it holds.
pub(crate) fn share_refusal(what: Unsupported) -> String {
    unsupported_text(what, true)
}

/// The message for an edit a shared document refused.
pub(crate) fn edit_refusal(refusal: &Refusal) -> String {
    match refusal {
        Refusal::Unsupported(what) => unsupported_text(*what, false),
        Refusal::ReadOnly => suite_common::i18n("You can only read this shared document. Nothing was changed."),
        Refusal::Replaced => suite_common::i18n("This document was replaced, so it is no longer shared."),
        Refusal::Other(why) => format!("{} {why}", suite_common::i18n("This change could not be shared and was not made:")),
    }
}

/// The version that is not this session's own (the other head of two), if any:
/// the first head this session did not publish.
pub(crate) fn their_head(heads: &[HeadInfo]) -> Option<&HeadInfo> {
    heads.iter().find(|h| !h.mine)
}

/// A join code is a single line of what the sidecar returned; what a person
/// pastes may carry spaces around it.
pub(crate) fn clean_code(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite_collab::Author;

    fn head(id: &str, mine: bool) -> HeadInfo {
        HeadInfo { id: id.into(), author: Author { principal: "ed25519:x".into(), device: "d".into() }, ts_ms: 1, message: None, covers: None, covered: false, mine }
    }

    #[test]
    fn a_join_code_names_its_server() {
        assert_eq!(invite_server("streams:http://127.0.0.1:4437/v1/stream/#abc123").as_deref(), Some("http://127.0.0.1:4437/v1/stream/"));
        assert_eq!(invite_server("  streams:https://h.example/s/#d  ").as_deref(), Some("https://h.example/s/"));
        assert_eq!(invite_server("matrix:https://h.example#d"), None);
        assert_eq!(invite_server("streams:nothing-after"), None);
        assert_eq!(invite_server("streams:#doc"), None);
        assert_eq!(invite_server(""), None);
    }

    #[test]
    fn people_are_split_on_spaces_commas_and_lines() {
        assert_eq!(people("ed25519:a, ed25519:b\ned25519:c  "), ["ed25519:a", "ed25519:b", "ed25519:c"]);
        assert!(people("  ,, \n").is_empty());
    }

    #[test]
    fn a_key_is_shortened_for_a_list_but_never_when_short() {
        assert_eq!(short("ed25519:ABCDEFGHIJKLMNOPQRSTUV"), "ed25519:ABCDEFGHIJKL…");
        assert_eq!(short("ed25519:abc"), "ed25519:abc");
        assert_eq!(short("someone"), "someone");
    }

    #[test]
    fn the_status_name_says_the_state_in_words_and_counts_people() {
        let online = connection_words(&ConnState::Online, 2);
        assert!(online.label.starts_with("Sharing: online"), "{}", online.label);
        assert!(online.label.contains('2'), "{}", online.label);
        let alone = connection_words(&ConnState::Offline { since_ms: 1 }, 1);
        assert!(alone.label.starts_with("Sharing: offline"), "{}", alone.label);
        for state in [ConnState::Connecting, ConnState::AuthRequired, ConnState::Error { message: "x".into() }] {
            let w = connection_words(&state, 1);
            assert!(!w.state.is_empty() && w.label.contains(&w.state) && !w.icon.is_empty());
        }
    }

    #[test]
    fn theirs_is_the_head_this_session_did_not_publish() {
        assert_eq!(their_head(&[head("a", true), head("b", false)]).map(|h| h.id.as_str()), Some("b"));
        assert!(their_head(&[head("a", true)]).is_none());
        assert!(their_head(&[]).is_none());
    }

    #[test]
    fn every_unsupported_thing_has_its_own_words() {
        use std::collections::HashSet;
        let all = [Unsupported::Tables, Unsupported::Footnotes, Unsupported::HeadersFooters, Unsupported::Sections, Unsupported::Comments, Unsupported::RunStyle];
        let share: HashSet<String> = all.iter().map(|w| share_refusal(*w)).collect();
        let edit: HashSet<String> = all.iter().map(|w| edit_refusal(&Refusal::Unsupported(*w))).collect();
        assert_eq!((share.len(), edit.len()), (6, 6));
        assert!(edit_refusal(&Refusal::Unsupported(Unsupported::Tables)).starts_with("Tables"));
        assert!(share_refusal(Unsupported::Footnotes).contains("footnotes"));
    }

    #[test]
    fn a_pasted_code_loses_its_whitespace() {
        assert_eq!(clean_code(" streams:http://h/s/#d\n "), "streams:http://h/s/#d");
    }
}
