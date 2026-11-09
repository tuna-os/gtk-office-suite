//! How snooze and undo (#527) record a message a Tuta account moved: by its
//! uid, since a Tuta listing carries no Message-ID until the body is read,
//! while a Tuta mail keeps its id, and so its uid, wherever it moves. Every
//! other account's mail goes by its Message-ID.

use crate::models::Message;

const TUTA_KEY: &str = "tuta-uid:";

/// The key of the Tuta message whose uid is `uid`.
pub(crate) fn tuta_key(uid: u32) -> String {
    format!("{TUTA_KEY}{uid}")
}

/// The uid a [`tuta_key`] stands for.
pub(crate) fn tuta_key_uid(key: &str) -> Option<u32> {
    key.strip_prefix(TUTA_KEY)?.parse().ok()
}

impl Message {
    /// Whether `key`, as snooze and undo record a moved message (see
    /// [`tuta_key`]), names this message.
    pub(crate) fn answers_to(&self, key: &str) -> bool {
        (!self.message_id.is_empty() && self.message_id == key)
            || tuta_key_uid(key) == Some(self.uid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Snooze and undo find a Tuta message again by its uid, and every
    /// other message by its Message-ID (#527).
    #[test]
    fn a_moved_message_answers_to_its_key() {
        assert_eq!(tuta_key_uid(&tuta_key(42)), Some(42));
        assert_eq!(tuta_key_uid("42"), None);
        assert_eq!(tuta_key_uid("tuta-uid:x"), None);
        let tuta = Message {
            uid: 42,
            ..Message::blank()
        };
        assert!(tuta.answers_to(&tuta_key(42)));
        assert!(!tuta.answers_to(&tuta_key(43)));
        // An empty Message-ID answers to nothing, an empty key included.
        assert!(!tuta.answers_to(""));
        let read = Message {
            message_id: "a@b".into(),
            ..tuta
        };
        assert!(read.answers_to("a@b") && read.answers_to(&tuta_key(42)));
    }
}
