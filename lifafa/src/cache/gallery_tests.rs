//! Tests of the attachments gallery's queries and the attachment scan.

use super::test_util::*;
use super::*;
use crate::models::{FolderKind, GallerySort};

/// A query over everything, ordered newest first.
fn all(sort: GallerySort, limit: u32, offset: u32) -> GalleryQuery<'static> {
    GalleryQuery {
        folders: &[],
        account_id: None,
        tokens: &[],
        bucket: 0,
        sort,
        limit,
        offset,
        data_cap: 10,
    }
}

/// The gallery lists what the scan found, whether or not the bytes were
/// ever downloaded — the whole point of the metadata tier.
#[test]
fn the_gallery_lists_attachments_that_were_never_downloaded() {
    let c = Cache::in_memory().unwrap();
    add_folder(&c, "INBOX", FolderKind::Inbox);
    add_folder(&c, "Archive", FolderKind::Archive);
    add_msg(&c, "INBOX", 1, "Alice", "Hi", 100);
    add_msg(&c, "Archive", 2, "Bob", "Report", 200);
    add_meta(&c, "INBOX", 1, 0, "a.png", 4);
    add_meta(&c, "Archive", 2, 0, "old.pdf", 900_000);
    // Only the inbox one has ever been fetched.
    add_att(&c, "INBOX", 1, 0, "a.png", &[0u8; 4]);

    let items = c.gallery_page(&all(GallerySort::Newest, 50, 0));
    assert_eq!(items.len(), 2);
    // Newest message first: Archive/Bob (200) before Inbox/Alice (100).
    assert_eq!(items[0].name, "old.pdf");
    assert!(!items[0].downloaded, "never fetched");
    assert!(items[0].data.is_none());
    assert_eq!(
        items[0].size, 900_000,
        "size comes from the scan, not the blob"
    );
    assert_eq!(items[1].name, "a.png");
    assert!(items[1].downloaded);
    assert_eq!(items[1].data.as_deref(), Some(&[0u8; 4][..]));
}

/// A cached file bigger than the page's cap is listed, and known to be
/// downloaded, but its bytes stay behind — a page has to stay small however
/// far back it reaches.
#[test]
fn a_cached_file_over_the_cap_is_listed_without_its_bytes() {
    let c = Cache::in_memory().unwrap();
    add_folder(&c, "INBOX", FolderKind::Inbox);
    add_msg(&c, "INBOX", 1, "Alice", "Hi", 100);
    add_meta(&c, "INBOX", 1, 0, "big.bin", 20);
    add_att(&c, "INBOX", 1, 0, "big.bin", &[0u8; 20]);

    let items = c.gallery_page(&all(GallerySort::Newest, 50, 0));
    assert_eq!(items.len(), 1);
    assert!(items[0].downloaded, "the bytes are in the cache");
    assert!(items[0].data.is_none(), "but over the cap, so not carried");
}

/// Paging must not drop or repeat a row. Every item has the same timestamp
/// here, so only the tie-break keeps the order stable.
#[test]
fn paging_covers_every_row_exactly_once_even_when_the_sort_key_ties() {
    let c = Cache::in_memory().unwrap();
    add_folder(&c, "INBOX", FolderKind::Inbox);
    for uid in 1..=10 {
        add_msg(&c, "INBOX", uid, "X", "S", 500);
        add_meta(&c, "INBOX", uid, 0, "f.png", 2);
    }
    let mut seen: Vec<(u32, String)> = Vec::new();
    for page in 0..4 {
        for item in c.gallery_page(&all(GallerySort::Newest, 3, page * 3)) {
            seen.push((item.uid, item.name.clone()));
        }
    }
    assert_eq!(seen.len(), 10, "every row, once");
    let mut uids: Vec<u32> = seen.iter().map(|(u, _)| *u).collect();
    uids.sort_unstable();
    uids.dedup();
    assert_eq!(uids.len(), 10, "no row served twice");
    assert_eq!(c.gallery_total(&all(GallerySort::Newest, 3, 0)), 10);
}

/// The scope reaches the query, so a page is cut from the right set rather
/// than filtered afterwards.
#[test]
fn the_folder_scope_narrows_the_query_itself() {
    let c = Cache::in_memory().unwrap();
    add_folder(&c, "INBOX", FolderKind::Inbox);
    add_folder(&c, "Sent", FolderKind::Sent);
    add_msg(&c, "INBOX", 1, "Alice", "Hi", 100);
    add_msg(&c, "Sent", 2, "Me", "Out", 200);
    add_meta(&c, "INBOX", 1, 0, "in.png", 2);
    add_meta(&c, "Sent", 2, 0, "out.png", 2);

    let inbox_only = [(1u32, "INBOX".to_string())];
    let q = GalleryQuery {
        folders: &inbox_only,
        ..all(GallerySort::Newest, 50, 0)
    };
    let items = c.gallery_page(&q);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "in.png");
    assert_eq!(c.gallery_total(&q), 1, "the count follows the same scope");
}

/// Search matches the filename, the sender, the subject and the type words
/// stored beside each row — and every word has to match something.
#[test]
fn search_matches_across_the_row_and_its_message() {
    let c = Cache::in_memory().unwrap();
    add_folder(&c, "INBOX", FolderKind::Inbox);
    add_msg(&c, "INBOX", 1, "Dana Whitfield", "Quarter review", 100);
    add_meta(&c, "INBOX", 1, 0, "budget.xlsx", 2);
    add_msg(&c, "INBOX", 2, "Bob", "Holiday", 200);
    add_meta(&c, "INBOX", 2, 0, "beach.png", 2);

    let hit = |tokens: &[String]| {
        c.gallery_page(&GalleryQuery {
            tokens,
            ..all(GallerySort::Newest, 50, 0)
        })
        .len()
    };
    assert_eq!(hit(&["budget".to_string()]), 1, "filename");
    assert_eq!(hit(&["whitfield".to_string()]), 1, "sender");
    assert_eq!(hit(&["quarter".to_string()]), 1, "subject");
    assert_eq!(hit(&["spreadsheet".to_string()]), 1, "type keyword");
    assert_eq!(hit(&["image".to_string()]), 1, "type keyword, other row");
    assert_eq!(
        hit(&["dana".to_string(), "spreadsheet".to_string()]),
        1,
        "both words have to match"
    );
    assert_eq!(hit(&["dana".to_string(), "holiday".to_string()]), 0);
}

/// A search word is matched literally: SQL's own wildcards are not the
/// user's to type by accident.
#[test]
fn a_search_word_cannot_smuggle_in_a_wildcard() {
    let c = Cache::in_memory().unwrap();
    add_folder(&c, "INBOX", FolderKind::Inbox);
    add_msg(&c, "INBOX", 1, "A", "S", 100);
    add_meta(&c, "INBOX", 1, 0, "report.pdf", 2);

    let wild = ["%".to_string()];
    let q = GalleryQuery {
        tokens: &wild,
        ..all(GallerySort::Newest, 50, 0)
    };
    assert_eq!(
        c.gallery_page(&q).len(),
        0,
        "% matches a literal percent sign"
    );
}

/// The type filter and the type sort both read the bucket stored with the
/// row, so they agree with the UI's own dropdown.
#[test]
fn the_type_filter_uses_the_stored_bucket() {
    let c = Cache::in_memory().unwrap();
    add_folder(&c, "INBOX", FolderKind::Inbox);
    add_msg(&c, "INBOX", 1, "A", "S", 100);
    add_meta(&c, "INBOX", 1, 0, "shot.png", 2);
    add_msg(&c, "INBOX", 2, "B", "T", 200);
    add_meta(&c, "INBOX", 2, 0, "doc.pdf", 2);

    let images = GalleryQuery {
        bucket: 1,
        ..all(GallerySort::Newest, 50, 0)
    };
    let items = c.gallery_page(&images);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "shot.png");
    assert_eq!(c.gallery_total(&images), 1);
}

/// Each sort criterion orders by what it says it does.
#[test]
fn every_sort_criterion_orders_the_page() {
    let c = Cache::in_memory().unwrap();
    add_folder(&c, "INBOX", FolderKind::Inbox);
    add_msg(&c, "INBOX", 1, "Zoe", "S", 100);
    add_meta(&c, "INBOX", 1, 0, "apple.pdf", 900);
    add_msg(&c, "INBOX", 2, "Adam", "T", 200);
    add_meta(&c, "INBOX", 2, 0, "zebra.png", 50);

    let first = |sort| c.gallery_page(&all(sort, 50, 0))[0].name.clone();
    assert_eq!(first(GallerySort::Newest), "zebra.png");
    assert_eq!(first(GallerySort::Oldest), "apple.pdf");
    assert_eq!(first(GallerySort::Name), "apple.pdf");
    assert_eq!(first(GallerySort::NameDesc), "zebra.png");
    assert_eq!(first(GallerySort::Largest), "apple.pdf");
    assert_eq!(first(GallerySort::Smallest), "zebra.png");
    assert_eq!(first(GallerySort::Sender), "zebra.png", "Adam before Zoe");
    assert_eq!(first(GallerySort::SenderDesc), "apple.pdf");
    assert_eq!(first(GallerySort::Type), "apple.pdf", "pdf before png");
    assert_eq!(first(GallerySort::TypeDesc), "zebra.png");
}

/// Re-describing a message replaces what was known about it, so a message
/// re-scanned after an edit does not accumulate ghost rows.
#[test]
fn rescanning_a_message_replaces_its_attachments() {
    let c = Cache::in_memory().unwrap();
    add_folder(&c, "INBOX", FolderKind::Inbox);
    add_msg(&c, "INBOX", 1, "A", "S", 100);
    add_meta(&c, "INBOX", 1, 0, "first.pdf", 2);
    add_meta(&c, "INBOX", 1, 0, "second.pdf", 2);
    let items = c.gallery_page(&all(GallerySort::Newest, 50, 0));
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "second.pdf");
}

/// A batch the scan wrote off wholesale must be askable again: the v15
/// re-queue drops the scan mark from any message recorded as holding
/// nothing, so the scan that now isolates the one message at fault gets
/// another look at its neighbours.
#[test]
fn messages_recorded_as_holding_nothing_can_be_re_queued() {
    let c = Cache::in_memory().unwrap();
    add_folder(&c, "INBOX", FolderKind::Inbox);
    add_msg(&c, "INBOX", 1, "A", "S", 100);
    add_msg(&c, "INBOX", 2, "B", "T", 200);
    add_meta(&c, "INBOX", 1, 0, "real.pdf", 2);
    c.save_attachment_meta(1, "INBOX", 2, &[]); // written off
    assert_eq!(c.unscanned_attachment_count(1, "INBOX"), 0);

    c.conn
        .execute(
            "DELETE FROM attachment_scan WHERE NOT EXISTS (\
                 SELECT 1 FROM attachment_meta am \
                 WHERE am.account_id = attachment_scan.account_id \
                   AND am.folder_path = attachment_scan.folder_path \
                   AND am.uid = attachment_scan.uid)",
            [],
        )
        .unwrap();

    assert_eq!(
        c.unscanned_attachment_uids(1, "INBOX", 10),
        vec![2],
        "only the empty one"
    );
    assert_eq!(
        c.gallery_page(&all(GallerySort::Newest, 50, 0)).len(),
        1,
        "the described one is untouched"
    );
}

/// The scan's work queue: messages flagged as carrying an attachment that
/// have not been described. A message with nothing in it is still marked,
/// or it would be asked about on every pass forever.
#[test]
fn the_scan_queue_empties_even_for_messages_holding_nothing() {
    let c = Cache::in_memory().unwrap();
    add_folder(&c, "INBOX", FolderKind::Inbox);
    add_msg(&c, "INBOX", 1, "A", "S", 100);
    add_msg(&c, "INBOX", 2, "B", "T", 200);
    // A third with no paperclip: never in the queue.
    c.conn
        .execute(
            "INSERT INTO messages (account_id, folder_path, uid, from_name, from_addr, \
             subject, date, ts, unread, starred, has_attachment) \
             VALUES (1, 'INBOX', 3, 'C', '', 'U', '', 300, 0, 0, 0)",
            [],
        )
        .unwrap();

    assert_eq!(c.unscanned_attachment_count(1, "INBOX"), 2);
    assert_eq!(
        c.unscanned_attachment_uids(1, "INBOX", 10),
        vec![2, 1],
        "newest first"
    );

    c.save_attachment_meta(1, "INBOX", 2, &[]); // described, holds nothing
    assert_eq!(c.unscanned_attachment_count(1, "INBOX"), 1);
    add_meta(&c, "INBOX", 1, 0, "x.pdf", 2);
    assert_eq!(c.unscanned_attachment_count(1, "INBOX"), 0);
    assert!(c.unscanned_attachment_uids(1, "INBOX", 10).is_empty());
}
