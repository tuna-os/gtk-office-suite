// SPDX-License-Identifier: GPL-3.0-or-later
//! Open and save through a real GVfs remote location (RFC-0003 spike 1).
//!
//! Needs a GVfs mount, so it runs only when `TUNAOS_REMOTE_DIR` names one:
//! a directory URI such as `dav://localhost:8080/` that is already mounted
//! (`gio mount`). `tools/cloud-spike/run.sh` sets up a WebDAV server,
//! GVfs and the mount in a container and runs this.

use gtk4::gio;
use gtk4::gio::prelude::*;
use suite_common::locations::{commit_save, open_location, remote_uri, save_location, CHANGED_ELSEWHERE};

fn remote_dir() -> Option<gio::File> {
    std::env::var("TUNAOS_REMOTE_DIR").ok().map(|uri| gio::File::for_uri(&uri))
}

fn put(file: &gio::File, bytes: &[u8]) {
    file.replace_contents(bytes, None, false, gio::FileCreateFlags::NONE, None::<&gio::Cancellable>).unwrap();
}

fn get(file: &gio::File) -> Vec<u8> {
    file.load_contents(None::<&gio::Cancellable>).unwrap().0.to_vec()
}

#[test]
fn a_document_at_a_remote_location_opens_saves_and_refuses_to_overwrite_a_newer_one() {
    let Some(dir) = remote_dir() else {
        eprintln!("TUNAOS_REMOTE_DIR isn't set; skipping");
        return;
    };
    let file = dir.child("spike-book.xlsx");
    put(&file, b"version 1");
    println!("location: {} (local path: {:?})", file.uri(), file.path());

    // Open: a local copy with the same name.
    let staged = open_location(&file).unwrap();
    assert_eq!(staged.file_name().unwrap(), "spike-book.xlsx");
    assert_eq!(std::fs::read(&staged).unwrap(), b"version 1");
    if file.path().is_none() {
        assert_eq!(remote_uri(&staged).as_deref(), Some(file.uri().as_str()));
    }

    // Save: the app writes the staged path; commit uploads it.
    std::fs::write(&staged, b"version 2").unwrap();
    commit_save(&staged).unwrap();
    assert_eq!(get(&file), b"version 2");

    // Saved twice in a row: the etag from the first upload is current.
    std::fs::write(&staged, b"version 3").unwrap();
    commit_save(&staged).unwrap();
    assert_eq!(get(&file), b"version 3");

    // Someone else changes it: the next save is refused, and their
    // version stays.
    let etag_before = file.query_info("etag::value", gio::FileQueryInfoFlags::NONE, None::<&gio::Cancellable>).unwrap().etag();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    put(&file, b"their version");
    let etag_after = file.query_info("etag::value", gio::FileQueryInfoFlags::NONE, None::<&gio::Cancellable>).unwrap().etag();
    println!("etag before: {etag_before:?}, after someone else's save: {etag_after:?}");
    std::fs::write(&staged, b"my version").unwrap();
    match commit_save(&staged) {
        Err(e) => {
            assert!(e.starts_with(CHANGED_ELSEWHERE), "{e}");
            assert_eq!(get(&file), b"their version");
            println!("conflict detected: {e}");
        }
        Ok(()) => panic!(
            "the backend accepted a stale etag and overwrote the other version (etags {etag_before:?} -> {etag_after:?})"
        ),
    }

    // Save As to a new remote name.
    let copy = dir.child("spike-copy.xlsx");
    let _ = copy.delete(None::<&gio::Cancellable>);
    let target = save_location(&copy).unwrap();
    std::fs::write(&target, b"a copy").unwrap();
    commit_save(&target).unwrap();
    assert_eq!(get(&copy), b"a copy");
    let _ = copy.delete(None::<&gio::Cancellable>);
    let _ = file.delete(None::<&gio::Cancellable>);
}

#[test]
fn the_async_download_and_upload_report_progress_and_conflicts() {
    use suite_common::locations::{download, upload, UploadError};
    let Some(dir) = remote_dir() else {
        eprintln!("TUNAOS_REMOTE_DIR isn't set; skipping");
        return;
    };
    let file = dir.child("spike-async.xlsx");
    let big: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    put(&file, &big);
    let context = gtk4::glib::MainContext::default();
    let seen = std::cell::RefCell::new(Vec::new());
    let staged = context.block_on(download(&file, |done, total| seen.borrow_mut().push((done, total)))).unwrap();
    assert_eq!(std::fs::read(&staged).unwrap(), big);
    let seen = seen.into_inner();
    assert!(seen.len() > 1, "progress arrives in chunks: {seen:?}");
    assert_eq!(seen.last().unwrap().0, big.len() as u64);
    println!("progress reports: {} (total known: {:?})", seen.len(), seen.last().unwrap().1);

    std::fs::write(&staged, b"mine").unwrap();
    context.block_on(upload(&staged, true)).unwrap();
    assert_eq!(get(&file), b"mine");

    std::thread::sleep(std::time::Duration::from_millis(1100));
    put(&file, b"theirs");
    std::fs::write(&staged, b"mine again").unwrap();
    assert!(matches!(context.block_on(upload(&staged, true)), Err(UploadError::ChangedElsewhere(_))));
    assert_eq!(get(&file), b"theirs");
    // Overwrite, by choice.
    context.block_on(upload(&staged, false)).unwrap();
    assert_eq!(get(&file), b"mine again");
    let _ = file.delete(None::<&gio::Cancellable>);
}
