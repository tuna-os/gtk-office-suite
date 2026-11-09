//! Characterization tests for the row's pure helpers, quirks included.

use super::faces::{first_recipient_addr, recipient_names};
use super::headings::due_label;
use super::{default_width, resizable};
use crate::config::ListColumn;

#[test]
fn recipient_names_prefer_the_display_name() {
    assert_eq!(recipient_names("Ann <a@x>, b@y"), "Ann, b@y");
    assert_eq!(recipient_names("\"Ann\" <a@x>"), "Ann");
    assert_eq!(recipient_names("<a@x>"), "a@x");
    assert_eq!(recipient_names(" , ,"), "");
    // Splits on every comma, quoted or not, a known quirk.
    assert_eq!(recipient_names("\"Doe, Jane\" <j@x>"), "\"Doe, Jane");
}

#[test]
fn the_first_recipient_address_is_the_first_entry() {
    assert_eq!(first_recipient_addr("Ann <a@x>, b@y"), Some("a@x".into()));
    assert_eq!(first_recipient_addr(" , b@y"), Some("b@y".into()));
    assert_eq!(first_recipient_addr(""), None);
    assert_eq!(first_recipient_addr("Ann <>"), None);
    // The quoted comma splits the name: the "address" is half of it.
    assert_eq!(
        first_recipient_addr("\"Doe, Jane\" <j@x>"),
        Some("\"Doe".into())
    );
}

#[test]
fn column_widths_and_resizing() {
    let widths: Vec<(ListColumn, i32, bool)> = ListColumn::ALL
        .iter()
        .map(|&c| (c, default_width(c), resizable(c)))
        .collect();
    assert_eq!(
        format!("{widths:?}"),
        "[(Star, -1, false), (Replied, -1, false), (Sender, 160, true), \
         (Recipients, 160, true), (Correspondents, 200, true), (Subject, -1, false), \
         (Tags, 90, true), (Attachment, -1, false), (Importance, -1, false), \
         (Account, 110, true), (Due, -1, true), (Date, -1, true)]"
    );
}

#[test]
fn no_due_date_has_no_label() {
    assert_eq!(due_label(0), "");
    assert_eq!(due_label(-1), "");
}
