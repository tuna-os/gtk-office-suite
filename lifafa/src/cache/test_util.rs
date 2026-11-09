//! Rows the cache tests put in place.

use rusqlite::params;

use super::messages::kind_to_i64;
use super::Cache;
use crate::models::FolderKind;

pub(super) fn add_msg(c: &Cache, folder: &str, uid: u32, from: &str, subject: &str, ts: i64) {
    c.conn.execute(
        "INSERT INTO messages (account_id, folder_path, uid, from_name, from_addr, subject, date, ts, unread, starred, has_attachment) \
         VALUES (1, ?1, ?2, ?3, '', ?4, '', ?5, 0, 0, 1)",
        params![folder, uid, from, subject, ts],
    ).unwrap();
}
pub(super) fn add_att(c: &Cache, folder: &str, uid: u32, idx: u32, name: &str, data: &[u8]) {
    c.conn.execute(
        "INSERT INTO attachments (account_id, folder_path, uid, idx, name, data) VALUES (1, ?1, ?2, ?3, ?4, ?5)",
        params![folder, uid, idx, name, data],
    ).unwrap();
}
pub(super) fn add_folder(c: &Cache, path: &str, kind: FolderKind) {
    c.conn.execute(
        "INSERT INTO folders (account_id, path, name, kind, unread, ord) VALUES (1, ?1, ?1, ?2, 0, 0)",
        params![path, kind_to_i64(kind)],
    ).unwrap();
}

/// Describe an attachment without downloading it, as the scan does.
pub(super) fn add_meta(c: &Cache, folder: &str, uid: u32, idx: u32, name: &str, size: u64) {
    c.save_attachment_meta(
        1,
        folder,
        uid,
        &[crate::models::AttachmentMeta {
            idx,
            name: name.to_string(),
            mime: String::new(),
            size,
            section: format!("{}", idx + 1),
        }],
    );
}
