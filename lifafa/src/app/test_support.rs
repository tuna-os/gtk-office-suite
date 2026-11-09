//! Fixtures shared by the app's test modules.

use crate::models::Message;

pub(super) use crate::models::test_util::msg;

/// A conversation member as the related-lookup hands it over: what matters
/// here is which folder it came from and what Message-ID it carries.
pub(super) fn labelled(folder_id: u32, uid: u32, message_id: &str) -> Message {
    let mut m = msg(1, folder_id, uid);
    m.message_id = message_id.to_string();
    m
}
